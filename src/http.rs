use crate::config::Config;
use crate::events::EventBus;
use axum::{Router, extract::State, http::StatusCode};
use sqlx::SqlitePool;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_swagger_ui::SwaggerUi;
use tower_http::services::{ServeDir, ServeFile};

/// Shared state for every HTTP handler: the database pool and the storage
/// configuration (roots, limits) artifact/model routes need.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Config,
    /// Fan-out for change notifications; write handlers publish here after
    /// their database write succeeds.
    pub events: EventBus,
}

/// Builds the full route table plus the OpenAPI document describing it.
/// Shared by [`router`] (which also needs live state to serve requests) and
/// the `gen-openapi` binary (which only needs the document, with no
/// database connection).
fn build_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(crate::openapi::base_document())
        .routes(routes!(health))
        .merge(crate::artifacts_api::router())
        .merge(crate::models_api::router())
        .merge(crate::runs_api::router())
        .merge(crate::runs_write_api::router())
        .merge(crate::results_api::router())
        .merge(crate::version_api::router())
        .merge(crate::events_api::router())
}

pub fn router(pool: SqlitePool, config: Config) -> Router {
    let dashboard_dist = config.dashboard_dist.clone();
    let state = AppState {
        pool,
        config,
        events: EventBus::default(),
    };
    let (router, openapi) = build_router().split_for_parts();

    let router = router
        // Serves both `/openapi.json` (the raw document) and interactive
        // Swagger UI at `/docs` - neither versioned under `/api/v1`.
        .merge(SwaggerUi::new("/docs").url("/openapi.json", openapi));

    // Optional built-dashboard serving: only paths no route above matched
    // reach the fallback, so API/health/docs always take precedence. Any
    // unknown path (a client-side route on reload) gets `index.html`. See
    // `specs/ingestion-service` - "Service optionally serves the built
    // dashboard".
    let router = match dashboard_dist {
        Some(dir) => {
            let index = ServeFile::new(dir.join("index.html"));
            router.fallback_service(ServeDir::new(dir).fallback(index))
        }
        None => router,
    };

    router.with_state(state)
}

/// The generated OpenAPI document alone, with no live database connection
/// required. Used by the `gen-openapi` binary and by the drift-check test.
pub fn openapi_document() -> utoipa::openapi::OpenApi {
    build_router().split_for_parts().1
}

/// Reports whether the service is running and able to reach its database.
#[utoipa::path(
    get,
    path = "/health",
    operation_id = "healthCheck",
    tag = "system",
    responses(
        (status = 200, description = "The service and its database connection are healthy."),
        (status = 503, description = "The service cannot reach its database."),
    )
)]
async fn health(State(state): State<AppState>) -> StatusCode {
    match crate::db::ping(&state.pool).await {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
