use clap::Parser;
use std::path::PathBuf;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "Regenerate config's Lua types")]
struct Args {
    /// Sets the output file path. Defaults to `config_schema.json` in the crate root.
    #[arg(short, long, value_name = "FILE")]
    output: Option<String>,
}

fn generate_lua_types() -> Result<()> {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    internal_bins::lua::process_file(
        crate_root.join("../config/src/lib.rs"),
        crate_root.join("../config/init.lua"),
        vec![
            "Config",
            "ShellCache",
            "Tmux",
            "Session",
            "Command",
            "Window",
        ],
    );

    Ok(())
}

fn main() -> Result<()> {
    // Initialize tracing, use `info` by default
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    latest_bin::ensure_latest_bin()?;

    generate_lua_types()?;

    Ok(())
}
