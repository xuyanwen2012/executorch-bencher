//! Grouped benchmark results: one row per configuration key with
//! median/min/max/n throughput over succeeded runs. See
//! `specs/ingestion-service` - "Service exposes grouped benchmark results".
//!
//! Grouping and statistics are computed in Rust from a single filtered
//! `SELECT` of the key, metric, and flag columns: SQLite has no median, and
//! the fold is simpler to test and fast for tens of thousands of runs.

use crate::domain::{CorrectnessResult, DeviceClass, ExitStatus, Platform, Sha256Hex};
use crate::runs::{RunListFilter, RunsError};
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Upper bound on rows returned by [`results`] through the HTTP API.
pub const MAX_ROWS: usize = 500;

/// Exact-match filters for [`results`]. Outcome fields are deliberately
/// absent: every run of a configuration contributes to its row (as a
/// statistic or a count), so filtering on outcome would misreport counts.
#[derive(Debug, Clone, Default)]
pub struct ResultsFilter {
    pub platform: Option<Platform>,
    pub device_class: Option<DeviceClass>,
    pub device_serial: Option<String>,
    pub model_asset_id: Option<Uuid>,
    pub git_commit_sha: Option<String>,
    pub git_branch: Option<String>,
    pub git_dirty: Option<bool>,
    pub sumd_driver_version: Option<String>,
    pub bsp_version: Option<String>,
    pub host_accelerator: Option<String>,
    pub prompt_sha256: Option<Sha256Hex>,
}

impl From<&ResultsFilter> for RunListFilter {
    fn from(f: &ResultsFilter) -> Self {
        RunListFilter {
            platform: f.platform,
            device_class: f.device_class,
            device_serial: f.device_serial.clone(),
            model_asset_id: f.model_asset_id,
            git_commit_sha: f.git_commit_sha.clone(),
            git_branch: f.git_branch.clone(),
            git_dirty: f.git_dirty,
            sumd_driver_version: f.sumd_driver_version.clone(),
            bsp_version: f.bsp_version.clone(),
            host_accelerator: f.host_accelerator.clone(),
            prompt_sha256: f.prompt_sha256.clone(),
            ..RunListFilter::default()
        }
    }
}

/// The benchmark configuration key: every dimension that changes measured
/// performance. Dirty and clean runs of one commit are distinct keys. The
/// platform-specific dimensions are present only for their platform:
/// SUMD driver, BSP, and the pinned clocks on Android; the accelerator on
/// Linux.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConfigKey {
    pub platform: Platform,
    pub device_class: DeviceClass,
    pub device_serial: String,
    pub model_asset_id: Uuid,
    pub git_commit_sha: String,
    pub git_dirty: bool,
    pub sumd_driver_version: Option<String>,
    pub bsp_version: Option<String>,
    pub gpu_clock_mhz: Option<i64>,
    pub mif_clock_mhz: Option<i64>,
    pub int_clock_mhz: Option<i64>,
    pub host_accelerator: Option<String>,
    pub prompt_sha256: String,
}

/// Summary statistics over one metric's succeeded-run samples.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricStats {
    pub median: f64,
    pub min: f64,
    pub max: f64,
    pub n: u64,
}

impl MetricStats {
    fn from_samples(mut samples: Vec<f64>) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        samples.sort_by(f64::total_cmp);
        let n = samples.len();
        let median = if n % 2 == 1 {
            samples[n / 2]
        } else {
            (samples[n / 2 - 1] + samples[n / 2]) / 2.0
        };
        Some(MetricStats {
            median,
            min: samples[0],
            max: samples[n - 1],
            n: n as u64,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResultRow {
    pub key: ConfigKey,
    pub device_model: Option<String>,
    pub model_original_name: String,
    pub git_branch: Option<String>,
    pub git_commit_timestamp: Option<DateTime<Utc>>,
    pub git_commit_subject: Option<String>,
    pub input_token_count: i64,
    /// `None` when no run of this configuration succeeded.
    pub prefill: Option<MetricStats>,
    /// `None` when no succeeded run recorded a decode throughput.
    pub decode: Option<MetricStats>,
    pub total_runs: u64,
    pub not_succeeded: u64,
    /// Succeeded runs whose correctness result is `failed`.
    pub correctness_failed: u64,
    pub throttled: u64,
    pub first_started_at: DateTime<Utc>,
    pub last_started_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ResultsPage {
    pub rows: Vec<ResultRow>,
    /// True when more configurations matched than `max_rows` allowed.
    pub truncated: bool,
}

#[derive(Debug, Default)]
struct Accumulator {
    device_model: Option<String>,
    model_original_name: String,
    git_branch: Option<String>,
    git_commit_timestamp: Option<DateTime<Utc>>,
    git_commit_subject: Option<String>,
    input_token_count: i64,
    prefill: Vec<f64>,
    decode: Vec<f64>,
    total_runs: u64,
    not_succeeded: u64,
    correctness_failed: u64,
    throttled: u64,
    first_started_at: Option<DateTime<Utc>>,
    last_started_at: Option<DateTime<Utc>>,
}

/// Groups runs matching `filter` by configuration key and computes each
/// group's statistics. Rows are ordered by commit timestamp (falling back
/// to the configuration's earliest run start) descending, then model name,
/// then device serial, then the remaining key fields; at most `max_rows`
/// are returned.
pub async fn results(
    pool: &SqlitePool,
    filter: &ResultsFilter,
    max_rows: usize,
) -> Result<ResultsPage, RunsError> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT r.platform, r.device_class, r.device_serial, r.device_model, r.model_asset_id,
                r.git_commit_sha, r.git_dirty,
                r.sumd_driver_version, r.bsp_version, r.gpu_clock_mhz, r.mif_clock_mhz,
                r.int_clock_mhz, r.host_accelerator, r.prompt_sha256, r.git_branch,
                r.git_commit_timestamp,
                r.git_commit_subject, r.input_token_count, r.started_at,
                r.exit_status, r.correctness_result, r.thermal_throttling,
                r.prefill_tokens_per_sec, r.decode_tokens_per_sec,
                m.original_name AS model_original_name
         FROM runs r
         LEFT JOIN model_assets m ON m.id = r.model_asset_id
         WHERE 1 = 1",
    );
    RunListFilter::from(filter).push_where_clauses(&mut qb);
    qb.push(" ORDER BY r.started_at ASC, r.id ASC");
    // Stream the rows instead of loading every matching run first, so
    // memory is bounded by the number of configurations, not of runs.
    let query = qb.build();
    let mut rows = query.fetch(pool);
    let mut groups: BTreeMap<ConfigKey, Accumulator> = BTreeMap::new();
    while let Some(row) = rows.try_next().await? {
        let model_asset_id: String = row.try_get("model_asset_id")?;
        let platform: String = row.try_get("platform")?;
        let device_class: String = row.try_get("device_class")?;
        let key = ConfigKey {
            platform: Platform::try_from(platform.as_str())
                .map_err(|err| RunsError(format!("invalid stored platform: {err}")))?,
            device_class: DeviceClass::try_from(device_class.as_str())
                .map_err(|err| RunsError(format!("invalid stored device_class: {err}")))?,
            device_serial: row.try_get("device_serial")?,
            model_asset_id: Uuid::parse_str(&model_asset_id)
                .map_err(|err| RunsError(format!("invalid stored model_asset_id: {err}")))?,
            git_commit_sha: row.try_get("git_commit_sha")?,
            git_dirty: row.try_get::<i64, _>("git_dirty")? != 0,
            sumd_driver_version: row.try_get("sumd_driver_version")?,
            bsp_version: row.try_get("bsp_version")?,
            gpu_clock_mhz: row.try_get("gpu_clock_mhz")?,
            mif_clock_mhz: row.try_get("mif_clock_mhz")?,
            int_clock_mhz: row.try_get("int_clock_mhz")?,
            host_accelerator: row.try_get("host_accelerator")?,
            prompt_sha256: row.try_get("prompt_sha256")?,
        };
        let started_at: DateTime<Utc> = row.try_get("started_at")?;
        let exit_status: String = row.try_get("exit_status")?;
        let exit_status = ExitStatus::try_from(exit_status.as_str())
            .map_err(|err| RunsError(format!("invalid stored exit_status: {err}")))?;
        let correctness: String = row.try_get("correctness_result")?;
        let correctness = CorrectnessResult::try_from(correctness.as_str())
            .map_err(|err| RunsError(format!("invalid stored correctness_result: {err}")))?;
        let throttled = row.try_get::<Option<i64>, _>("thermal_throttling")? == Some(1);
        let prefill: f64 = row.try_get("prefill_tokens_per_sec")?;
        let decode: Option<f64> = row.try_get("decode_tokens_per_sec")?;

        let acc = groups.entry(key).or_default();
        if acc.device_model.is_none() {
            acc.device_model = row.try_get("device_model")?;
        }
        if acc.total_runs == 0 {
            acc.model_original_name = row
                .try_get::<Option<String>, _>("model_original_name")?
                .unwrap_or_default();
            acc.input_token_count = row.try_get("input_token_count")?;
        }
        if acc.git_branch.is_none() {
            acc.git_branch = row.try_get("git_branch")?;
        }
        if acc.git_commit_timestamp.is_none() {
            acc.git_commit_timestamp = row.try_get("git_commit_timestamp")?;
        }
        if acc.git_commit_subject.is_none() {
            acc.git_commit_subject = row.try_get("git_commit_subject")?;
        }
        acc.total_runs += 1;
        if throttled {
            acc.throttled += 1;
        }
        // Rows arrive oldest first, so first/last fall out of the order.
        acc.first_started_at.get_or_insert(started_at);
        acc.last_started_at = Some(started_at);
        if exit_status == ExitStatus::Succeeded {
            acc.prefill.push(prefill);
            if let Some(decode) = decode {
                acc.decode.push(decode);
            }
            if correctness == CorrectnessResult::Failed {
                acc.correctness_failed += 1;
            }
        } else {
            acc.not_succeeded += 1;
        }
    }

    let mut rows: Vec<ResultRow> = groups
        .into_iter()
        .filter_map(|(key, acc)| {
            // A group only exists because a row was accumulated into it,
            // so `first_started_at` is always set; never panic regardless.
            let first = acc.first_started_at?;
            Some(ResultRow {
                key,
                device_model: acc.device_model,
                model_original_name: acc.model_original_name,
                git_branch: acc.git_branch,
                git_commit_timestamp: acc.git_commit_timestamp,
                git_commit_subject: acc.git_commit_subject,
                input_token_count: acc.input_token_count,
                prefill: MetricStats::from_samples(acc.prefill),
                decode: MetricStats::from_samples(acc.decode),
                total_runs: acc.total_runs,
                not_succeeded: acc.not_succeeded,
                correctness_failed: acc.correctness_failed,
                throttled: acc.throttled,
                first_started_at: first,
                last_started_at: acc.last_started_at.unwrap_or(first),
            })
        })
        .collect();

    rows.sort_by_key(|row| {
        (
            Reverse(row.git_commit_timestamp.unwrap_or(row.first_started_at)),
            row.model_original_name.clone(),
            row.key.device_serial.clone(),
            row.key.clone(),
        )
    });

    let truncated = rows.len() > max_rows;
    rows.truncate(max_rows);
    Ok(ResultsPage { rows, truncated })
}

/// Distinct filter values across *all* runs, ignoring any active filter,
/// so a client can always widen a filter it has narrowed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facets {
    pub platforms: Vec<Platform>,
    pub device_classes: Vec<DeviceClass>,
    pub device_serials: Vec<String>,
    /// `(id, original_name)` of every model asset referenced by a run.
    pub models: Vec<(Uuid, String)>,
    pub git_branches: Vec<String>,
    pub sumd_driver_versions: Vec<String>,
    pub bsp_versions: Vec<String>,
    pub host_accelerators: Vec<String>,
}

pub async fn facets(pool: &SqlitePool) -> Result<Facets, RunsError> {
    let model_rows = sqlx::query(
        "SELECT DISTINCT m.id, m.original_name
         FROM runs r JOIN model_assets m ON m.id = r.model_asset_id
         ORDER BY m.original_name, m.id",
    )
    .fetch_all(pool)
    .await?;
    let models = model_rows
        .into_iter()
        .map(|row| {
            let id: String = row.try_get("id")?;
            let name: String = row.try_get("original_name")?;
            Uuid::parse_str(&id)
                .map(|id| (id, name))
                .map_err(|err| RunsError(format!("invalid stored model id: {err}")))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // One grouped scan yields every distinct value of every facet column at
    // once, instead of one full-table scan per facet.
    let rows = sqlx::query(
        "SELECT platform, device_class, device_serial, git_branch,
                sumd_driver_version, bsp_version, host_accelerator
         FROM runs
         GROUP BY platform, device_class, device_serial, git_branch,
                  sumd_driver_version, bsp_version, host_accelerator",
    )
    .fetch_all(pool)
    .await?;

    let mut platforms = BTreeSet::new();
    let mut device_classes = BTreeSet::new();
    let mut device_serials = BTreeSet::new();
    let mut git_branches = BTreeSet::new();
    let mut sumd_driver_versions = BTreeSet::new();
    let mut bsp_versions = BTreeSet::new();
    let mut host_accelerators = BTreeSet::new();
    for row in rows {
        platforms.insert(row.try_get::<String, _>("platform")?);
        device_classes.insert(row.try_get::<String, _>("device_class")?);
        device_serials.insert(row.try_get::<String, _>("device_serial")?);
        for (column, set) in [
            ("git_branch", &mut git_branches),
            ("sumd_driver_version", &mut sumd_driver_versions),
            ("bsp_version", &mut bsp_versions),
            ("host_accelerator", &mut host_accelerators),
        ] {
            if let Some(value) = row.try_get::<Option<String>, _>(column)? {
                set.insert(value);
            }
        }
    }

    let platforms = platforms
        .iter()
        .map(|p| {
            Platform::try_from(p.as_str())
                .map_err(|err| RunsError(format!("invalid stored platform: {err}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let device_classes = device_classes
        .iter()
        .map(|c| {
            DeviceClass::try_from(c.as_str())
                .map_err(|err| RunsError(format!("invalid stored device_class: {err}")))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Facets {
        platforms,
        device_classes,
        device_serials: device_serials.into_iter().collect(),
        models,
        git_branches: git_branches.into_iter().collect(),
        sumd_driver_versions: sumd_driver_versions.into_iter().collect(),
        bsp_versions: bsp_versions.into_iter().collect(),
        host_accelerators: host_accelerators.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::model_registry::{ExternalModelStorage, ModelStorage};
    use crate::runs::{NewRun, insert_run, test_support::minimal_new_run};

    struct Ctx {
        pool: SqlitePool,
        _dir: tempfile::TempDir,
    }

    async fn ctx() -> Ctx {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", dir.path().join("b.sqlite3").display());
        let pool = db::connect_and_migrate(&url).await.unwrap();
        Ctx { pool, _dir: dir }
    }

    async fn model(ctx: &Ctx, name: &str) -> Uuid {
        let path = ctx._dir.path().join(name);
        std::fs::write(&path, name.as_bytes()).unwrap();
        ExternalModelStorage
            .register(&ctx.pool, &path)
            .await
            .unwrap()
            .id
    }

    fn android_mut(run: &mut NewRun) -> &mut crate::runs::AndroidDeviceState {
        match &mut run.host {
            crate::runs::HostState::Android(a) => a,
            crate::runs::HostState::Linux(_) => panic!("expected an android run"),
        }
    }

    fn lab_mut(run: &mut NewRun) -> &mut crate::runs::AndroidLabConfig {
        android_mut(run).lab.as_mut().expect("a lab android run")
    }

    fn linux_run(model_asset_id: Uuid, host: &str, accelerator: &str, prefill: f64) -> NewRun {
        let mut r = minimal_new_run(Uuid::now_v7(), model_asset_id);
        r.device_serial = host.to_string();
        r.device_class = DeviceClass::External;
        r.host = crate::runs::HostState::Linux(crate::runs::LinuxHostState {
            os: "Ubuntu 24.04.4 LTS".into(),
            kernel: "6.8.0".into(),
            cpu_model: "AMD EPYC 4464P".into(),
            cpu_count: Some(16),
            memory_bytes: None,
            accelerator: accelerator.to_string(),
            accelerator_driver: Some("Mesa 25.2.8".into()),
            uptime_seconds: None,
            thermal_throttling: None,
        });
        r.prefill_tokens_per_sec = prefill;
        r
    }

    fn run(model_asset_id: Uuid, prefill: f64, minute: i64) -> NewRun {
        let mut r = minimal_new_run(Uuid::now_v7(), model_asset_id);
        r.prefill_tokens_per_sec = prefill;
        r.started_at += chrono::Duration::minutes(minute);
        r
    }

    #[tokio::test]
    async fn repetitions_collapse_into_one_row_with_a_median() {
        let c = ctx().await;
        let m = model(&c, "llama.pte").await;
        for (i, p) in [100.0, 110.0, 120.0, 130.0, 900.0].into_iter().enumerate() {
            let mut r = run(m, p, i as i64);
            r.decode_tokens_per_sec = if i == 0 { None } else { Some(10.0 * i as f64) };
            insert_run(&c.pool, &r).await.unwrap();
        }
        let page = results(&c.pool, &ResultsFilter::default(), MAX_ROWS)
            .await
            .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert!(!page.truncated);
        let row = &page.rows[0];
        assert_eq!(
            row.prefill,
            Some(MetricStats {
                median: 120.0,
                min: 100.0,
                max: 900.0,
                n: 5
            })
        );
        // Decode: 10,20,30,40 over the four runs that recorded it.
        assert_eq!(
            row.decode,
            Some(MetricStats {
                median: 25.0,
                min: 10.0,
                max: 40.0,
                n: 4
            })
        );
        assert_eq!(row.total_runs, 5);
        assert_eq!(row.model_original_name, "llama.pte");
        assert_eq!(row.input_token_count, 10);
        assert!(row.first_started_at < row.last_started_at);
    }

    #[tokio::test]
    async fn only_succeeded_runs_contribute_to_statistics() {
        let c = ctx().await;
        let m = model(&c, "m.pte").await;
        for p in [100.0, 200.0, 300.0] {
            insert_run(&c.pool, &run(m, p, 0)).await.unwrap();
        }
        for _ in 0..2 {
            let mut r = run(m, 999.0, 0);
            r.exit_status = ExitStatus::Crashed;
            android_mut(&mut r).thermal_throttling = Some(true);
            insert_run(&c.pool, &r).await.unwrap();
        }
        let mut r = run(m, 250.0, 0);
        r.correctness_result = CorrectnessResult::Failed;
        insert_run(&c.pool, &r).await.unwrap();

        let page = results(&c.pool, &ResultsFilter::default(), MAX_ROWS)
            .await
            .unwrap();
        let row = &page.rows[0];
        assert_eq!(row.prefill.as_ref().unwrap().n, 4);
        assert_eq!(row.prefill.as_ref().unwrap().max, 300.0);
        assert_eq!(row.total_runs, 6);
        assert_eq!(row.not_succeeded, 2);
        assert_eq!(row.correctness_failed, 1);
        assert_eq!(row.throttled, 2);
    }

    #[tokio::test]
    async fn a_configuration_with_no_succeeded_runs_has_null_statistics() {
        let c = ctx().await;
        let m = model(&c, "m.pte").await;
        let mut r = run(m, 1.0, 0);
        r.exit_status = ExitStatus::TimedOut;
        insert_run(&c.pool, &r).await.unwrap();
        let page = results(&c.pool, &ResultsFilter::default(), MAX_ROWS)
            .await
            .unwrap();
        let row = &page.rows[0];
        assert!(row.prefill.is_none());
        assert!(row.decode.is_none());
        assert_eq!(row.not_succeeded, 1);
        assert_eq!(row.total_runs, 1);
    }

    #[tokio::test]
    async fn dirty_and_clean_runs_of_one_commit_are_separate_rows() {
        let c = ctx().await;
        let m = model(&c, "m.pte").await;
        insert_run(&c.pool, &run(m, 100.0, 0)).await.unwrap();
        let mut dirty = run(m, 100.0, 1);
        dirty.git_dirty = true;
        insert_run(&c.pool, &dirty).await.unwrap();
        let page = results(&c.pool, &ResultsFilter::default(), MAX_ROWS)
            .await
            .unwrap();
        assert_eq!(page.rows.len(), 2);
        let dirty_flags: Vec<bool> = page.rows.iter().map(|r| r.key.git_dirty).collect();
        assert!(dirty_flags.contains(&true) && dirty_flags.contains(&false));
        assert_eq!(page.rows[0].key.git_commit_sha, page.rows[1].key.git_commit_sha);
    }

    #[tokio::test]
    async fn rows_order_by_commit_timestamp_with_first_run_fallback_and_filters_apply() {
        let c = ctx().await;
        let m = model(&c, "m.pte").await;
        let ts = |s: &str| {
            DateTime::parse_from_rfc3339(s)
                .unwrap()
                .with_timezone(&Utc)
        };
        // Commit A: timestamp 2026-08-01, run at base+0.
        let mut a = run(m, 1.0, 0);
        a.git_commit_sha = "aaaa".into();
        a.git_commit_timestamp = Some(ts("2026-08-01T00:00:00Z"));
        a.git_branch = Some("main".into());
        insert_run(&c.pool, &a).await.unwrap();
        // Commit B: timestamp 2026-08-20, run at base+1.
        let mut b = run(m, 1.0, 1);
        b.git_commit_sha = "bbbb".into();
        b.git_commit_timestamp = Some(ts("2026-08-20T00:00:00Z"));
        insert_run(&c.pool, &b).await.unwrap();
        // Commit C: no timestamp; first run at base+2 (2026-09-01) - newest.
        let mut cc = run(m, 1.0, 2);
        cc.git_commit_sha = "cccc".into();
        cc.device_serial = "other-device".into();
        insert_run(&c.pool, &cc).await.unwrap();

        let page = results(&c.pool, &ResultsFilter::default(), MAX_ROWS)
            .await
            .unwrap();
        let order: Vec<&str> = page.rows.iter().map(|r| r.key.git_commit_sha.as_str()).collect();
        assert_eq!(order, vec!["cccc", "bbbb", "aaaa"]);
        assert_eq!(page.rows[2].git_branch.as_deref(), Some("main"));

        let filtered = results(
            &c.pool,
            &ResultsFilter {
                device_serial: Some("device-001".into()),
                ..Default::default()
            },
            MAX_ROWS,
        )
        .await
        .unwrap();
        assert_eq!(filtered.rows.len(), 2);

        let capped = results(&c.pool, &ResultsFilter::default(), 2)
            .await
            .unwrap();
        assert_eq!(capped.rows.len(), 2);
        assert!(capped.truncated);
    }

    #[tokio::test]
    async fn linux_hosts_group_by_hostname_and_accelerator_and_facet_by_platform() {
        let c = ctx().await;
        let m = model(&c, "m.pte").await;
        insert_run(&c.pool, &run(m, 100.0, 0)).await.unwrap();
        for p in [900.0, 1000.0, 1100.0] {
            insert_run(&c.pool, &linux_run(m, "box-a", "Intel Arc B580", p))
                .await
                .unwrap();
        }
        insert_run(&c.pool, &linux_run(m, "box-a", "llvmpipe", 5.0))
            .await
            .unwrap();
        insert_run(&c.pool, &linux_run(m, "box-b", "Intel Arc B580", 50.0))
            .await
            .unwrap();

        let page = results(&c.pool, &ResultsFilter::default(), MAX_ROWS)
            .await
            .unwrap();
        assert_eq!(page.rows.len(), 4);
        let arc_a = page
            .rows
            .iter()
            .find(|r| {
                r.key.platform == Platform::Linux
                    && r.key.device_serial == "box-a"
                    && r.key.host_accelerator.as_deref() == Some("Intel Arc B580")
            })
            .expect("box-a / Arc row");
        assert_eq!(arc_a.prefill.as_ref().unwrap().median, 1000.0);
        assert_eq!(arc_a.key.sumd_driver_version, None);
        assert_eq!(arc_a.key.gpu_clock_mhz, None);
        assert_eq!(arc_a.throttled, 0);
        let android = page
            .rows
            .iter()
            .find(|r| r.key.platform == Platform::Android)
            .expect("android row");
        assert_eq!(android.key.host_accelerator, None);
        assert_eq!(android.key.bsp_version.as_deref(), Some("bsp-1.0"));

        let linux_only = results(
            &c.pool,
            &ResultsFilter {
                platform: Some(Platform::Linux),
                host_accelerator: Some("Intel Arc B580".into()),
                ..Default::default()
            },
            MAX_ROWS,
        )
        .await
        .unwrap();
        assert_eq!(linux_only.rows.len(), 2);

        let f = facets(&c.pool).await.unwrap();
        assert_eq!(f.platforms, vec![Platform::Android, Platform::Linux]);
        assert_eq!(f.device_classes, vec![DeviceClass::External, DeviceClass::Internal]);
        assert_eq!(f.host_accelerators, vec!["Intel Arc B580", "llvmpipe"]);
        assert_eq!(f.bsp_versions, vec!["bsp-1.0"]);
    }

    #[tokio::test]
    async fn facets_ignore_filters_and_list_distinct_values() {
        let c = ctx().await;
        let m1 = model(&c, "a.pte").await;
        let m2 = model(&c, "b.pte").await;
        let mut r1 = run(m1, 1.0, 0);
        r1.device_serial = "dev-1".into();
        r1.git_branch = Some("main".into());
        lab_mut(&mut r1).sumd_driver_version = "sumd-1".into();
        insert_run(&c.pool, &r1).await.unwrap();
        let mut r2 = run(m2, 1.0, 1);
        r2.device_serial = "dev-2".into();
        lab_mut(&mut r2).bsp_version = "bsp-2".into();
        insert_run(&c.pool, &r2).await.unwrap();

        let f = facets(&c.pool).await.unwrap();
        assert_eq!(f.device_serials, vec!["dev-1", "dev-2"]);
        assert_eq!(
            f.models.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>(),
            vec!["a.pte", "b.pte"]
        );
        assert_eq!(f.git_branches, vec!["main"]);
        assert_eq!(f.sumd_driver_versions, vec!["sumd-1", "sumd-1.0"]);
        assert_eq!(f.bsp_versions, vec!["bsp-1.0", "bsp-2"]);
        // Facets come from all runs; a filtered results query does not
        // change them (there is no filter parameter to pass).
        let filtered = results(
            &c.pool,
            &ResultsFilter {
                device_serial: Some("dev-1".into()),
                ..Default::default()
            },
            MAX_ROWS,
        )
        .await
        .unwrap();
        assert_eq!(filtered.rows.len(), 1);
        assert_eq!(facets(&c.pool).await.unwrap(), f);
    }
}
