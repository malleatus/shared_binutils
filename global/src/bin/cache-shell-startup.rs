use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Command;
use tracing::{error, trace};
use ureq;

fn main() -> Result<()> {
    let source_file = std::env::args().nth(1).context("Missing source file argument")?;
    let dest_file = std::env::args().nth(2).context("Missing destination file argument")?;

    let source_file = Path::new(&source_file);
    let dest_file = Path::new(&dest_file);

    let file = File::open(source_file).context("Failed to open source file")?;
    let reader = BufReader::new(file);

    let mut new_content = Vec::new();

    for line in reader.lines() {
        let line = line.context("Failed to read line")?;

        if let Some(command) = line.strip_prefix("# CMD:") {
            let trimmed_command = command.trim();

            new_content.push(format!("# CMD: {}", trimmed_command));

            trace!("Running command: {}", trimmed_command);

            let output = Command::new("sh")
                .arg("-c")
                .arg(trimmed_command)
                .output()
                .context("Failed to execute command")?;

            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                new_content.push(format!(
                    "# OUTPUT START: {}\n{}\n# OUTPUT END: {}",
                    trimmed_command, output_str, trimmed_command
                ));
            } else {
                error!(
                    "Failed to run command '{}':\n {}",
                    trimmed_command,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        } else if let Some(url) = line.strip_prefix("# FETCH:") {
            let trimmed_url = url.trim();

            new_content.push(format!("# FETCH: {}", trimmed_url));

            trace!("Fetching URL: {}", trimmed_url);

            let response = ureq::get(trimmed_url).call().context("Failed to fetch URL")?;

            if response.status() == 200 {
                let content = response.into_string().context("Failed to read response content")?;
                new_content.push(format!(
                    "# FETCHED CONTENT START: {}\n{}\n# FETCHED CONTENT END: {}",
                    trimmed_url, content, trimmed_url
                ));
            } else {
                error!(
                    "Failed to fetch URL '{}': {}",
                    trimmed_url,
                    response.status_text()
                );
            }
        } else {
            new_content.push(line);
        }
    }

    if let Some(parent) = dest_file.parent() {
        std::fs::create_dir_all(parent).context("Failed to create destination directory")?;
    }

    let mut dest_file = File::create(dest_file).context("Failed to create destination file")?;
    for line in new_content {
        writeln!(dest_file, "{}", line).context("Failed to write to destination file")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cmd_inline() -> Result<()> {
        let temp_dir = tempdir()?;
        let source_file = temp_dir.path().join("source.txt");
        let dest_file = temp_dir.path().join("dest.txt");

        std::fs::write(&source_file, "# CMD: echo Hello, world!")?;

        main_with_args(source_file.to_str().unwrap(), dest_file.to_str().unwrap())?;

        let result = std::fs::read_to_string(dest_file)?;
        assert!(result.contains("# OUTPUT START: echo Hello, world!"));
        assert!(result.contains("Hello, world!"));
        assert!(result.contains("# OUTPUT END: echo Hello, world!"));

        Ok(())
    }

    #[test]
    fn test_fetch_inline() -> Result<()> {
        let temp_dir = tempdir()?;
        let source_file = temp_dir.path().join("source.txt");
        let dest_file = temp_dir.path().join("dest.txt");

        std::fs::write(&source_file, "# FETCH: https://httpbin.org/get")?;

        main_with_args(source_file.to_str().unwrap(), dest_file.to_str().unwrap())?;

        let result = std::fs::read_to_string(dest_file)?;
        assert!(result.contains("# FETCHED CONTENT START: https://httpbin.org/get"));
        assert!(result.contains("\"url\": \"https://httpbin.org/get\""));
        assert!(result.contains("# FETCHED CONTENT END: https://httpbin.org/get"));

        Ok(())
    }

    fn main_with_args(source_file: &str, dest_file: &str) -> Result<()> {
        let args = vec![
            "cache-shell-startup".to_string(),
            source_file.to_string(),
            dest_file.to_string(),
        ];
        std::env::set_var("RUST_BACKTRACE", "1");
        std::env::set_var("RUST_LOG", "trace");
        std::env::set_args(args);
        main()
    }
}
