use std::{cell::RefCell, path::PathBuf};

thread_local! {
    pub static CMD_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

#[macro_export]
/// Sets the cwd for invocations of sh! within the given block.
/// Prior cwd is restored when exiting the block.
///
/// All uses of sh! share the same thread-local
macro_rules! in_dir {
    ($dir:expr_2021, $block:block) => {{
        use anyhow::Result;

        CMD_DIR.with(|current_dir| {
            let previous_dir = current_dir.replace(Some(PathBuf::from($dir)));
            let result = (|| -> Result<_> { $block })();
            current_dir.replace(previous_dir);
            result
        })
    }};
}

#[macro_export]
/// Run the shell command `$cmd` using zsh.
/// Return `Ok(stdout)` on success.
/// If the command cannot be invoked, return an error that includes the command string.
/// If the command exits non-zero, return an error with the comamnd string, stdout and stderr.
///
/// Not the least bit optimized for performance.
macro_rules! sh {
    ($cmd:expr_2021) => {{
        use anyhow::Context;

        CMD_DIR.with(|cmd_dir| {
            use std::process::Command;

            let mut command = Command::new("zsh");
            command.args(vec!["-c", $cmd]);

            if let Some(cwd) = cmd_dir.borrow().as_ref() {
                command.current_dir(cwd);
            }

            let output = command
                .output()
                .context(format!("Running command: {:?}", command))?;

            if !output.status.success() {
                let stdout = String::from_utf8(output.stdout)?.trim().to_string();
                let stderr = String::from_utf8(output.stderr)?.trim().to_string();
                return Err(anyhow::anyhow!(format!(
                    "cmd: {:?}\n\nout:\n\n{}\n\nerr:\n\n{}",
                    command, stdout, stderr
                )));
            }
            Ok(String::from_utf8(output.stdout)?.trim().to_string())
        })
    }};
}

#[macro_export]
///Run the shell command `$cmd` using zsh.
///Return Ok(success), i.e. true if the command exists 0 and 1 if the command exists non-zero.
///
///Returns an error if the command failed to run.
macro_rules! sh_q {
    ($cmd:expr_2021) => {{
        use anyhow::Context;

        CMD_DIR.with(|cmd_dir| {
            use std::process::Command;

            let mut command = Command::new("zsh");
            command.args(vec!["-c", $cmd]);

            if let Some(cwd) = cmd_dir.borrow().as_ref() {
                command.current_dir(cwd);
            }

            let output = command
                .output()
                .context(format!("Running command: {:?}", command))?;

            Ok::<bool, anyhow::Error>(output.status.success())
        })
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn test_in_dir_and_sh_macros() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;

        in_dir!(temp_dir.path(), {
            sh!("touch foo.md bar.md baz.md")?;

            let output = sh!("ls")?;

            println!("{}", output);

            assert!(output.contains("foo.md"));
            assert!(output.contains("bar.md"));
            assert!(output.contains("baz.md"));

            Ok(())
        })?;

        Ok(())
    }

    #[test]
    fn test_stderr_in_errors() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;

        in_dir!(temp_dir.path(), {
            sh!("touch foo.md")?;

            let result = sh!("ls foo.md bar.md baz.md");

            assert!(result.is_err());

            if let Err(err) = result {
                let error_message = format!("{}", err);
                assert!(error_message.contains("bar.md"));
                assert!(error_message.contains("baz.md"));
            }

            Ok(())
        })?;

        Ok(())
    }
}
