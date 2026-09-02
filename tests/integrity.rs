//! The read-only storage/database reconciliation report. See
//! `specs/artifact-storage` - "Storage integrity is reconciled through a
//! read-only report".

mod common;

use executorch_bencher::artifact_store::{ArtifactKind, store_artifact_bytes};
use executorch_bencher::integrity;
use executorch_bencher::runs::insert_run;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[tokio::test]
async fn a_consistent_store_reports_clean_and_each_drift_is_named_without_modifying_anything() {
    let ctx = common::test_context().await;

    let stdout = store_artifact_bytes(
        &ctx.pool,
        &ctx.artifact_root,
        &ctx.temporary_dir,
        ArtifactKind::Stdout,
        Some("stdout.log"),
        Some("text/plain"),
        b"referenced content".to_vec(),
    )
    .await
    .unwrap();
    let id = Uuid::now_v7();
    let mut run = common::seed_new_run(&ctx, id).await;
    run.stdout_artifact_id = Some(stdout.id);
    insert_run(&ctx.pool, &run).await.unwrap();

    let report = integrity::check(&ctx.pool, &ctx.artifact_root, &ctx.model_root)
        .await
        .unwrap();
    assert!(report.is_clean(), "fresh store should be clean: {report:?}");

    // An artifact row nobody references.
    let orphan_row = store_artifact_bytes(
        &ctx.pool,
        &ctx.artifact_root,
        &ctx.temporary_dir,
        ArtifactKind::Stderr,
        Some("stderr.log"),
        Some("text/plain"),
        b"unreferenced content".to_vec(),
    )
    .await
    .unwrap();

    // A file under the artifact root no row describes.
    let stray = ctx.artifact_root.join("sha256").join("zz").join("not-an-artifact");
    std::fs::create_dir_all(stray.parent().unwrap()).unwrap();
    std::fs::write(&stray, b"stray").unwrap();

    // The referenced artifact's file goes missing.
    let referenced_file = files_under(&ctx.artifact_root)
        .into_iter()
        .find(|p| p.file_name().and_then(|n| n.to_str()) == Some(stdout.sha256.as_str()))
        .expect("stored artifact file should exist");
    std::fs::remove_file(&referenced_file).unwrap();

    let before: Vec<PathBuf> = files_under(&ctx.artifact_root);
    let report = integrity::check(&ctx.pool, &ctx.artifact_root, &ctx.model_root)
        .await
        .unwrap();
    assert!(!report.is_clean());
    assert_eq!(report.unreferenced_artifacts, vec![orphan_row.id]);
    assert_eq!(report.artifacts_missing_files, vec![stdout.id]);
    assert_eq!(report.orphaned_artifact_files.len(), 1);
    assert!(report.orphaned_artifact_files[0].ends_with("not-an-artifact"));
    assert!(report.unavailable_models.is_empty());

    // Read-only: the report changed no file and no row.
    assert_eq!(files_under(&ctx.artifact_root), before);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(rows, 2);
}
