//! Change notifications published by the write paths and streamed to
//! subscribers as Server-Sent Events. See `specs/ingestion-service` -
//! "Service streams change notifications as Server-Sent Events".
//!
//! Events are signals, not state: a subscriber that receives one re-fetches
//! the REST operations. Nothing is persisted or replayed; a process-scoped
//! sequence number is attached at publish time so every subscriber sees the
//! same `id` for the same event.

use crate::artifact_store::ArtifactKind;
use crate::domain::{CorrectnessResult, DeviceClass, ExitStatus, Platform};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;
use utoipa::ToSchema;
use uuid::Uuid;

/// `data` payload of a `run.created` event: the run's listing summary.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RunCreatedEvent {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub repetition: i64,
    pub platform: Platform,
    pub device_class: DeviceClass,
    pub device_serial: String,
    pub device_model: Option<String>,
    pub model_asset_id: Uuid,
    pub model_original_name: String,
    pub git_commit_sha: String,
    pub git_dirty: bool,
    pub git_branch: Option<String>,
    pub exit_status: ExitStatus,
    pub correctness_result: CorrectnessResult,
    /// Prefill throughput in tokens per second.
    pub prefill_tokens_per_sec: f64,
    /// Decode throughput in tokens per second; null when not recorded.
    pub decode_tokens_per_sec: Option<f64>,
}

/// `data` payload of an `artifact.created` event.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ArtifactCreatedEvent {
    pub id: Uuid,
    pub kind: ArtifactKind,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the content.
    pub sha256: String,
    pub size_bytes: i64,
}

/// `data` payload of a `model.registered` event.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ModelRegisteredEvent {
    pub id: Uuid,
    pub original_name: String,
    /// Lowercase 64-character hexadecimal SHA-256 digest of the model file.
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub enum Event {
    RunCreated(RunCreatedEvent),
    ArtifactCreated(ArtifactCreatedEvent),
    ModelRegistered(ModelRegisteredEvent),
}

impl Event {
    /// The SSE `event:` name.
    pub fn name(&self) -> &'static str {
        match self {
            Event::RunCreated(_) => "run.created",
            Event::ArtifactCreated(_) => "artifact.created",
            Event::ModelRegistered(_) => "model.registered",
        }
    }

    /// The SSE `data:` payload.
    pub fn data(&self) -> serde_json::Value {
        match self {
            Event::RunCreated(e) => serde_json::to_value(e),
            Event::ArtifactCreated(e) => serde_json::to_value(e),
            Event::ModelRegistered(e) => serde_json::to_value(e),
        }
        .expect("event payloads always serialize")
    }
}

/// An event with the sequence number it was published under.
#[derive(Debug, Clone)]
pub struct Published {
    pub seq: u64,
    pub event: Event,
}

/// The in-process fan-out every write path publishes to. Cheap to clone;
/// all clones share one channel and one sequence.
#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<Published>,
    seq: Arc<AtomicU64>,
}

impl EventBus {
    /// `capacity` bounds how far a slow subscriber may fall behind before
    /// it is dropped and must reconnect.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        EventBus {
            sender,
            seq: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Publishes to every current subscriber. With no subscribers the
    /// event is simply dropped: nothing is persisted.
    pub fn publish(&self, event: Event) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.sender.send(Published { seq, event });
        seq
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Published> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        EventBus::new(256)
    }
}
