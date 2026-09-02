//! `GET /api/v1/events`: the live change stream as Server-Sent Events. See
//! `specs/ingestion-service` - "Service streams change notifications as
//! Server-Sent Events".

use crate::events::Published;
use crate::http::AppState;
use axum::extract::State;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures_util::stream::{Stream, StreamExt};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(stream_events))
}

fn to_sse(published: Published) -> SseEvent {
    SseEvent::default()
        .id(published.seq.to_string())
        .event(published.event.name())
        .data(published.event.data().to_string())
}

/// Stream change notifications as Server-Sent Events. Each event has an
/// `event` name, a process-scoped monotonically increasing `id`, and a
/// JSON `data` payload:
///
/// - `run.created` - a run was recorded; `data` is a `RunCreatedEvent`.
/// - `artifact.created` - an artifact was stored; `data` is an
///   `ArtifactCreatedEvent`.
/// - `model.registered` - a model asset was registered; `data` is a
///   `ModelRegisteredEvent`.
///
/// A comment keep-alive is sent while idle (every 15 seconds by default).
/// Events are signals to re-fetch, not state: nothing is replayed after a
/// reconnect, and the REST operations remain authoritative. A subscriber
/// that falls too far behind is disconnected and should reconnect and
/// re-fetch.
#[utoipa::path(
    get,
    path = "/api/v1/events",
    operation_id = "streamEvents",
    tag = "events",
    responses(
        (
            status = 200,
            description = "An open `text/event-stream`. Event names: `run.created` (data: `RunCreatedEvent`), `artifact.created` (data: `ArtifactCreatedEvent`), `model.registered` (data: `ModelRegisteredEvent`). Keep-alive comments are sent while idle; missed events are not replayed.",
            content_type = "text/event-stream",
            body = String,
        ),
    )
)]
async fn stream_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let receiver = state.events.subscribe();
    // A lagging receiver yields `Lagged` once; ending the stream there makes
    // the client reconnect and re-fetch rather than continue with a gap it
    // cannot see.
    let stream = BroadcastStream::new(receiver)
        .take_while(|item| {
            std::future::ready(!matches!(item, Err(BroadcastStreamRecvError::Lagged(_))))
        })
        .filter_map(|item| std::future::ready(item.ok()))
        .map(|published| Ok(to_sse(published)));
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(state.config.events_keep_alive_seconds.max(1)))
            .text("keep-alive"),
    )
}
