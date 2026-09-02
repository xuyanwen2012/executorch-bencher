//! Verifies the generated OpenAPI document, `/docs`, and `/api/v1/version`
//! match `specs/api-documentation` and `specs/ingestion-service` - "HTTP
//! error responses use a consistent JSON envelope".

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use executorch_bencher::http;
use serde_json::Value;
use tower::ServiceExt;

async fn fetch_json(app: axum::Router, uri: &str) -> Value {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET {uri} did not return 200"
    );
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).expect("response body should be valid JSON")
}

async fn openapi_document() -> Value {
    let (pool, ctx) = common::migrated_pool().await;
    let app = http::router(pool, ctx.config());
    fetch_json(app, "/openapi.json").await
}

#[tokio::test]
async fn openapi_json_is_a_well_formed_document_with_expected_metadata() {
    let doc = openapi_document().await;

    assert!(
        doc["openapi"].as_str().unwrap().starts_with("3."),
        "expected an OpenAPI 3.x document, got {:?}",
        doc["openapi"]
    );
    assert_eq!(doc["info"]["title"], "ExecuTorch Bencher API");
    assert_eq!(doc["info"]["version"], executorch_bencher::version_api::API_VERSION);
    assert!(
        doc["info"]["license"].is_null(),
        "license should be omitted"
    );
    assert!(
        doc.get("servers").is_none() || doc["servers"].as_array().unwrap().is_empty(),
        "servers block should be omitted"
    );

    let tag_names: Vec<&str> = doc["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(tag_names, vec!["system", "runs", "models", "artifacts", "events"]);
}

#[tokio::test]
async fn enum_schemas_list_exactly_their_documented_stable_values() {
    let doc = openapi_document().await;
    let schemas = &doc["components"]["schemas"];

    assert_eq!(
        schemas["ExitStatus"]["enum"],
        serde_json::json!([
            "succeeded",
            "crashed",
            "timed_out",
            "cancelled",
            "infrastructure_error"
        ])
    );
    assert_eq!(
        schemas["CorrectnessResult"]["enum"],
        serde_json::json!(["passed", "failed", "not_checked", "validator_error"])
    );
    assert_eq!(
        schemas["ArtifactKind"]["enum"],
        serde_json::json!([
            "prompt",
            "stdout",
            "stderr",
            "output",
            "crash_log",
            "logcat",
            "correctness_report"
        ])
    );
    assert_eq!(
        schemas["ModelStorageMode"]["enum"],
        serde_json::json!(["external", "managed"])
    );
}

#[tokio::test]
async fn sha256_fields_are_documented_as_lowercase_hex_digests() {
    let doc = openapi_document().await;
    let schemas = &doc["components"]["schemas"];

    for (schema_name, field) in [
        ("ArtifactMetadataResponse", "sha256"),
        ("UploadResponse", "sha256"),
        ("ModelAssetResponse", "sha256"),
        ("ModelAssetSummary", "sha256"),
    ] {
        let description = schemas[schema_name]["properties"][field]["description"]
            .as_str()
            .unwrap_or_else(|| panic!("{schema_name}.{field} should have a description"));
        assert!(
            description.contains("64-character") && description.contains("hexadecimal"),
            "{schema_name}.{field} description should document the SHA-256 format, got: {description:?}"
        );
    }
}

#[tokio::test]
async fn optional_response_fields_are_nullable() {
    let doc = openapi_document().await;
    let run_response = &doc["components"]["schemas"]["RunResponse"];
    let required: Vec<&str> = run_response["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    for optional_field in [
        "finished_at",
        "output_preview",
        "model_asset",
        "input_artifact",
    ] {
        assert!(
            !required.contains(&optional_field),
            "{optional_field} should not be required"
        );
    }
}

#[tokio::test]
async fn run_response_schema_exposes_only_its_real_fields() {
    let doc = openapi_document().await;
    let properties = doc["components"]["schemas"]["RunResponse"]["properties"]
        .as_object()
        .unwrap();
    let mut field_names: Vec<&str> = properties.keys().map(String::as_str).collect();
    field_names.sort_unstable();

    // Exactly the fields the handler serializes - pinned in both
    // directions so the contract can neither over- nor under-claim.
    let mut expected = vec![
        "id",
        "started_at",
        "finished_at",
        "exit_status",
        "correctness_result",
        "output_preview",
        "model_asset",
        "input_artifact",
        "output_artifact",
        "stdout_artifact",
        "stderr_artifact",
        "crash_artifact",
        "repetition",
        "command_args",
        "command_line",
        "input_parameters",
        "env_vars",
        "env_allowlist_version",
        "collector_version",
        "platform",
        "device_class",
        "device_serial",
        "device_model",
        "bsp_version",
        "sumd_driver_version",
        "device_uptime_seconds",
        "host_os",
        "host_kernel",
        "host_cpu_model",
        "host_cpu_count",
        "host_memory_bytes",
        "host_accelerator",
        "host_accelerator_driver",
        "battery_charging",
        "initial_temperature_celsius",
        "max_temperature_celsius",
        "thermal_throttling",
        "gpu_clock_mhz",
        "mif_clock_mhz",
        "int_clock_mhz",
        "git_commit_sha",
        "git_dirty",
        "git_branch",
        "git_commit_timestamp",
        "git_commit_subject",
        "executable_sha256",
        "prompt_sha256",
        "input_token_count",
        "output_token_count",
        "prefill_tokens_per_sec",
        "decode_tokens_per_sec",
        "error_summary",
    ];
    expected.sort_unstable();
    assert_eq!(field_names, expected);
}

#[tokio::test]
async fn numeric_run_fields_document_their_units_and_nullability() {
    let doc = openapi_document().await;
    let schemas = &doc["components"]["schemas"];

    // A nullable `$ref` field is rendered as `oneOf: [null, {$ref,
    // description}]`, so the description lives on the variant.
    let description = |schema: &str, field: &str| -> String {
        let property = &schemas[schema]["properties"][field];
        property["description"]
            .as_str()
            .or_else(|| {
                property["oneOf"]
                    .as_array()?
                    .iter()
                    .find_map(|variant| variant["description"].as_str())
            })
            .unwrap_or_else(|| panic!("{schema}.{field} should have a description"))
            .to_string()
    };
    assert!(description("RunResponse", "gpu_clock_mhz").contains("MHz"));
    assert!(description("RunResponse", "initial_temperature_celsius").contains("Celsius"));
    assert!(description("RunResponse", "device_uptime_seconds").contains("seconds"));
    assert!(description("RunResponse", "prefill_tokens_per_sec").contains("tokens per second"));
    assert!(description("RunSummaryResponse", "decode_tokens_per_sec").contains("tokens per second"));
    assert!(description("ResultRowResponse", "gpu_clock_mhz").contains("MHz"));
    assert!(description("ResultRowResponse", "decode").contains("tokens per second"));
    assert!(description("MetricStatsResponse", "median").contains("tokens per second"));

    let not_required = |schema: &str, field: &str| {
        let required: Vec<&str> = schemas[schema]["required"]
            .as_array()
            .map(|r| r.iter().map(|v| v.as_str().unwrap()).collect())
            .unwrap_or_default();
        assert!(
            !required.contains(&field),
            "{schema}.{field} should be nullable (not required)"
        );
    };
    not_required("RunResponse", "decode_tokens_per_sec");
    not_required("RunSummaryResponse", "decode_tokens_per_sec");
    not_required("ResultRowResponse", "decode");
    not_required("ResultRowResponse", "prefill");
    not_required("ResultRowResponse", "git_branch");
    not_required("ResultRowResponse", "git_commit_timestamp");
    not_required("ResultRowResponse", "git_commit_subject");
    for field in ["command_line", "error_summary", "git_branch", "git_commit_timestamp", "git_commit_subject"] {
        not_required("RunResponse", field);
    }
}

#[tokio::test]
async fn artifact_upload_is_documented_as_query_params_and_a_raw_body() {
    let doc = openapi_document().await;
    let upload_op = &doc["paths"]["/api/v1/artifacts"]["post"];

    let param_names: Vec<&str> = upload_op["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(param_names.contains(&"kind"));
    assert!(param_names.contains(&"original_name"));
    for param in upload_op["parameters"].as_array().unwrap() {
        assert_eq!(param["in"], "query");
    }

    let content = upload_op["requestBody"]["content"].as_object().unwrap();
    assert!(
        !content.contains_key("multipart/form-data"),
        "upload should not be documented as multipart"
    );
    assert!(content.contains_key("application/octet-stream"));
}

#[tokio::test]
async fn artifact_content_and_download_declare_binary_streaming_responses() {
    let doc = openapi_document().await;

    for (path, op) in [
        ("/api/v1/artifacts/{id}/content", "getArtifactContent"),
        ("/api/v1/artifacts/{id}/download", "downloadArtifact"),
    ] {
        let response = &doc["paths"][path]["get"]["responses"]["200"];
        assert_eq!(doc["paths"][path]["get"]["operationId"], op);
        let content = response["content"].as_object().unwrap();
        assert!(
            content.contains_key("application/octet-stream"),
            "{path} 200 response should declare application/octet-stream content"
        );
        assert!(
            !content.contains_key("application/json"),
            "{path} 200 response should not be modeled as a JSON string body"
        );
    }
}

#[tokio::test]
async fn version_endpoint_matches_documented_schema_and_built_package_version() {
    let (pool, ctx) = common::migrated_pool().await;
    let app = http::router(pool, ctx.config());
    let version = fetch_json(app, "/api/v1/version").await;

    assert_eq!(version["server_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(version["api_version"], executorch_bencher::version_api::API_VERSION);
    assert_eq!(version["schema_version"], 1);
    assert_eq!(version["minimum_runner_version"], "0.1.0");
}

#[tokio::test]
async fn operation_ids_are_present_and_unique() {
    let doc = openapi_document().await;
    let mut ids = Vec::new();
    for methods in doc["paths"].as_object().unwrap().values() {
        for op in methods.as_object().unwrap().values() {
            let id = op["operationId"]
                .as_str()
                .expect("every operation should declare an operationId")
                .to_string();
            ids.push(id);
        }
    }
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        ids.len(),
        unique.len(),
        "operationId values must be unique: {ids:?}"
    );
}

#[tokio::test]
async fn document_covers_run_creation_and_events_but_not_finalize_or_progress() {
    let doc = openapi_document().await;
    let paths = doc["paths"].as_object().unwrap();

    // Implemented write and stream operations are present.
    let create = &doc["paths"]["/api/v1/runs"]["post"];
    assert_eq!(create["operationId"], "createRun");
    assert!(
        create["requestBody"]["content"]["application/json"]["schema"]["$ref"]
            .as_str()
            .unwrap()
            .ends_with("/CreateRunRequest")
    );
    for status in ["201", "400", "409"] {
        assert!(create["responses"][status].is_object(), "createRun documents {status}");
    }
    let events = &doc["paths"]["/api/v1/events"]["get"];
    assert_eq!(events["operationId"], "streamEvents");
    assert!(events["responses"]["200"]["content"]["text/event-stream"].is_object());
    let description = events["responses"]["200"]["description"].as_str().unwrap();
    for name in ["run.created", "artifact.created", "model.registered"] {
        assert!(description.contains(name), "events description names {name}");
    }
    for component in ["RunCreatedEvent", "ArtifactCreatedEvent", "ModelRegisteredEvent"] {
        assert!(
            doc["components"]["schemas"][component].is_object(),
            "{component} payload schema is documented"
        );
        assert!(description.contains(component));
    }

    // Finalize and progress do not exist.
    for path in paths.keys() {
        assert!(!path.contains("finalize"), "unexpected {path}");
        assert!(!path.contains("progress"), "unexpected {path}");
    }

    // Tags match the operations that use them.
    let tags: Vec<&str> = doc["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(tags.contains(&"events"));
    assert!(!tags.contains(&"analysis"));

    // The model list documents its hash filter.
    let list_models = &doc["paths"]["/api/v1/models"]["get"];
    assert!(query_param_names(list_models).contains(&"sha256"));
}

/// The run creation request and the run response describe one entity:
/// every request field appears in the response with the same enumeration,
/// and the response adds only derived views.
#[tokio::test]
async fn run_request_and_response_schemas_agree() {
    let doc = openapi_document().await;
    let schemas = &doc["components"]["schemas"];
    let request = schemas["CreateRunRequest"]["properties"].as_object().unwrap();
    let response = schemas["RunResponse"]["properties"].as_object().unwrap();
    let request_only = ["input_artifact_id", "output_artifact_id", "stdout_artifact_id", "stderr_artifact_id", "crash_artifact_id", "model_asset_id"];
    let response_only = ["input_artifact", "output_artifact", "stdout_artifact", "stderr_artifact", "crash_artifact", "model_asset"];
    for (name, prop) in request {
        if request_only.contains(&name.as_str()) {
            continue;
        }
        let counterpart = response
            .get(name)
            .unwrap_or_else(|| panic!("request field {name} has no response counterpart"));
        let enum_ref = |p: &Value| -> Option<String> {
            p["$ref"]
                .as_str()
                .map(str::to_string)
                .or_else(|| {
                    p["oneOf"]
                        .as_array()?
                        .iter()
                        .find_map(|v| v["$ref"].as_str().map(str::to_string))
                })
                .or_else(|| p["allOf"].as_array()?.iter().find_map(|v| v["$ref"].as_str().map(str::to_string)))
        };
        if let Some(r) = enum_ref(prop) {
            assert_eq!(enum_ref(counterpart).as_deref(), Some(r.as_str()), "{name} enumeration differs");
        }
    }
    for (name, _) in response {
        if response_only.contains(&name.as_str()) {
            continue;
        }
        assert!(request.contains_key(name), "response field {name} cannot be submitted");
    }
}

fn query_param_names(operation: &Value) -> Vec<&str> {
    operation["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["in"] == "query")
        .map(|p| p["name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn run_listing_and_results_operations_are_documented_with_their_parameters() {
    let doc = openapi_document().await;

    let list = &doc["paths"]["/api/v1/runs"]["get"];
    assert_eq!(list["operationId"], "listRuns");
    let params = query_param_names(list);
    for expected in [
        "limit",
        "cursor",
        "platform",
        "device_class",
        "device_serial",
        "host_accelerator",
        "model_asset_id",
        "git_commit_sha",
        "git_branch",
        "git_dirty",
        "sumd_driver_version",
        "bsp_version",
        "gpu_clock_mhz",
        "mif_clock_mhz",
        "int_clock_mhz",
        "prompt_sha256",
        "exit_status",
        "correctness_result",
    ] {
        assert!(params.contains(&expected), "listRuns should document {expected}");
    }
    let list_schema = &doc["components"]["schemas"]["RunListResponse"]["properties"];
    assert!(list_schema["items"].is_object());
    assert!(list_schema["next_cursor"].is_object());

    let results = &doc["paths"]["/api/v1/results"]["get"];
    assert_eq!(results["operationId"], "getResults");
    let params = query_param_names(results);
    for expected in [
        "device_serial",
        "model_asset_id",
        "git_commit_sha",
        "git_branch",
        "git_dirty",
        "sumd_driver_version",
        "bsp_version",
        "prompt_sha256",
    ] {
        assert!(params.contains(&expected), "getResults should document {expected}");
    }
    let results_schema = &doc["components"]["schemas"]["ResultsResponse"]["properties"];
    assert!(results_schema["rows"].is_object());
    assert!(results_schema["truncated"].is_object());
    assert!(results_schema["facets"].is_object());
}

#[tokio::test]
async fn swagger_ui_responds_and_references_openapi_json() {
    let (pool, ctx) = common::migrated_pool().await;
    let app = http::router(pool, ctx.config());

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/docs/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8_lossy(&bytes);
    assert!(
        html.contains("swagger-initializer.js"),
        "Swagger UI page should load its initializer script"
    );

    // The initializer script is where utoipa-swagger-ui injects the actual
    // configured document URL.
    let init_js = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/docs/swagger-initializer.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(init_js.status(), StatusCode::OK);
    let init_bytes = axum::body::to_bytes(init_js.into_body(), usize::MAX)
        .await
        .unwrap();
    let init_js_text = String::from_utf8_lossy(&init_bytes);
    assert!(
        init_js_text.contains("/openapi.json"),
        "Swagger UI's initializer should point at /openapi.json, got: {init_js_text}"
    );

    let redirect = app
        .oneshot(Request::builder().uri("/docs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(redirect.status().is_redirection());
}
