use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

/// Sets up a local dotfiles repository with a predefined directory structure and symlinks.
///
/// This command:
/// - Creates a directory structure for local dotfiles (crates, nvim configs, snippets)
/// - Optionally clones a git repository as the base
/// - Sets up symlinks for:
///   - Local crates directory (~/src/rwjblue/dotfiles/binutils/local-crates -> local-dotfiles/crates)
///   - Neovim local config (~/.config/nvim/lua/local_config -> local-dotfiles/nvim/lua/local_config)
///
/// # Environment Variables:
/// - HOME: Required for path expansion
/// - RUST_LOG: Optional, controls logging verbosity (e.g. debug, info)
#[derive(Parser, Debug)]
struct Args {
    /// Git repository URL to clone (optional)
    #[arg(long)]
    repo: Option<String>,

    /// Path where the local-dotfiles should be created/cloned
    #[arg(long)]
    local_dotfiles_path: String,

    /// Path where local crates should be symlinked into
    #[arg(long)]
    local_crates_target_path: String,

    /// Show what would happen without making any changes
    #[arg(long)]
    dry_run: bool,
}

fn ensure_directory_structure(base_path: &Path, dry_run: bool) -> Result<()> {
    debug!("Creating directory structure at {}", base_path.display());

    let files: BTreeMap<&str, &str> = BTreeMap::from([
        (
            "binutils-config/local.config.lua",
            "return require('config')",
        ),
        ("crates/.gitkeep", ""),
        ("nvim/lua/local_config/config/autocmds.lua", ""),
        ("nvim/lua/local_config/config/options.lua", ""),
        ("nvim/lua/local_config/config/keymaps.lua", ""),
        ("nvim/lua/local_config/plugins/.gitkeep", ""),
        ("nvim/snippets/.gitkeep", ""),
    ]);

    for (file_path, contents) in files {
        let path = base_path.join(file_path);
        let parent_dir = path.parent().unwrap();
        debug!("Creating directory: {}", parent_dir.display());
        if !dry_run {
            fs::create_dir_all(parent_dir)
                .with_context(|| format!("Failed to create directory: {}", file_path))?;
        }

        if !path.exists() {
            debug!("Creating file: {}", path.display());
            if !dry_run {
                fs::write(&path, contents)
                    .with_context(|| format!("Failed to create: {}", path.display()))?;
            }
        } else {
            debug!("File already exists, skipping: {}", path.display());
        }
    }

    Ok(())
}

fn get_remote_url(repo_path: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_path)
        .output()
        .context("Failed to get remote URL")?;

    if !output.status.success() {
        anyhow::bail!("Failed to get remote URL");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn verify_remote_matches(repo_path: &Path, expected_url: &str) -> Result<()> {
    let remote_url = get_remote_url(repo_path)?;
    if remote_url != expected_url {
        anyhow::bail!(
            "Repository remote URL '{}' does not match expected URL '{}'",
            remote_url,
            expected_url
        );
    }
    Ok(())
}

fn clone_repo(repo_url: &str, target_path: &Path, dry_run: bool) -> Result<()> {
    info!(
        "Cloning repository {} to {}",
        repo_url,
        target_path.display()
    );

    if dry_run {
        return Ok(());
    }

    let output = Command::new("git")
        .args(["clone", repo_url, &target_path.to_string_lossy()])
        .output()
        .with_context(|| format!("Failed to clone repository: {}", repo_url))?;

    debug!(
        "Git clone stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    if !output.stderr.is_empty() {
        warn!(
            "Git clone stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

fn setup_symlinks(base_path: &Path, local_crates_path: &Path, dry_run: bool) -> Result<()> {
    debug!("Setting up symlinks");

    if let Some(parent) = local_crates_path.parent() {
        debug!("Creating parent directory: {}", parent.display());
        if !dry_run {
            fs::create_dir_all(parent)?;
        }
    }

    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    let nvim_config_path = PathBuf::from(&home).join(".config/nvim/lua/local_config");
    let binutils_local_config_path = PathBuf::from(&home).join(".config/binutils/local.config.lua");

    if let Some(parent) = nvim_config_path.parent() {
        debug!("Creating ~/.config/nvim path: {}", parent.display());
        if !dry_run {
            fs::create_dir_all(parent)?;
        }
    }

    if let Some(parent) = binutils_local_config_path.parent() {
        debug!("Creating ~/.config/binutils path: {}", parent.display());
        if !dry_run {
            fs::create_dir_all(parent)?;
        }
    }

    if local_crates_path.exists() {
        if local_crates_path.is_symlink() {
            debug!(
                "Removing existing crates symlink: {}",
                local_crates_path.display()
            );
            if !dry_run {
                fs::remove_file(local_crates_path)?;
            }
        } else {
            anyhow::bail!(
                "Target path exists but is not a symlink: {}",
                local_crates_path.display()
            );
        }
    }

    if nvim_config_path.exists() {
        if nvim_config_path.is_symlink() {
            debug!(
                "Removing existing nvim config symlink: {}",
                nvim_config_path.display()
            );
            if !dry_run {
                fs::remove_file(&nvim_config_path)?;
            }
        } else {
            anyhow::bail!(
                "Target path exists but is not a symlink: {}",
                nvim_config_path.display()
            );
        }
    }

    if binutils_local_config_path.exists() {
        if binutils_local_config_path.is_symlink() {
            debug!(
                "Removing existing nvim config symlink: {}",
                binutils_local_config_path.display()
            );
            if !dry_run {
                fs::remove_file(&binutils_local_config_path)?;
            }
        } else {
            anyhow::bail!(
                "Target path exists but is not a symlink: {}",
                binutils_local_config_path.display()
            );
        }
    }

    debug!(
        "Creating crates symlink: {} -> {}",
        base_path.join("crates").display(),
        local_crates_path.display()
    );
    if !dry_run {
        std::os::unix::fs::symlink(base_path.join("crates"), local_crates_path).with_context(
            || {
                format!(
                    "Failed to create symlink for crates directory to {}",
                    local_crates_path.display()
                )
            },
        )?;
    }

    debug!(
        "Creating nvim local lua config symlink: {} -> {}",
        base_path.join("nvim/lua/local_config").display(),
        nvim_config_path.display()
    );
    if !dry_run {
        std::os::unix::fs::symlink(base_path.join("nvim/lua/local_config"), nvim_config_path)
            .with_context(|| "Failed to create symlink for nvim local_config directory")?;
    }

    debug!(
        "Creating binutils local config symlink: {} -> {}",
        base_path.join("binutils-config/local.config.lua").display(),
        binutils_local_config_path.display()
    );
    if !dry_run {
        std::os::unix::fs::symlink(
            base_path.join("binutils-config/local.config.lua"),
            binutils_local_config_path,
        )
        .with_context(|| "Failed to create symlink for binutils local.config.lua")?;
    }
    Ok(())
}

fn run(args: Vec<String>) -> Result<()> {
    let args = Args::parse_from(args);

    let local_dotfiles_path = shellexpand::tilde(&args.local_dotfiles_path);
    let local_dotfiles_path = Path::new(&*local_dotfiles_path).to_path_buf();

    let local_crates_target_path = shellexpand::tilde(&args.local_crates_target_path);
    let local_crates_target_path = Path::new(&*local_crates_target_path).to_path_buf();

    if args.dry_run {
        info!("Running in dry-run mode - no changes will be made");
    }

    if let Some(repo) = args.repo {
        if local_dotfiles_path.exists() {
            verify_remote_matches(&local_dotfiles_path, &repo)?;
        } else {
            clone_repo(&repo, &local_dotfiles_path, args.dry_run)?;
        }
    }

    ensure_directory_structure(&local_dotfiles_path, args.dry_run)?;
    setup_symlinks(
        &local_dotfiles_path,
        &local_crates_target_path,
        args.dry_run,
    )?;

    info!("Local dotfiles setup completed successfully!");
    info!(
        "Created directory structure at: {}",
        local_dotfiles_path.display()
    );
    info!(
        "Symlinked crates directory to: {}",
        local_crates_target_path.display()
    );
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")),
        )
        .init();

    latest_bin::ensure_latest_bin()?;

    let args: Vec<String> = std::env::args().collect();
    run(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;
    use std::collections::BTreeMap;
    use std::fs;
    use std::process::Command;
    use test_utils::setup_test_environment;

    fn setup_test_repo(git_repo_path: &PathBuf) -> Result<()> {
        fs::create_dir_all(git_repo_path)?;

        Command::new("git")
            .args(["init"])
            .current_dir(git_repo_path)
            .output()?;

        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(git_repo_path)
            .output()?;
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(git_repo_path)
            .output()?;
        Command::new("git")
            .args(["add", "."])
            .current_dir(git_repo_path)
            .output()?;
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(git_repo_path)
            .output()?;

        Ok(())
    }

    #[test]
    fn test_clone_repo() -> Result<()> {
        let env = setup_test_environment();
        let git_repo_path = env.home.join("src/local-dotfiles-git-repo");
        let target_path = env.home.join("cloned-repo");

        setup_test_repo(&git_repo_path)?;
        let repo_contents = fixturify::read(&git_repo_path)?;

        clone_repo(&git_repo_path.to_string_lossy(), &target_path, false)?;
        let cloned_contents = fixturify::read(&target_path)?;

        assert_eq!(repo_contents, cloned_contents);

        Ok(())
    }

    #[test]
    fn test_run_with_existing_matching_repo() -> Result<()> {
        let env = setup_test_environment();
        let git_repo_path = env.home.join("src/local-dotfiles-git-repo");
        setup_test_repo(&git_repo_path)?;

        let target_path = env.home.join("src/workstuff/local-dotfiles");
        clone_repo(&git_repo_path.to_string_lossy(), &target_path, false)?;

        // Running again with same repo URL should succeed
        run(vec![
            "setup-local-dotfiles".to_string(),
            "--repo".to_string(),
            git_repo_path.to_string_lossy().to_string(),
            "--local-dotfiles-path".to_string(),
            "~/src/workstuff/local-dotfiles".to_string(),
            "--local-crates-target-path".to_string(),
            "~/src/rwjblue/dotfiles/binutils/local-crates".to_string(),
        ])?;

        Ok(())
    }

    #[test]
    fn test_run_with_existing_mismatched_repo() -> Result<()> {
        let env = setup_test_environment();
        let git_repo_path = env.home.join("src/local-dotfiles-git-repo");
        setup_test_repo(&git_repo_path)?;

        let target_path = env.home.join("src/workstuff/local-dotfiles");
        clone_repo(&git_repo_path.to_string_lossy(), &target_path, false)?;

        // Running with different repo URL should fail
        let result = run(vec![
            "setup-local-dotfiles".to_string(),
            "--repo".to_string(),
            "https://different-repo-url.git".to_string(),
            "--local-dotfiles-path".to_string(),
            "~/src/workstuff/local-dotfiles".to_string(),
            "--local-crates-target-path".to_string(),
            "~/src/rwjblue/dotfiles/binutils/local-crates".to_string(),
        ]);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("does not match expected URL"));

        Ok(())
    }

    #[test]
    fn test_run_with_git_repo() -> Result<()> {
        let env = setup_test_environment();
        let git_repo_path = env.home.join("src/local-dotfiles-git-repo");
        setup_test_repo(&git_repo_path)?;

        run(vec![
            "setup-local-dotfiles".to_string(),
            "--repo".to_string(),
            git_repo_path.to_string_lossy().to_string(),
            "--local-dotfiles-path".to_string(),
            "~/src/workstuff/local-dotfiles".to_string(),
            "--local-crates-target-path".to_string(),
            "~/src/rwjblue/dotfiles/binutils/local-crates".to_string(),
        ])?;

        let base_path = env.home.join("src/workstuff/local-dotfiles");
        let local_crates_path = env.home.join("src/rwjblue/dotfiles/binutils/local-crates");
        let nvim_config_lua_path = env.home.join(".config/nvim/lua/local_config");
        let binutils_local_config_path = env.home.join(".config/binutils/local.config.lua");

        assert!(base_path.exists());
        assert!(base_path.join(".git").exists());
        assert!(base_path.join("crates/.gitkeep").exists());
        assert!(base_path.join("nvim/snippets/.gitkeep").exists());

        assert!(local_crates_path.exists());
        assert!(local_crates_path.is_symlink());
        assert!(nvim_config_lua_path.exists());
        assert!(nvim_config_lua_path.is_symlink());
        assert!(binutils_local_config_path.exists());
        assert!(binutils_local_config_path.is_symlink());

        Ok(())
    }

    #[test]
    fn test_ensure_directory_structure_without_existing_structure() -> Result<()> {
        let env = setup_test_environment();
        let base_path = env.home.join("local-dotfiles");

        ensure_directory_structure(&base_path, false)?;

        let result = fixturify::read(&base_path)?;
        assert_debug_snapshot!(result, @r###"
        {
            "binutils-config/local.config.lua": "return require('config')",
            "crates/.gitkeep": "",
            "nvim/lua/local_config/config/autocmds.lua": "",
            "nvim/lua/local_config/config/keymaps.lua": "",
            "nvim/lua/local_config/config/options.lua": "",
            "nvim/lua/local_config/plugins/.gitkeep": "",
            "nvim/snippets/.gitkeep": "",
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_setup_symlinks() -> Result<()> {
        let env = setup_test_environment();
        let base_path = env.home.join("local-dotfiles");
        let local_crates_path = env.home.join("src/rwjblue/dotfiles/binutils/local-crates");

        ensure_directory_structure(&base_path, false)?;
        setup_symlinks(&base_path, &local_crates_path, false)?;

        // Verify symlinks
        assert!(local_crates_path.exists());
        assert!(local_crates_path.is_symlink());
        assert_eq!(fs::read_link(&local_crates_path)?, base_path.join("crates"));

        let nvim_config_path = env.home.join(".config/nvim/lua/local_config");
        assert!(nvim_config_path.exists());
        assert!(nvim_config_path.is_symlink());
        assert_eq!(
            fs::read_link(&nvim_config_path)?,
            base_path.join("nvim/lua/local_config")
        );

        let binutils_local_config_path = env.home.join(".config/binutils/local.config.lua");
        assert!(binutils_local_config_path.exists());
        assert!(binutils_local_config_path.is_symlink());
        assert_eq!(
            fs::read_link(&binutils_local_config_path)?,
            base_path.join("binutils-config/local.config.lua")
        );
        Ok(())
    }

    #[test]
    fn test_run_with_no_repo() -> Result<()> {
        let env = setup_test_environment();

        run(vec![
            "setup-local-dotfiles".to_string(),
            "--local-dotfiles-path".to_string(),
            "~/src/workstuff/local-dotfiles".to_string(),
            "--local-crates-target-path".to_string(),
            "~/src/rwjblue/dotfiles/binutils/local-crates".to_string(),
        ])?;

        let base_path = env.home.join("src/workstuff/local-dotfiles");
        let local_crates_path = env.home.join("src/rwjblue/dotfiles/binutils/local-crates");
        let nvim_config_lua_path = env.home.join(".config/nvim/lua/local_config");
        let binutils_local_config_path = env.home.join(".config/binutils/local.config.lua");

        assert!(base_path.exists());
        assert!(local_crates_path.exists());
        assert!(local_crates_path.is_symlink());
        assert!(nvim_config_lua_path.exists());
        assert!(nvim_config_lua_path.is_symlink());
        assert!(binutils_local_config_path.exists());
        assert!(binutils_local_config_path.is_symlink());

        Ok(())
    }

    #[test]
    fn test_run_with_dry_run() -> Result<()> {
        let env = setup_test_environment();

        run(vec![
            "setup-local-dotfiles".to_string(),
            "--local-dotfiles-path".to_string(),
            "~/src/workstuff/local-dotfiles".to_string(),
            "--local-crates-target-path".to_string(),
            "~/src/rwjblue/dotfiles/binutils/local-crates".to_string(),
            "--dry-run".to_string(),
        ])?;

        let base_path = env.home.join("src/workstuff/local-dotfiles");
        let local_crates_path = env.home.join("src/rwjblue/dotfiles/binutils/local-crates");
        let nvim_config_lua_path = env.home.join(".config/nvim/lua/local_config");

        assert!(!base_path.exists());
        assert!(!local_crates_path.exists());
        assert!(!nvim_config_lua_path.exists());

        Ok(())
    }

    #[test]
    fn test_ensure_directory_structure_with_existing_complete_structure() -> Result<()> {
        let env = setup_test_environment();
        let base_path = env.home.join("local-dotfiles");

        let source_files: BTreeMap<String, String> = BTreeMap::from([
            ("crates/.gitkeep".to_string(), "".to_string()),
            ("nvim/lua/local_config/.gitkeep".to_string(), "".to_string()),
            ("nvim/snippets/.gitkeep".to_string(), "".to_string()),
        ]);
        fixturify::write(&base_path, &source_files)?;

        ensure_directory_structure(&base_path, false)?;

        let result = fixturify::read(&base_path)?;
        assert_debug_snapshot!(result, @r###"
        {
            "binutils-config/local.config.lua": "return require('config')",
            "crates/.gitkeep": "",
            "nvim/lua/local_config/.gitkeep": "",
            "nvim/lua/local_config/config/autocmds.lua": "",
            "nvim/lua/local_config/config/keymaps.lua": "",
            "nvim/lua/local_config/config/options.lua": "",
            "nvim/lua/local_config/plugins/.gitkeep": "",
            "nvim/snippets/.gitkeep": "",
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_ensure_directory_structure_with_partial_structure() -> Result<()> {
        let env = setup_test_environment();
        let base_path = env.home.join("local-dotfiles");

        let source_files: BTreeMap<String, String> =
            BTreeMap::from([("crates/.gitkeep".to_string(), "".to_string())]);
        fixturify::write(&base_path, &source_files)?;

        ensure_directory_structure(&base_path, false)?;

        let result = fixturify::read(&base_path)?;
        assert_debug_snapshot!(result, @r###"
        {
            "binutils-config/local.config.lua": "return require('config')",
            "crates/.gitkeep": "",
            "nvim/lua/local_config/config/autocmds.lua": "",
            "nvim/lua/local_config/config/keymaps.lua": "",
            "nvim/lua/local_config/config/options.lua": "",
            "nvim/lua/local_config/plugins/.gitkeep": "",
            "nvim/snippets/.gitkeep": "",
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_ensure_directory_structure_preserves_existing_content() -> Result<()> {
        let env = setup_test_environment();
        let base_path = env.home.join("local-dotfiles");

        let source_files: BTreeMap<String, String> = BTreeMap::from([
            (
                "crates/existing-crate/Cargo.toml".to_string(),
                "[package]".to_string(),
            ),
            (
                "nvim/lua/local_config/init.lua".to_string(),
                "-- Config".to_string(),
            ),
        ]);
        fixturify::write(&base_path, &source_files)?;

        ensure_directory_structure(&base_path, false)?;

        let result = fixturify::read(&base_path)?;
        assert_debug_snapshot!(result, @r###"
        {
            "binutils-config/local.config.lua": "return require('config')",
            "crates/.gitkeep": "",
            "crates/existing-crate/Cargo.toml": "[package]",
            "nvim/lua/local_config/config/autocmds.lua": "",
            "nvim/lua/local_config/config/keymaps.lua": "",
            "nvim/lua/local_config/config/options.lua": "",
            "nvim/lua/local_config/init.lua": "-- Config",
            "nvim/lua/local_config/plugins/.gitkeep": "",
            "nvim/snippets/.gitkeep": "",
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_setup_symlinks_errors_on_existing_non_symlink() -> Result<()> {
        let env = setup_test_environment();
        let base_path = env.home.join("local-dotfiles");
        let local_crates_path = env.home.join("src/rwjblue/dotfiles/binutils/local-crates");

        ensure_directory_structure(&base_path, false)?;

        fs::create_dir_all(local_crates_path.join("foo-blah"))?;
        fs::write(local_crates_path.join("foo-blah/Cargo.toml"), "content")?;

        let result = setup_symlinks(&base_path, &local_crates_path, false);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Target path exists but is not a symlink: {}",
                local_crates_path.display()
            )
        );

        Ok(())
    }

    #[test]
    fn test_ensure_directory_structure_preserves_contents() -> Result<()> {
        let env = setup_test_environment();
        let base_path = env.home.join("local-dotfiles");

        let autocmds_content = r#"-- Neovim autocmds configuration"#;

        let source_files: BTreeMap<String, String> = BTreeMap::from([(
            "nvim/lua/local_config/config/autocmds.lua".to_string(),
            autocmds_content.to_string(),
        )]);
        fixturify::write(&base_path, &source_files)?;

        ensure_directory_structure(&base_path, false)?;

        let result = fixturify::read(&base_path)?;
        assert_eq!(
            result.get("nvim/lua/local_config/config/autocmds.lua"),
            Some(&autocmds_content.to_string())
        );

        Ok(())
    }
}
