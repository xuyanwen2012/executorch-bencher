mod common;

use executorch_bencher::runs::{get_run, insert_run};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn concurrent_reads_succeed_while_a_short_write_transaction_is_in_progress() {
    let (pool, ctx) = common::migrated_pool().await;
    let id = Uuid::now_v7();
    insert_run(&pool, &common::seed_new_run(&ctx, id).await)
        .await
        .expect("seed insert should succeed");

    let writer_pool = pool.clone();
    let writer = tokio::spawn(async move {
        let mut tx = writer_pool
            .begin()
            .await
            .expect("failed to begin transaction");
        sqlx::query("UPDATE runs SET error_summary = 'writing' WHERE id = ?")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await
            .expect("update within transaction should succeed");
        // Hold the write transaction open briefly so a concurrent read is
        // guaranteed to overlap it.
        tokio::time::sleep(Duration::from_millis(150)).await;
        tx.commit().await.expect("commit should succeed");
    });

    // Give the writer a head start so its transaction is open before the
    // read below is issued.
    tokio::time::sleep(Duration::from_millis(30)).await;

    let reader_pool = pool.clone();
    let reader = tokio::spawn(async move { get_run(&reader_pool, id).await });

    let read_result = reader.await.expect("reader task should not panic");
    let run = read_result
        .expect("read should succeed while a write transaction is in progress")
        .expect("run should exist");
    assert_eq!(run.id, id);

    writer.await.expect("writer task should not panic");
}
