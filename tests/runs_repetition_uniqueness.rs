mod common;

use executorch_bencher::runs::{get_run, insert_run};
use uuid::Uuid;

#[tokio::test]
async fn multiple_repetitions_and_retries_persist_independently() {
    let (pool, ctx) = common::migrated_pool().await;

    let first_id = Uuid::now_v7();
    let mut first = common::seed_new_run(&ctx, first_id).await;
    first.repetition = 0;
    insert_run(&pool, &first)
        .await
        .expect("first repetition should insert");

    // A retry of the same logical repetition number: the MVP schema does
    // not tie uniqueness to a shared configuration/device grouping, so two
    // rows with the same repetition number are both accepted as
    // independent, immutable attempts.
    let second_id = Uuid::now_v7();
    let mut second = common::seed_new_run(&ctx, second_id).await;
    second.repetition = 0;
    insert_run(&pool, &second)
        .await
        .expect("retried repetition should insert");

    let third_id = Uuid::now_v7();
    let mut third = common::seed_new_run(&ctx, third_id).await;
    third.repetition = 1;
    insert_run(&pool, &third)
        .await
        .expect("second repetition should insert");

    for id in [first_id, second_id, third_id] {
        let fetched = get_run(&pool, id)
            .await
            .expect("get should succeed")
            .expect("run should exist");
        assert_eq!(fetched.id, id);
    }
}

#[tokio::test]
async fn a_duplicate_run_id_is_rejected_as_a_primary_key_violation() {
    let (pool, ctx) = common::migrated_pool().await;
    let id = Uuid::now_v7();

    let first = common::seed_new_run(&ctx, id).await;
    insert_run(&pool, &first)
        .await
        .expect("first insert should succeed");

    let duplicate = common::seed_new_run(&ctx, id).await;
    let err = insert_run(&pool, &duplicate)
        .await
        .expect_err("duplicate run id should be rejected");
    assert!(err.to_string().to_lowercase().contains("unique"));
}
