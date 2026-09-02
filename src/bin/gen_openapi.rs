//! Regenerates the checked-in OpenAPI contract at `openapi/openapi.json`
//! from the same route and type definitions the running server uses.
//!
//! Usage: `cargo run --bin gen-openapi`
//!
//! See `specs/api-documentation` - "Checked-in OpenAPI contract stays in
//! sync with the runtime document".

use executorch_bencher::http;
use std::path::Path;

fn main() {
    let document = http::openapi_document();
    let json = serde_json::to_string_pretty(&document)
        .expect("the generated OpenAPI document always serializes to JSON");

    let out_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("openapi/openapi.json");
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|err| {
            eprintln!("failed to create {}: {err}", parent.display());
            std::process::exit(1);
        });
    }
    std::fs::write(&out_path, format!("{json}\n")).unwrap_or_else(|err| {
        eprintln!("failed to write {}: {err}", out_path.display());
        std::process::exit(1);
    });

    println!("wrote {}", out_path.display());
}
