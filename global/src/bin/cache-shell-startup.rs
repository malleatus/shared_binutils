use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info, trace};
use tracing_subscriber::EnvFilter;

#[derive(Debug)]
enum LineAction {
    Command { command: String, silent: bool },
    Fetch(String),
    Other(String),
}

fn parse_line(line: &str) -> LineAction {
    let trimmed_line = line.trim_start();

    if let Some(command) = trimmed_line.strip_prefix("# CMD:") {
        LineAction::Command {
            command: command.trim().to_string(),
            silent: false,
        }
    } else if let Some(command) = trimmed_line.strip_prefix("# CMD_SILENT:") {
        LineAction::Command {
            command: command.trim().to_string(),
            silent: true,
        }
    } else if let Some(url) = trimmed_line.strip_prefix("# FETCH:") {
        LineAction::Fetch(url.trim().to_string())
    } else {
        LineAction::Other(line.to_string())
    }
}

fn handle_command(command: String, silent: bool) -> Result<Vec<String>> {
    let mut result = Vec::new();

    if !silent {
        result.push(format!("# CMD: {}", &command));
    }

    trace!("Running command: {}", &command);

    let output = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .context(format!("Failed to execute command (`{}`)", &command))?;

    if output.status.success() {
        let output_str = String::from_utf8_lossy(&output.stdout);

        if silent {
            result.push(output_str.to_string());
        } else {
            result.push(format!(
                "# OUTPUT START: {}\n{}\n# OUTPUT END: {}",
                &command, output_str, &command
            ));
        }
        Ok(result)
    } else {
        let error_message = format!(
            "Failed to run command (`{}`):\n{}",
            &command,
            String::from_utf8_lossy(&output.stderr)
        );
        anyhow::bail!("{}", error_message);
    }
}

fn handle_fetch(url: String) -> Result<Vec<String>> {
    let mut result = Vec::new();
    result.push(format!("# FETCH: {}", &url));
    trace!("Fetching URL: {}", &url);

    let response = ureq::get(&url)
        .call()
        .context(format!("Failed to fetch URL: {}", &url))?;

    if response.status() == 200 {
        let content = response
            .into_body()
            .read_to_string()
            .context("Failed to read response content")?;
        result.push(format!(
            "# FETCHED CONTENT START: {}\n{}\n# FETCHED CONTENT END: {}",
            &url, content, &url
        ));
        Ok(result)
    } else {
        let status = response.status();
        let error_body = response
            .into_body()
            .read_to_string()
            .unwrap_or_else(|_| "Failed to read error response body".to_string());
        anyhow::bail!(
            "Failed to fetch URL '{}' (Status Code: {}):\nError Body: {}",
            &url,
            status,
            error_body
        );
    }
}

fn process_file<S: AsRef<Path>>(source_file: S, dest_file: S) -> Result<()> {
    let source_file = source_file.as_ref();
    let dest_file = dest_file.as_ref();

    debug!("Processing file: {}", source_file.display());

    let file = File::open(source_file).context("Failed to open file for reading")?;
    let reader = BufReader::new(file);

    let mut new_content = Vec::new();

    for line in reader.lines() {
        let line = line.context("Failed to read line")?;
        match parse_line(&line) {
            LineAction::Command { command, silent } => {
                let lines = handle_command(command, silent)?;
                new_content.extend(lines);
            }
            LineAction::Fetch(url) => {
                let lines = handle_fetch(url)?;
                new_content.extend(lines);
            }
            LineAction::Other(line) => {
                new_content.push(line);
            }
        }
    }

    if let Some(parent) = dest_file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directories for path: {:?}", parent))?;
    }

    debug!("Writing to file: {}", dest_file.display());
    trace!("New content:\n{}", new_content.join("\n"));

    let mut output_file = File::create(dest_file)
        .with_context(|| format!("Failed to open file for writing: {}", dest_file.display()))?;

    for line in new_content {
        writeln!(output_file, "{}", line).context("Failed to write line")?;
    }

    output_file.flush().context("Failed to flush file")?;

    Ok(())
}

fn process_directory(source_dir: &Path, dest_dir: &Path, final_dest_dir: &Path) -> Result<()> {
    info!("Scanning directory: {}", source_dir.display());

    for entry in fs::read_dir(source_dir)
        .with_context(|| format!("Failed to read directory: {}", source_dir.display()))?
    {
        let entry = entry.context("Failed to process directory entry")?;
        let path = entry.path();

        if path.starts_with(final_dest_dir) {
            continue;
        }

        if path.is_dir() {
            let relative_path = path
                .strip_prefix(source_dir)
                .context("Failed to get relative path")?;
            let new_dest_dir = dest_dir.join(relative_path);

            fs::create_dir_all(&new_dest_dir).context("Failed to create destination directory")?;
            process_directory(&path, &new_dest_dir, final_dest_dir)
                .context(format!("Failed to process directory {:?}", path))?;
        } else {
            let relative_path = path
                .strip_prefix(source_dir)
                .context("Failed to get relative path")?;
            let dest_file = dest_dir.join(relative_path);
            process_file(&path, &dest_file)
                .context(format!("Failed to process file {:?}", path))?;
        }
    }
    Ok(())
}

/// Represents whether to clear the destination directory
#[derive(ValueEnum, Clone, Debug, PartialEq)]
enum DestinationStrategy {
    Clear,
    Merge,
}

/// cache-shell-setup
///
/// Processes .zsh files to cache environment variables by running commands
/// and saving their output. This tool speeds up your shell startup time by
/// precomputing and caching the output of commands like `brew shellenv`.
/// It processes all `.zsh` files in a specified directory, looks for specific
/// commands (e.g., `# CMD:`), executes them, and stores their output directly
/// in the `.zsh` files, ensuring the operation is idempotent.
#[derive(Parser, Debug)]
#[command(name = "cache-shell-setup")]
struct Args {
    /// Path to the configuration file. Defaults to `~/.config/binutils/config.yaml`.
    #[arg(long)]
    config_file: Option<String>,

    /// Directory path to process
    #[clap(short, long)]
    source: Option<String>,

    /// Directory path to emit the expanded output into
    #[clap(short, long)]
    destination: Option<String>,

    /// Whether to clear the destination directory before processing
    #[clap(value_enum, long, default_value_t = DestinationStrategy::Clear)]
    destination_strategy: DestinationStrategy,
}

fn run(args: Vec<String>) -> Result<()> {
    let args = Args::parse_from(args);
    let config_file = args.config_file.as_ref().map(PathBuf::from);
    let config = config::read_config(config_file)?;

    let source_dir = if let Some(source) = args.source {
        source
    } else if let Some(shell_caching) = &config.shell_caching {
        shell_caching.source.clone()
    } else {
        anyhow::bail!(
            "No source directory provided. Either use the --source flag or set it in the config file. \nArgs: {:?} \nConfig: {:?}",
            args,
            config
        );
    };

    let destination_dir = if let Some(destination) = args.destination {
        destination
    } else if let Some(shell_caching) = &config.shell_caching {
        shell_caching.destination.clone()
    } else {
        anyhow::bail!(
            "No source directory provided. Either use the --source flag or set it in the config file"
        );
    };

    let source_dir = shellexpand::tilde(&source_dir).to_string();
    let source_dir = Path::new(&source_dir);

    let dest_dir = shellexpand::tilde(&destination_dir).to_string();
    let dest_dir = Path::new(&dest_dir);

    let temp_dest_dir = tempfile::tempdir()?;
    let temp_dest_dir = temp_dest_dir.path();

    process_directory(source_dir, temp_dest_dir, dest_dir)
        .context("Failed to process directory")?;

    if args.destination_strategy == DestinationStrategy::Clear && dest_dir.exists() {
        info!("Clearing destination directory");
        fs::remove_dir_all(dest_dir).context("Failed to clear destination directory")?;
    }

    copy_recursively(temp_dest_dir, dest_dir)?;

    Ok(())
}

fn copy_recursively(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_recursively(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), dest_path)?;
        }
    }

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

    let args: Vec<String> = std::env::args().collect();
    run(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_debug_snapshot, assert_snapshot};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;
    use std::fs::write;
    use tempfile::tempdir;
    use test_utils::setup_test_environment;

    #[test]
    fn test_parse_command() {
        assert_debug_snapshot!(parse_line("# CMD: echo hello"), @r###"
        Command {
            command: "echo hello",
            silent: false,
        }
        "###);

        assert_debug_snapshot!(parse_line("   # CMD: echo hello"), @r###"
        Command {
            command: "echo hello",
            silent: false,
        }
        "###);
    }

    #[test]
    fn test_parse_command_silent() {
        assert_debug_snapshot!(parse_line("# CMD_SILENT: echo hello"), @r###"
        Command {
            command: "echo hello",
            silent: true,
        }
        "###);

        assert_debug_snapshot!(parse_line("     # CMD_SILENT: echo hello"), @r###"
        Command {
            command: "echo hello",
            silent: true,
        }
        "###);
    }

    #[test]
    fn test_parse_fetch() {
        assert_debug_snapshot!(parse_line("# FETCH: http://example.com"), @r###"
        Fetch(
            "http://example.com",
        )
        "###);

        assert_debug_snapshot!(parse_line("    # FETCH: http://example.com"), @r###"
        Fetch(
            "http://example.com",
        )
        "###);
    }

    #[test]
    fn test_parse_other() {
        assert_debug_snapshot!(parse_line("This is a regular line"), @r###"
        Other(
            "This is a regular line",
        )
        "###);

        assert_debug_snapshot!(parse_line("      This is a regular line"), @r###"
        Other(
            "      This is a regular line",
        )
        "###);
    }

    #[test]
    fn test_process_file_with_valid_command() -> Result<()> {
        let dir = tempdir()?;
        let source_file = dir.path().join("test.zsh");
        let dest_file = dir.path().join("output.zsh");

        let content = "# CMD: echo 'hello world'\n";
        write(&source_file, content)?;

        process_file(&source_file, &dest_file)?;

        let source_contents = fs::read_to_string(&source_file)?;

        assert_snapshot!(source_contents, @r###"
        # CMD: echo 'hello world'
        "###);

        let processed_content = fs::read_to_string(&dest_file)?;
        assert_snapshot!(processed_content, @r###"
        # CMD: echo 'hello world'
        # OUTPUT START: echo 'hello world'
        hello world

        # OUTPUT END: echo 'hello world'
        "###);

        Ok(())
    }

    #[test]
    fn test_process_file_with_silent_command() -> Result<()> {
        let dir = tempdir()?;
        let source_file = dir.path().join("test.zsh");
        let dest_file = dir.path().join("output.zsh");

        let content = "# CMD_SILENT: echo 'hello world'\n";
        write(&source_file, content)?;

        process_file(&source_file, &dest_file)?;

        let source_contents = fs::read_to_string(&source_file)?;

        assert_snapshot!(source_contents, @r###"
        # CMD_SILENT: echo 'hello world'
        "###);

        let processed_content = fs::read_to_string(&dest_file)?;
        assert_snapshot!(processed_content, @r###"
        hello world

        "###);

        Ok(())
    }

    #[test]
    fn test_process_file_with_existing_output() -> Result<()> {
        let dir = tempdir()?;
        let source_file = dir.path().join("test.zsh");
        let dest_file = dir.path().join("output.zsh");

        write(&source_file, "# CMD: echo 'hello world'\n")?;
        write(
            &dest_file,
            "# CMD: echo 'hello world'\n# OUTPUT START: echo 'hello world'\nold output\n# OUTPUT END: echo 'hello world'\n",
        )?;

        process_file(&source_file, &dest_file)?;

        let source_contents = fs::read_to_string(&source_file)?;

        assert_snapshot!(source_contents, @r###"
        # CMD: echo 'hello world'
        "###);

        let processed_content = fs::read_to_string(&dest_file)?;
        assert_snapshot!(processed_content, @r###"
        # CMD: echo 'hello world'
        # OUTPUT START: echo 'hello world'
        hello world

        # OUTPUT END: echo 'hello world'
        "###);

        Ok(())
    }

    #[test]
    fn test_process_file_with_invalid_command() -> Result<()> {
        let dir = tempdir()?;
        let source_file = dir.path().join("test.zsh");
        let dest_file = dir.path().join("output.zsh");

        let content = "# CMD: invalidcommand\n";
        write(&source_file, content)?;

        // Process the file (should not panic, just print error)
        let result = process_file(&source_file, &dest_file);

        let err = result.unwrap_err();

        let error_alternate_output = format!("{:#}", err);

        // the alternate output is not stable between local and CI, so we can't use snapshots here
        assert!(
            error_alternate_output.contains("Failed to run command (`invalidcommand`)"),
            "Error output: {}",
            error_alternate_output
        );
        assert!(
            error_alternate_output.contains("not found"),
            "Error output: {}",
            error_alternate_output
        );

        Ok(())
    }

    #[test]
    fn test_process_directory() {
        let temp_dir = tempdir().unwrap();
        let base_dir = temp_dir.path();

        let source_files: BTreeMap<String, String> = BTreeMap::from([
            (
                "zsh/zshrc".to_string(),
                "# CMD: echo 'hello world'\n".to_string(),
            ),
            (
                "zsh/plugins/thing.zsh".to_string(),
                "# CMD: echo 'goodbye world'\n".to_string(),
            ),
        ]);

        fixturify::write(base_dir, &source_files).unwrap();

        let source_dir = base_dir.join("zsh");
        let dest_dir = base_dir.join("zsh/dist");

        process_directory(&source_dir, &dest_dir, &dest_dir).unwrap();

        let file_map = fixturify::read(base_dir).unwrap();

        assert_debug_snapshot!(file_map, @r###"
        {
            "zsh/dist/plugins/thing.zsh": "# CMD: echo 'goodbye world'\n# OUTPUT START: echo 'goodbye world'\ngoodbye world\n\n# OUTPUT END: echo 'goodbye world'\n",
            "zsh/dist/zshrc": "# CMD: echo 'hello world'\n# OUTPUT START: echo 'hello world'\nhello world\n\n# OUTPUT END: echo 'hello world'\n",
            "zsh/plugins/thing.zsh": "# CMD: echo 'goodbye world'\n",
            "zsh/zshrc": "# CMD: echo 'hello world'\n",
        }
        "###)
    }

    #[test]
    fn test_run_with_args() {
        let env = setup_test_environment();

        let source_files: BTreeMap<String, String> = BTreeMap::from([
            (
                "src/rwjblue/dotfiles/zsh/zshrc".to_string(),
                "# CMD: echo 'hello world'\n".to_string(),
            ),
            (
                "src/rwjblue/dotfiles/zsh/plugins/thing.zsh".to_string(),
                "# CMD: echo 'goodbye world'\n".to_string(),
            ),
            (
                "src/rwjblue/dotfiles/zsh/dist/plugins/thing.zsh".to_string(),
                "# CMD: echo 'goodbye world'\n# OLD OUTPUT SHOULD BE DELETED".to_string(),
            ),
        ]);

        fixturify::write(&env.home, &source_files).unwrap();

        run(vec![
            "cache-shell-setup".to_string(),
            "--source=~/src/rwjblue/dotfiles/zsh".to_string(),
            "--destination=~/src/rwjblue/dotfiles/zsh/dist".to_string(),
        ])
        .unwrap();

        let file_map = fixturify::read(&env.home).unwrap();

        assert_debug_snapshot!(file_map, @r###"
        {
            "src/rwjblue/dotfiles/zsh/dist/plugins/thing.zsh": "# CMD: echo 'goodbye world'\n# OUTPUT START: echo 'goodbye world'\ngoodbye world\n\n# OUTPUT END: echo 'goodbye world'\n",
            "src/rwjblue/dotfiles/zsh/dist/zshrc": "# CMD: echo 'hello world'\n# OUTPUT START: echo 'hello world'\nhello world\n\n# OUTPUT END: echo 'hello world'\n",
            "src/rwjblue/dotfiles/zsh/plugins/thing.zsh": "# CMD: echo 'goodbye world'\n",
            "src/rwjblue/dotfiles/zsh/zshrc": "# CMD: echo 'hello world'\n",
        }
        "###)
    }

    #[test]
    fn test_run_with_command_failures() {
        let env = setup_test_environment();

        let source_files: BTreeMap<String, String> = BTreeMap::from([
            (
                "src/rwjblue/dotfiles/zsh/zshrc".to_string(),
                "# CMD: zomg-wtf-bbq\n".to_string(),
            ),
            (
                "src/rwjblue/dotfiles/zsh/plugins/thing.zsh".to_string(),
                "# CMD: echo 'goodbye world'\n".to_string(),
            ),
            (
                "src/rwjblue/dotfiles/zsh/dist/zshrc".to_string(),
                "# original contents; before running caching".to_string(),
            ),
            (
                "src/rwjblue/dotfiles/zsh/dist/plugins/thing.zsh".to_string(),
                "# original contents; before running caching".to_string(),
            ),
        ]);

        fixturify::write(&env.home, &source_files).unwrap();

        let result = run(vec![
            "cache-shell-setup".to_string(),
            "--source=~/src/rwjblue/dotfiles/zsh".to_string(),
            "--destination=~/src/rwjblue/dotfiles/zsh/dist".to_string(),
        ]);

        let err = result.unwrap_err();
        let replace_home_dir = |content: String| -> String {
            content.replace(&env.home.to_string_lossy().to_string(), "~")
        };
        let err_output = replace_home_dir(format!("{:?}", err));
        #[cfg(target_os = "macos")]
        {
            assert_snapshot!(err_output, @r###"
            Failed to process directory

            Caused by:
                0: Failed to process file "~/src/rwjblue/dotfiles/zsh/zshrc"
                1: Failed to run command (`zomg-wtf-bbq`):
                   sh: zomg-wtf-bbq: command not found
                   
            "###);
        }

        #[cfg(target_os = "linux")]
        {
            assert!(err_output.contains("zomg-wtf-bbq"));
            assert!(err_output.contains("not found"));
        }

        let file_map = fixturify::read(&env.home).unwrap();

        assert_eq!(file_map, source_files);
    }

    #[test]
    fn test_run_with_config() {
        let env = setup_test_environment();

        let config_path = &env.config_file;
        fs::write(
            config_path,
            r###"return { shell_caching = { source = "~/other-path/zsh", destination = "~/other-path/zsh/dist" } }"###,
        )
        .expect("Could not write to config file");

        let source_files: BTreeMap<String, String> = BTreeMap::from([
            (
                "other-path/zsh/zshrc".to_string(),
                "# CMD: echo 'hello world'\n".to_string(),
            ),
            (
                "other-path/zsh/plugins/thing.zsh".to_string(),
                "# CMD: echo 'goodbye world'\n".to_string(),
            ),
            (
                "other-path/zsh/dist/plugins/thing.zsh".to_string(),
                "# CMD: echo 'goodbye world'\n# OLD OUTPUT SHOULD BE DELETED".to_string(),
            ),
        ]);

        fixturify::write(&env.home, &source_files).unwrap();

        run(vec![]).unwrap();

        let file_map = fixturify::read(&env.home).unwrap();

        assert_debug_snapshot!(file_map, @r###"
        {
            ".config/binutils/config.lua": "return { shell_caching = { source = \"~/other-path/zsh\", destination = \"~/other-path/zsh/dist\" } }",
            "other-path/zsh/dist/plugins/thing.zsh": "# CMD: echo 'goodbye world'\n# OUTPUT START: echo 'goodbye world'\ngoodbye world\n\n# OUTPUT END: echo 'goodbye world'\n",
            "other-path/zsh/dist/zshrc": "# CMD: echo 'hello world'\n# OUTPUT START: echo 'hello world'\nhello world\n\n# OUTPUT END: echo 'hello world'\n",
            "other-path/zsh/plugins/thing.zsh": "# CMD: echo 'goodbye world'\n",
            "other-path/zsh/zshrc": "# CMD: echo 'hello world'\n",
        }
        "###)
    }

    #[test]
    fn test_run_with_merging() {
        let env = setup_test_environment();

        let source_files: BTreeMap<String, String> = BTreeMap::from([
            (
                "src/rwjblue/dotfiles/zsh/zshrc".to_string(),
                "# CMD: echo 'hello world'\n".to_string(),
            ),
            (
                "src/rwjblue/dotfiles/zsh/plugins/thing.zsh".to_string(),
                "# CMD: echo 'goodbye world'\n".to_string(),
            ),
            (
                "src/rwjblue/dotfiles/zsh/dist/plugins/thing.zsh".to_string(),
                "# CMD: echo 'goodbye world'\n# OLD OUTPUT SHOULD BE DELETED".to_string(),
            ),
            (
                "src/rwjblue/dotfiles/zsh/dist/plugins/weird-other-thing.zsh".to_string(),
                "# HAHAHA WTF IS THIS?!?! DO NOT WORRY ABOUT".to_string(),
            ),
        ]);

        fixturify::write(&env.home, &source_files).unwrap();

        run(vec![
            "cache-shell-setup".to_string(),
            "--source=~/src/rwjblue/dotfiles/zsh".to_string(),
            "--destination=~/src/rwjblue/dotfiles/zsh/dist".to_string(),
            "--destination-strategy=merge".into(),
        ])
        .unwrap();

        let file_map = fixturify::read(&env.home).unwrap();

        assert_debug_snapshot!(file_map, @r###"
        {
            "src/rwjblue/dotfiles/zsh/dist/plugins/thing.zsh": "# CMD: echo 'goodbye world'\n# OUTPUT START: echo 'goodbye world'\ngoodbye world\n\n# OUTPUT END: echo 'goodbye world'\n",
            "src/rwjblue/dotfiles/zsh/dist/plugins/weird-other-thing.zsh": "# HAHAHA WTF IS THIS?!?! DO NOT WORRY ABOUT",
            "src/rwjblue/dotfiles/zsh/dist/zshrc": "# CMD: echo 'hello world'\n# OUTPUT START: echo 'hello world'\nhello world\n\n# OUTPUT END: echo 'hello world'\n",
            "src/rwjblue/dotfiles/zsh/plugins/thing.zsh": "# CMD: echo 'goodbye world'\n",
            "src/rwjblue/dotfiles/zsh/zshrc": "# CMD: echo 'hello world'\n",
        }
        "###)
    }

    #[test]
    fn test_run_with_nonexistent_destination() {
        let env = setup_test_environment();

        let source_files: BTreeMap<String, String> = BTreeMap::from([(
            "src/rwjblue/dotfiles/zsh/zshrc".to_string(),
            "# CMD: echo 'hello world'\n".to_string(),
        )]);

        fixturify::write(&env.home, &source_files).unwrap();

        run(vec![
            "cache-shell-setup".to_string(),
            "--source=~/src/rwjblue/dotfiles/zsh".to_string(),
            "--destination=~/src/rwjblue/dotfiles/nonexistent/dist".to_string(),
        ])
        .unwrap();

        let file_map = fixturify::read(&env.home).unwrap();

        assert_debug_snapshot!(file_map, @r###"
        {
            "src/rwjblue/dotfiles/nonexistent/dist/zshrc": "# CMD: echo 'hello world'\n# OUTPUT START: echo 'hello world'\nhello world\n\n# OUTPUT END: echo 'hello world'\n",
            "src/rwjblue/dotfiles/zsh/zshrc": "# CMD: echo 'hello world'\n",
        }
        "###);
    }

    #[test]
    fn test_process_file_with_fetch() -> Result<()> {
        let mut server = mockito::Server::new();

        let server_url = server.url();

        let mock = server
            .mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("# some content returned here!!")
            .create();

        let dir = tempdir()?;
        let source_file = dir.path().join("test.zsh");
        let dest_file = dir.path().join("output.zsh");

        let content = format!("# FETCH: {}/test", server_url);
        write(&source_file, content)?;

        process_file(&source_file, &dest_file)?;

        mock.assert();

        let replace_server_addr = |content: &str, server_url: &str| -> String {
            content.replace(server_url, "{server_url}")
        };

        let source_contents = fs::read_to_string(&source_file)?;
        assert_snapshot!(replace_server_addr(&source_contents, &server_url), @r###"
        # FETCH: {server_url}/test
        "###);

        let processed_content = fs::read_to_string(&dest_file)?;
        assert_snapshot!(replace_server_addr(&processed_content, &server_url), @r###"
        # FETCH: {server_url}/test
        # FETCHED CONTENT START: {server_url}/test
        # some content returned here!!
        # FETCHED CONTENT END: {server_url}/test
        "###);

        Ok(())
    }

    #[test]
    fn test_process_file_with_invalid_fetch() -> Result<()> {
        let mut server = mockito::Server::new();

        let server_url = server.url();

        let mock = server
            .mock("GET", "/test")
            .with_status(500)
            .with_header("content-type", "text/plain")
            .with_body("ZOMG ERROR")
            .create();

        let dir = tempdir()?;
        let source_file = dir.path().join("test.zsh");
        let dest_file = dir.path().join("output.zsh");

        let content = format!("# FETCH: {}/test", server_url);
        write(&source_file, content)?;

        let replace_server_addr = |content: String, server_url: &str| -> String {
            content.replace(server_url, "{server_url}")
        };

        let result = process_file(&source_file, &dest_file);

        let err = result.unwrap_err();
        assert_snapshot!(replace_server_addr(format!("{:#}", err), &server_url), @"Failed to fetch URL: {server_url}/test: http status: 500");

        mock.assert();

        Ok(())
    }
}

// TODO: Add support to handle race conditions: currently sheldon source reads the files in
// zsh/dist *but* we also have `# CMD: sheldon source` (which reads those files)
// Try adding "passes" so you can `# CMD(1): sheldon source` (where the default is "pass 0")
// and each pass would get flushed to disk together -- this does make a z-index war kinda thing
// but in practice who cares?
