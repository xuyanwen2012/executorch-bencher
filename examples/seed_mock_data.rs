//! Seeds the DEV (mock) database with fake Android and Linux runs so the
//! dashboard has both platforms to render. Never run this against the real
//! profile: `just seed-mock` loads `.env`, the dev profile, for you.
//!
//! Idempotent: skips when runs from this seeder already exist.

use chrono::{Duration, Utc};
use executorch_bencher::artifact_store::{ArtifactKind, store_artifact_bytes};
use executorch_bencher::config::Config;
use executorch_bencher::db;
use executorch_bencher::domain::{CorrectnessResult, DeviceClass, ExitStatus, Sha256Hex};
use executorch_bencher::model_registry::{ExternalModelStorage, ModelStorage};
use executorch_bencher::runs::{
    AndroidDeviceState, AndroidLabConfig, HostState, LinuxHostState, NewRun, insert_run,
};
use sqlx::Row;
use uuid::Uuid;

const COLLECTOR: &str = "seed-mock-data/0.1";

#[tokio::main]
async fn main() {
    let config = Config::from_env().expect("failed to load config from .env");
    if !config.database_url.contains("data/dev/") {
        eprintln!(
            "refusing to seed mock data into {} - only the dev profile (data/dev/) takes fakes",
            config.database_url
        );
        std::process::exit(1);
    }
    config
        .prepare_storage_roots()
        .expect("failed to prepare storage roots");
    let pool = db::connect_and_migrate(&config.database_url)
        .await
        .expect("failed to connect and migrate");

    let existing: i64 = sqlx::query("SELECT count(*) AS c FROM runs WHERE collector_version = ?")
        .bind(COLLECTOR)
        .fetch_one(&pool)
        .await
        .expect("count")
        .get("c");
    if existing > 0 {
        println!("mock data already seeded ({existing} runs); nothing to do");
        return;
    }

    let models_dir = config.data_root.join("mock-models");
    std::fs::create_dir_all(&models_dir).expect("mock model dir");
    let mut model_ids = Vec::new();
    for name in ["llama3_2-1b_vulkan_8da4w.pte", "llama3_2-3b_vulkan_4w.pte"] {
        let path = models_dir.join(name);
        std::fs::write(&path, format!("fake model bytes for {name}")).expect("write model");
        let asset = ExternalModelStorage
            .register(&pool, &path)
            .await
            .expect("register model");
        model_ids.push(asset.id);
    }

    let prompt = "Explain the difference between prefill and decode in one paragraph.";
    let prompt_artifact = store_artifact_bytes(
        &pool,
        &config.artifact_root,
        &config.temporary_dir,
        ArtifactKind::Prompt,
        Some("prompt.txt"),
        Some("text/plain"),
        prompt.as_bytes().to_vec(),
    )
    .await
    .expect("prompt artifact");
    let prompt_sha256 = Sha256Hex::try_from(prompt_artifact.sha256).unwrap();

    let commits = [
        ("1111111111111111111111111111111111111111", "2026-08-20T10:00:00Z", "Baseline Vulkan prefill"),
        ("2222222222222222222222222222222222222222", "2026-08-27T15:30:00Z", "Fuse RMSNorm into attention"),
        ("3333333333333333333333333333333333333333", "2026-09-01T09:00:00Z", "Tune workgroup sizes"),
    ];
    // Two lab (internal) phones with the full snapshot, and one retail
    // (external) phone that can only report build, SoC, and GPU.
    let android_hosts = [
        ("R5CX12ABCDE", Some(("bsp-2.3.1", "sumd-1.8.0")), "SM-G998B"),
        ("R5CX99ZZZZZ", Some(("bsp-2.4.0", "sumd-1.9.0")), "SM-G998B"),
        ("R5CY21Y3VEV", None, "SM-S926B"),
    ];
    let linux_hosts = [
        ("ubuntu-lts", "Ubuntu 26.04.1 LTS", "7.0.0-30-generic", "Intel(R) Core(TM) i9-14900K", 32, "NVIDIA GeForce RTX 4070 Ti SUPER", "595.84"),
        ("rocky-ryzen", "Rocky Linux 10.2 (Red Quartz)", "6.12.0-211.49.1.el10_2.x86_64", "AMD Ryzen 9 7940HS w/ Radeon 780M Graphics", 16, "AMD Radeon 780M Graphics (RADV PHOENIX)", "Mesa 25.2.7"),
    ];

    let mut seq = 0u64;
    let mut jitter = |base: f64| {
        seq += 1;
        base * (1.0 + ((seq * 7919) % 13) as f64 / 200.0 - 0.03)
    };
    let mut count = 0usize;
    for (ci, (sha, ts, subject)) in commits.iter().enumerate() {
        let commit_time = chrono::DateTime::parse_from_rfc3339(ts).unwrap().with_timezone(&Utc);
        for (mi, model_id) in model_ids.iter().enumerate() {
            let base_prefill = if mi == 0 { 900.0 } else { 380.0 } * (1.0 + ci as f64 * 0.08);
            let base_decode = if mi == 0 { 42.0 } else { 19.0 } * (1.0 + ci as f64 * 0.05);
            for (hi, (serial, lab, model_name)) in android_hosts.iter().enumerate() {
                for rep in 0..3 {
                    let crashed = ci == 1 && hi == 1 && mi == 1 && rep == 2;
                    let throttled = ci == 2 && hi == 0 && rep == 1;
                    let host = match lab {
                        Some((bsp, sumd)) => AndroidDeviceState::internal(
                            AndroidLabConfig {
                                bsp_version: bsp.to_string(),
                                sumd_driver_version: sumd.to_string(),
                                gpu_clock_mhz: 980,
                                mif_clock_mhz: 5333,
                                int_clock_mhz: 934,
                            },
                            3600 * (hi as i64 + 1) + rep * 300,
                            hi == 0,
                            31.0 + rep as f64,
                            38.0 + rep as f64 * 2.0 + if throttled { 9.0 } else { 0.0 },
                            throttled,
                        ),
                        None => AndroidDeviceState {
                            os: Some("Android 16 (BP4A.251205.006)".into()),
                            kernel: Some("6.1.157-android14-11".into()),
                            soc: Some("s5e9945".into()),
                            cpu_count: Some(10),
                            memory_bytes: Some(11_472_032 * 1024),
                            gpu: Some("Samsung Xclipse 940".into()),
                            gpu_driver: Some("Driver version: 24.0.534".into()),
                            ..Default::default()
                        },
                    };
                    let run = NewRun {
                        id: Uuid::now_v7(),
                        started_at: commit_time + Duration::hours(2 + hi as i64) + Duration::minutes(rep * 5 + mi as i64 * 20),
                        finished_at: Some(commit_time + Duration::hours(2 + hi as i64) + Duration::minutes(rep * 5 + mi as i64 * 20 + 2)),
                        repetition: rep,
                        command_args_json: r#"["--model_path","/data/local/tmp/model.pte","--prompt_file","/data/local/tmp/prompt.txt","--max_new_tokens","64"]"#.into(),
                        command_line: Some("llama_main --model_path /data/local/tmp/model.pte --prompt_file /data/local/tmp/prompt.txt --max_new_tokens 64".into()),
                        input_parameters_json: r#"{"backend":"vulkan","seq_len":64}"#.into(),
                        env_vars_json: r#"{"ET_LOG_LEVEL":"info"}"#.into(),
                        env_allowlist_version: "v1".into(),
                        collector_version: COLLECTOR.into(),
                        device_class: if lab.is_some() { DeviceClass::Internal } else { DeviceClass::External },
                        device_serial: serial.to_string(),
                        device_model: Some(model_name.to_string()),
                        host: HostState::Android(host),
                        git_commit_sha: sha.to_string(),
                        git_dirty: ci == 2,
                        git_branch: Some("main".into()),
                        git_commit_timestamp: Some(commit_time),
                        git_commit_subject: Some(subject.to_string()),
                        executable_sha256: Some(Sha256Hex::try_from(format!("{:0>64}", ci + 1)).unwrap()),
                        model_asset_id: *model_id,
                        prompt_sha256: prompt_sha256.clone(),
                        input_token_count: 14,
                        output_token_count: if crashed { 0 } else { 64 },
                        prefill_tokens_per_sec: if crashed { 0.0 } else { jitter(base_prefill * if throttled { 0.7 } else if lab.is_none() { 0.35 } else { 1.0 }) },
                        decode_tokens_per_sec: if crashed { None } else { Some(jitter(base_decode)) },
                        exit_status: if crashed { ExitStatus::Crashed } else { ExitStatus::Succeeded },
                        correctness_result: if crashed { CorrectnessResult::NotChecked } else { CorrectnessResult::Passed },
                        input_artifact_id: Some(prompt_artifact.id),
                        output_artifact_id: None,
                        output_preview: if crashed { None } else { Some("Prefill processes the whole prompt in one batched pass; decode then emits one token at a time...".into()) },
                        stdout_artifact_id: None,
                        stderr_artifact_id: None,
                        crash_artifact_id: None,
                        error_summary: if crashed { Some("SIGSEGV in vulkan command buffer submit".into()) } else { None },
                    };
                    insert_run(&pool, &run).await.expect("insert android run");
                    count += 1;
                }
            }
            for (hi, (hostname, os, kernel, cpu, cpus, accel, driver)) in linux_hosts.iter().enumerate() {
                for rep in 0..3 {
                    let scale = if hi == 0 { 6.0 } else { 1.3 };
                    let run = NewRun {
                        id: Uuid::now_v7(),
                        started_at: commit_time + Duration::hours(5 + hi as i64) + Duration::minutes(rep * 3 + mi as i64 * 15),
                        finished_at: Some(commit_time + Duration::hours(5 + hi as i64) + Duration::minutes(rep * 3 + mi as i64 * 15 + 1)),
                        repetition: rep,
                        command_args_json: r#"["--model_path=/mnt/models/model.pte","--tokenizer_path=/mnt/models/tokenizer.model","--prompt_file=/tmp/prompt.txt","--max_new_tokens=1"]"#.into(),
                        command_line: Some("llama_main --model_path=/mnt/models/model.pte --tokenizer_path=/mnt/models/tokenizer.model --prompt_file=/tmp/prompt.txt --max_new_tokens=1".into()),
                        input_parameters_json: r#"{"backend":"vulkan","benchmark":"prefill-2048"}"#.into(),
                        env_vars_json: "{}".into(),
                        env_allowlist_version: "none".into(),
                        collector_version: COLLECTOR.into(),
                        device_class: DeviceClass::External,
                        device_serial: hostname.to_string(),
                        device_model: None,
                        host: HostState::Linux(LinuxHostState {
                            os: os.to_string(),
                            kernel: kernel.to_string(),
                            cpu_model: cpu.to_string(),
                            cpu_count: Some(*cpus),
                            memory_bytes: Some(64 * 1024 * 1024 * 1024),
                            accelerator: accel.to_string(),
                            accelerator_driver: Some(driver.to_string()),
                            uptime_seconds: None,
                            thermal_throttling: None,
                        }),
                        git_commit_sha: sha.to_string(),
                        git_dirty: ci == 2,
                        git_branch: Some("main".into()),
                        git_commit_timestamp: Some(commit_time),
                        git_commit_subject: Some(subject.to_string()),
                        executable_sha256: if hi == 0 { None } else { Some(Sha256Hex::try_from(format!("{:0>64}", ci + 10)).unwrap()) },
                        model_asset_id: *model_id,
                        prompt_sha256: prompt_sha256.clone(),
                        input_token_count: 2048,
                        output_token_count: 0,
                        prefill_tokens_per_sec: jitter(base_prefill * scale),
                        decode_tokens_per_sec: None,
                        exit_status: ExitStatus::Succeeded,
                        correctness_result: CorrectnessResult::NotChecked,
                        input_artifact_id: Some(prompt_artifact.id),
                        output_artifact_id: None,
                        output_preview: None,
                        stdout_artifact_id: None,
                        stderr_artifact_id: None,
                        crash_artifact_id: None,
                        error_summary: None,
                    };
                    insert_run(&pool, &run).await.expect("insert linux run");
                    count += 1;
                }
            }
        }
    }
    println!("seeded {count} mock runs across {} commits into {}", commits.len(), config.database_url);
}
