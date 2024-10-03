use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::debug;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Paths to the workspaces. Pass multiple times to add more.
    #[arg(long = "workspace-path", short)]
    workspace_paths: Option<Vec<PathBuf>>,
}

fn get_workspace_paths(arg_values: Vec<String>) -> Result<Vec<PathBuf>> {
    let args = Args::parse_from(arg_values);

    let workspace_paths = if let Some(workspace_paths) = args.workspace_paths {
        workspace_paths
    } else {
        vec![std::env::current_dir()?]
    };

    Ok(workspace_paths)
}

fn main() -> Result<()> {
    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")),
        )
        .init();

    latest_bin::ensure_latest_bin()?;

    for workspace_path in get_workspace_paths(std::env::args().collect())? {
        debug!("Processing workspace_root: {}", workspace_path.display());
        global::build_utils::generate_symlinks(Some(workspace_path))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;

    #[test]
    fn test_workspace_paths_empty() -> Result<()> {
        // Test when no paths are provided (expect default path).
        let paths = vec!["generate-binutils-symlinks".to_string()];
        let result = get_workspace_paths(paths).unwrap();

        assert_eq!(result, vec![std::env::current_dir()?]);

        Ok(())
    }

    #[test]
    fn test_workspace_paths_with_values() {
        // Test when multiple paths are provided.
        let paths = vec![
            "generate-binutils-symlinks".to_string(),
            "--workspace-path".to_string(),
            "/path/to/workspace1".to_string(),
            "--workspace-path".to_string(),
            "/path/to/workspace2".to_string(),
        ];
        let result = get_workspace_paths(paths).unwrap();
        assert_debug_snapshot!(result, @r###"
    [
        "/path/to/workspace1",
        "/path/to/workspace2",
    ]
    "###);
    }
}
