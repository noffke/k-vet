//! Writes the OpenAPI document to stdout.
//!
//! `cargo run --bin export-openapi > openapi.json` — the committed file is the input for
//! the frontend's orval codegen, and CI fails when either drifts.

use std::io::Write;
use std::process::ExitCode;

use k_vet_backend::ApiDoc;
use utoipa::OpenApi;

fn main() -> ExitCode {
    let document = match ApiDoc::openapi().to_pretty_json() {
        Ok(json) => json,
        Err(error) => {
            let _ = writeln!(
                std::io::stderr(),
                "cannot serialize the OpenAPI document: {error}"
            );
            return ExitCode::FAILURE;
        }
    };
    match writeln!(std::io::stdout(), "{document}") {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(
                std::io::stderr(),
                "cannot write the OpenAPI document: {error}"
            );
            ExitCode::FAILURE
        }
    }
}
