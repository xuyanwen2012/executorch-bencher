//! Asserts the checked-in `openapi/openapi.json` matches what the server
//! generates at runtime. See `specs/api-documentation` - "Checked-in
//! OpenAPI contract stays in sync with the runtime document".
//!
//! Regenerate with `cargo run --bin gen-openapi` when this fails after an
//! intentional route or schema change.

use executorch_bencher::http;
use serde_json::Value;
use std::path::Path;

#[test]
fn checked_in_openapi_json_matches_the_runtime_document() {
    let checked_in_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("openapi/openapi.json");
    let checked_in_text = std::fs::read_to_string(&checked_in_path).unwrap_or_else(|err| {
        panic!(
            "{} should exist - run `cargo run --bin gen-openapi` first: {err}",
            checked_in_path.display()
        )
    });
    let checked_in: Value =
        serde_json::from_str(&checked_in_text).expect("checked-in file should be valid JSON");

    let runtime = serde_json::to_value(http::openapi_document())
        .expect("the generated OpenAPI document always serializes to JSON");

    assert_eq!(
        checked_in, runtime,
        "openapi/openapi.json is stale - run `cargo run --bin gen-openapi` to regenerate it"
    );
}
