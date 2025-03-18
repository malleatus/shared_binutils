use anyhow::Result;
use clap::Parser;
use config::{read_config, Config};
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

fn get_workspace_paths(arg_values: Vec<String>, config: &Config) -> Result<Vec<PathBuf>> {
    let args = Args::parse_from(arg_values);

    let workspace_paths = if let Some(workspace_paths) = args.workspace_paths {
        workspace_paths
    } else if let Some(crate_locations) = &config.crate_locations {
        crate_locations
            .iter()
            .map(|location| {
                let expanded = shellexpand::tilde(&location);
                PathBuf::from(expanded.into_owned())
            })
            .collect()
    } else {
        vec![std::env::current_dir()?]
    };

    Ok(workspace_paths)
}

fn run(args: Vec<String>, config: &Config) -> Result<()> {
    for workspace_path in get_workspace_paths(args, config)? {
        debug!("Processing workspace_root: {}", workspace_path.display());
        let cargo_toml_path = workspace_path.join("Cargo.toml");
        if !cargo_toml_path.exists() {
            debug!(
                "Skipping workspace without Cargo.toml: {}",
                workspace_path.display()
            );
            continue;
        }
        global::build_utils::generate_symlinks(Some(workspace_path))?;
    }

    Ok(())
}

fn main() -> Result<()> {
    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")),
        )
        .init();

    latest_bin::ensure_latest_bin()?;

    let config = read_config(None)?;
    let args = std::env::args().collect();

    run(args, &config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::Config;
    use insta::{assert_debug_snapshot, assert_snapshot};
    use test_utils::{
        create_workspace_with_packages, setup_test_environment, stabilize_home_paths, FakePackage,
    };

    #[test]
    fn test_workspace_paths_no_args_no_config() -> Result<()> {
        let config = Config {
            crate_locations: None,
            tmux: None,
            shell_caching: None,
        };

        // Test when no paths are provided (expect default path).
        let args = vec!["generate-binutils-symlinks".to_string()];
        let result = get_workspace_paths(args, &config).unwrap();

        assert_eq!(result, vec![std::env::current_dir()?]);

        Ok(())
    }

    #[test]
    fn test_workspace_paths_with_args_no_config() {
        let config = Config {
            crate_locations: None,
            tmux: None,
            shell_caching: None,
        };

        // Test when multiple paths are provided.
        let args = vec![
            "generate-binutils-symlinks".to_string(),
            "--workspace-path".to_string(),
            "/path/to/workspace1".to_string(),
            "--workspace-path".to_string(),
            "/path/to/workspace2".to_string(),
        ];
        let result = get_workspace_paths(args, &config).unwrap();
        assert_debug_snapshot!(result, @r###"
    [
        "/path/to/workspace1",
        "/path/to/workspace2",
    ]
    "###);
    }

    #[test]
    fn test_workspace_paths_empty_args_with_config() -> Result<()> {
        let env = setup_test_environment();

        let workspace_dir = env.home.join("workspace");
        create_workspace_with_packages(
            workspace_dir.as_path(),
            vec![
                FakePackage {
                    name: "foo".to_string(),
                    bins: vec![],
                },
                FakePackage {
                    name: "bar".to_string(),
                    bins: vec![],
                },
                FakePackage {
                    name: "baz".to_string(),
                    bins: vec![],
                },
            ],
        );

        let config = Config {
            crate_locations: Some(vec![String::from("~/workspace")]),
            tmux: None,
            shell_caching: None,
        };

        let args = vec!["generate-binutils-symlinks".to_string()];

        let result = get_workspace_paths(args, &config).unwrap();
        let debug_output = format!("{:#?}", result);

        assert_snapshot!(stabilize_home_paths(&env, &debug_output), @r###"
        [
            "~/workspace",
        ]
        "###);

        Ok(())
    }

    #[test]
    fn test_workspace_paths_skips_paths_without_cargo_config() -> Result<()> {
        let env = setup_test_environment();
        let invalid_dir = env.home.join("invalid_dir");
        std::fs::create_dir_all(&invalid_dir)?;

        let config = Config {
            crate_locations: Some(vec![String::from("~/invalid_dir")]),
            tmux: None,
            shell_caching: None,
        };

        let args = vec!["generate-binutils-symlinks".to_string()];

        run(args, &config)?;

        Ok(())
    }
}
