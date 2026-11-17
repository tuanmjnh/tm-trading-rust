use std::error::Error;
use std::fs;
use std::path::PathBuf;

use tm_trading_contracts::ApiDoc;
use utoipa::OpenApi;

fn main() -> Result<(), Box<dyn Error>> {
    let output_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/api/openapi.yaml")
        .canonicalize()
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/api/openapi.yaml")
        });

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(output_path, ApiDoc::openapi().to_yaml()?)?;
    Ok(())
}
