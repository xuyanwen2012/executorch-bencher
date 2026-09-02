mod common;

use executorch_bencher::domain::validate_env_vars;
use executorch_bencher::runs::{get_run, insert_run};
use uuid::Uuid;

#[tokio::test]
async fn environment_allowlist_json_preserves_unset_vs_empty_across_a_round_trip() {
    let (pool, ctx) = common::migrated_pool().await;
    let id = Uuid::now_v7();
    let mut new_run = common::seed_new_run(&ctx, id).await;
    new_run.env_vars_json = validate_env_vars(r#"{"EXPERIMENT_UNSET":null,"EXPERIMENT_EMPTY":""}"#)
        .expect("env vars should validate");

    insert_run(&pool, &new_run)
        .await
        .expect("insert should succeed");
    let fetched = get_run(&pool, id)
        .await
        .expect("get should succeed")
        .expect("run should exist");

    let value: serde_json::Value = serde_json::from_str(&fetched.env_vars_json).unwrap();
    assert!(value["EXPERIMENT_UNSET"].is_null());
    assert_eq!(
        value["EXPERIMENT_EMPTY"],
        serde_json::Value::String(String::new())
    );
    assert_ne!(value["EXPERIMENT_UNSET"], value["EXPERIMENT_EMPTY"]);
}
