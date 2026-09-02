mod common;

use executorch_bencher::artifact_store::{self, ArtifactKind};
use executorch_bencher::db;
use executorch_bencher::model_registry::{self, ExternalModelStorage, ModelStorage};
use executorch_bencher::runs::{get_run, insert_run};
use uuid::Uuid;

#[tokio::test]
async fn restarting_against_the_same_database_file_reads_back_previously_stored_data() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = dir.path().join("benchmarks.sqlite3");
    let database_url = format!("sqlite://{}", db_path.display());
    let model_path = dir.path().join("model.pte");
    std::fs::write(&model_path, b"fake model bytes").expect("failed to write model file");
    let artifact_root = dir.path().join("artifacts");
    let temporary_dir = dir.path().join("temporary");

    let id = Uuid::now_v7();
    let model_asset_id;
    let artifact_id;
    {
        let pool = db::connect_and_migrate(&database_url)
            .await
            .expect("first connect_and_migrate should succeed");
        model_asset_id = ExternalModelStorage
            .register(&pool, &model_path)
            .await
            .expect("model registration should succeed")
            .id;
        artifact_id = artifact_store::store_artifact_bytes(
            &pool,
            &artifact_root,
            &temporary_dir,
            ArtifactKind::Prompt,
            Some("prompt.txt"),
            None,
            b"restart persistence prompt".to_vec(),
        )
        .await
        .expect("artifact store should succeed")
        .id;
        insert_run(&pool, &common::build_new_run(id, model_asset_id))
            .await
            .expect("insert should succeed");
        pool.close().await;
    }

    // Simulate an application restart: reopen a fresh pool against the same
    // on-disk file and re-run migrations (as `main` does on every startup).
    let pool = db::connect_and_migrate(&database_url)
        .await
        .expect("second connect_and_migrate should succeed");
    let fetched = get_run(&pool, id)
        .await
        .expect("get should succeed")
        .expect("previously stored run should still exist after restart");
    assert_eq!(fetched.id, id);
    assert_eq!(fetched.model_asset_id, model_asset_id);

    let model_asset = model_registry::get_model_asset(&pool, model_asset_id)
        .await
        .expect("model asset lookup should succeed")
        .expect("model asset should still be registered after restart");
    assert_eq!(model_asset.id, model_asset_id);

    let artifact_record = artifact_store::get_artifact_record(&pool, artifact_id)
        .await
        .expect("artifact lookup should succeed")
        .expect("artifact should still be registered after restart");
    let mut content = artifact_store::open_artifact_content(&artifact_root, &artifact_record)
        .await
        .expect("artifact content should still be readable after restart");
    let mut buf = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut content, &mut buf)
        .await
        .expect("failed to read artifact content");
    assert_eq!(buf, b"restart persistence prompt");
}
