use anyhow::Result;
use clap::Parser;
use tracing::debug;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the workspace
    #[arg(long, short)]
    workspace_path: Option<String>,
}

fn main() -> Result<()> {
    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    latest_bin::ensure_latest_bin()?;

    let args = Args::parse();

    let workspace_root = if let Some(workspace_path) = args.workspace_path {
        std::path::PathBuf::from(workspace_path)
    } else {
        latest_bin::get_crate_root()?
    };

    debug!("workspace_root: {}", workspace_root.display());

    global::build_utils::generate_symlinks(Some(workspace_root))?;

    Ok(())
}
