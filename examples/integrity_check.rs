//! Read-only storage/database integrity report, run explicitly (never on
//! every startup): `cargo run --example integrity_check`.
//!
//! Reports, without modifying anything: unreferenced artifact rows,
//! artifact/model rows with missing files, files on disk with no database
//! row, and unavailable external models. See `src/integrity.rs` and
//! `specs/artifact-storage` - "Storage integrity is reconciled through a
//! read-only report, not automatic deletion".

use executorch_bencher::config::Config;
use executorch_bencher::db;
use executorch_bencher::integrity;

#[tokio::main]
async fn main() {
    let config = Config::from_env().expect("failed to load config from .env");
    let pool = db::connect_and_migrate(&config.database_url)
        .await
        .expect("failed to connect and migrate");

    let report = integrity::check(&pool, &config.artifact_root, &config.model_root)
        .await
        .expect("integrity check failed");

    println!("Storage integrity report");
    println!("=========================");
    println!(
        "Unreferenced artifacts:        {}",
        report.unreferenced_artifacts.len()
    );
    for id in &report.unreferenced_artifacts {
        println!("  - {id}");
    }
    println!(
        "Artifacts with missing files:  {}",
        report.artifacts_missing_files.len()
    );
    for id in &report.artifacts_missing_files {
        println!("  - {id}");
    }
    println!(
        "Orphaned artifact files:       {}",
        report.orphaned_artifact_files.len()
    );
    for path in &report.orphaned_artifact_files {
        println!("  - {}", path.display());
    }
    println!(
        "Orphaned model files:          {}",
        report.orphaned_model_files.len()
    );
    for path in &report.orphaned_model_files {
        println!("  - {}", path.display());
    }
    println!(
        "Unavailable external models:   {}",
        report.unavailable_models.len()
    );
    for id in &report.unavailable_models {
        println!("  - {id}");
    }

    if report.is_clean() {
        println!("\nNo issues found.");
    } else {
        println!("\nIssues found - see above. This report is read-only; nothing was modified.");
        std::process::exit(1);
    }
}
