use executorch_bencher::config::Config;
use executorch_bencher::db;
use sqlx::Row;

/// Exercises the exact same startup path `main.rs` runs on every restart
/// (`Config::from_env` -> `db::connect_and_migrate`) against the existing
/// on-disk database file, and confirms previously stored rows are still
/// readable afterward.
#[tokio::main]
async fn main() {
    let config = Config::from_env().expect("failed to load config from .env");
    let pool = db::connect_and_migrate(&config.database_url)
        .await
        .expect("restart: failed to connect and migrate against existing database file");

    let count: i64 = sqlx::query("SELECT count(*) AS c FROM runs")
        .fetch_one(&pool)
        .await
        .expect("failed to count runs")
        .get("c");
    println!("restart successful: {count} run(s) readable from existing database file");
    assert!(
        count > 0,
        "expected previously stored data to still be present"
    );
}
