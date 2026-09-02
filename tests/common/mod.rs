use executorch_bencher::config::{Config, StorageLimits};
use executorch_bencher::db;
use executorch_bencher::domain::{CorrectnessResult, DeviceClass, ExitStatus, Sha256Hex};
use executorch_bencher::model_registry::{ExternalModelStorage, ModelStorage};
use executorch_bencher::runs::{
    AndroidDeviceState, AndroidLabConfig, HostState, LinuxHostState, NewRun,
};
use sqlx::SqlitePool;
use std::path::PathBuf;
use uuid::Uuid;

/// Holds a hermetic SQLite pool (migrated) and fresh storage-root
/// directories, plus the temp-directory guards that keep both alive for the
/// duration of a test.
#[allow(dead_code)]
pub struct TestContext {
    pub pool: SqlitePool,
    /// The `sqlite://` URL the pool was opened from, for tests that need
    /// to re-open the same database file (e.g. simulating a restart).
    pub database_url: String,
    pub artifact_root: PathBuf,
    pub temporary_dir: PathBuf,
    pub model_root: PathBuf,
    _db_dir: tempfile::TempDir,
    _data_dir: tempfile::TempDir,
}

#[allow(dead_code)]
pub async fn test_context() -> TestContext {
    let db_dir = tempfile::tempdir().expect("failed to create temp db dir");
    let data_dir = tempfile::tempdir().expect("failed to create temp data dir");
    let db_path = db_dir.path().join("benchmarks.sqlite3");
    let database_url = format!("sqlite://{}", db_path.display());

    let pool = db::connect_and_migrate(&database_url)
        .await
        .expect("failed to connect and migrate");

    TestContext {
        pool,
        database_url,
        artifact_root: data_dir.path().join("artifacts"),
        temporary_dir: data_dir.path().join("temporary"),
        model_root: data_dir.path().join("models"),
        _db_dir: db_dir,
        _data_dir: data_dir,
    }
}

/// Convenience for tests that only need a migrated pool, not the storage
/// directories (kept alongside so their temp dirs aren't dropped).
#[allow(dead_code)]
pub async fn migrated_pool() -> (SqlitePool, TestContext) {
    let ctx = test_context().await;
    let pool = ctx.pool.clone();
    (pool, ctx)
}

impl TestContext {
    /// A `Config` pointing at this context's storage roots, for tests that
    /// exercise the HTTP layer (which takes a `Config`, not bare paths).
    #[allow(dead_code)]
    pub fn config(&self) -> Config {
        Config {
            listen_addr: executorch_bencher::config::DEFAULT_LISTEN_ADDR
                .parse()
                .unwrap(),
            database_url: "sqlite::memory:".to_string(),
            data_root: self._data_dir.path().to_path_buf(),
            artifact_root: self.artifact_root.clone(),
            model_root: self.model_root.clone(),
            model_register_roots: vec![self.model_root.clone()],
            temporary_dir: self.temporary_dir.clone(),
            trash_dir: self._data_dir.path().join("trash"),
            dashboard_dist: None,
            limits: StorageLimits::default(),
            events_keep_alive_seconds: 15,
        }
    }

    /// Registers (or reuses) one small throwaway external model file, for
    /// tests that just need *some* valid `model_asset_id` to satisfy the
    /// `runs.model_asset_id` foreign key. Every call writes the same fixed
    /// content, so repeated calls dedupe onto the same `model_assets` row.
    #[allow(dead_code)]
    pub async fn shared_test_model(&self) -> Uuid {
        let path = self._data_dir.path().join("shared-test-model.pte");
        std::fs::write(&path, b"fake model bytes for tests")
            .expect("failed to write test model file");
        let asset = ExternalModelStorage
            .register(&self.pool, &path)
            .await
            .expect("test model registration should succeed");
        asset.id
    }
}

/// The complete lab (internal-device) Android snapshot `build_new_run` uses.
#[allow(dead_code)]
pub fn android_device_state() -> AndroidDeviceState {
    AndroidDeviceState::internal(
        AndroidLabConfig {
            bsp_version: "bsp-1.0".to_string(),
            sumd_driver_version: "sumd-1.0".to_string(),
            gpu_clock_mhz: 980,
            mif_clock_mhz: 5333,
            int_clock_mhz: 934,
        },
        100,
        false,
        20.0,
        25.0,
        false,
    )
}

/// What a retail, unrooted phone can report: build, kernel, SoC, GPU; no
/// BSP, SUMD, clocks, battery, or temperatures.
#[allow(dead_code)]
pub fn external_android_device_state() -> AndroidDeviceState {
    AndroidDeviceState {
        os: Some("Android 16 (BP4A.251205.006)".to_string()),
        kernel: Some("6.1.157-android14-11".to_string()),
        soc: Some("s5e9945".to_string()),
        cpu_count: Some(10),
        memory_bytes: Some(11_472_032 * 1024),
        gpu: Some("Samsung Xclipse 940".to_string()),
        gpu_driver: None,
        ..Default::default()
    }
}

/// Mutable access to a run's lab config; panics unless it is a lab Android run.
#[allow(dead_code)]
pub fn android_lab_mut(run: &mut NewRun) -> &mut AndroidLabConfig {
    android_mut(run).lab.as_mut().expect("expected a lab android run")
}

/// `build_new_run` re-targeted at an external (retail) phone.
#[allow(dead_code)]
pub fn build_new_external_android_run(id: Uuid, model_asset_id: Uuid, serial: &str) -> NewRun {
    let mut run = build_new_run(id, model_asset_id);
    run.device_class = DeviceClass::External;
    run.device_serial = serial.to_string();
    run.device_model = Some("SM-S926B".to_string());
    run.host = HostState::Android(external_android_device_state());
    run
}

/// A representative Linux workstation snapshot.
#[allow(dead_code)]
pub fn linux_host_state() -> LinuxHostState {
    LinuxHostState {
        os: "Ubuntu 24.04.4 LTS".to_string(),
        kernel: "7.0.0-30-generic".to_string(),
        cpu_model: "AMD EPYC 4464P 12-Core Processor".to_string(),
        cpu_count: Some(16),
        memory_bytes: Some(16_299_392 * 1024),
        accelerator: "Intel(R) Arc(tm) B580 Graphics (BMG G21)".to_string(),
        accelerator_driver: Some("Mesa 25.2.8".to_string()),
        uptime_seconds: None,
        thermal_throttling: None,
    }
}

/// Mutable access to a run's Android snapshot; panics for a Linux run.
#[allow(dead_code)]
pub fn android_mut(run: &mut NewRun) -> &mut AndroidDeviceState {
    match &mut run.host {
        HostState::Android(a) => a,
        HostState::Linux(_) => panic!("expected an android run"),
    }
}

/// `build_new_run` re-targeted at a Linux host named `hostname`.
#[allow(dead_code)]
pub fn build_new_linux_run(id: Uuid, model_asset_id: Uuid, hostname: &str) -> NewRun {
    let mut run = build_new_run(id, model_asset_id);
    run.device_class = DeviceClass::External;
    run.device_serial = hostname.to_string();
    run.host = HostState::Linux(linux_host_state());
    run
}

/// A fully populated `NewRun` with sensible defaults, for tests that only
/// care about a subset of fields. Every field can be overridden on the
/// returned value before insertion.
#[allow(dead_code)]
pub fn build_new_run(id: Uuid, model_asset_id: Uuid) -> NewRun {
    NewRun {
        id,
        started_at: chrono::Utc::now(),
        finished_at: None,
        repetition: 0,
        command_args_json: r#"["--model","resnet50"]"#.to_string(),
        command_line: Some("./run.sh resnet50".to_string()),
        input_parameters_json: "{}".to_string(),
        env_vars_json: "{}".to_string(),
        env_allowlist_version: "v1".to_string(),
        collector_version: "collector-0.1".to_string(),

        device_class: DeviceClass::Internal,
        device_serial: "device-001".to_string(),
        device_model: None,
        host: HostState::Android(android_device_state()),

        git_commit_sha: "abc123".to_string(),
        git_dirty: false,
        git_branch: None,
        git_commit_timestamp: None,
        git_commit_subject: None,
        executable_sha256: Some(Sha256Hex::try_from("a".repeat(64)).unwrap()),
        model_asset_id,
        prompt_sha256: Sha256Hex::try_from("c".repeat(64)).unwrap(),
        input_token_count: 10,
        output_token_count: 20,

        prefill_tokens_per_sec: 100.0,
        decode_tokens_per_sec: Some(50.0),
        exit_status: ExitStatus::Succeeded,
        correctness_result: CorrectnessResult::NotChecked,
        input_artifact_id: None,
        output_artifact_id: None,
        output_preview: None,
        stdout_artifact_id: None,
        stderr_artifact_id: None,
        crash_artifact_id: None,
        error_summary: None,
    }
}

/// Registers a shared throwaway model against `ctx` and builds a fully
/// populated `NewRun` referencing it.
#[allow(dead_code)]
pub async fn seed_new_run(ctx: &TestContext, id: Uuid) -> NewRun {
    let model_asset_id = ctx.shared_test_model().await;
    build_new_run(id, model_asset_id)
}
