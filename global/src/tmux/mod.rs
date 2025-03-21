use anyhow::Result;
use std::os::unix::process::CommandExt;
use std::{collections::BTreeMap, path::PathBuf, process::Command};
use tracing::{debug, trace};

use config::{Config, Window, gather_crate_locations};

/// `TmuxOptions` is a trait for managing various options for working with these tmux utilities.
///
/// It provides methods to check if the current run is a dry run, if it's in debug mode, and if it should attach to tmux.
pub trait TmuxOptions {
    /// Checks if the current run is a dry run.
    ///
    /// In a dry run, commands are not actually executed. Instead, they are just printed to the console.
    /// This is useful for testing and debugging.
    fn is_dry_run(&self) -> bool;

    /// Checks if the current run is in debug mode.
    fn is_debug(&self) -> bool;

    /// Provides the socket name of the tmux server to attach to.
    ///
    /// If this returns `None`, the default socket name will be used.
    fn socket_name(&self) -> Option<String>;

    /// Decides if we should attach to tmux in the running terminal.
    ///
    /// Returns `Some(true)` if we should attach, `Some(false)` if we should not, and `None` if the decision is left to the default behavior.
    fn should_attach(&self) -> Option<bool>;

    fn config_file(&self) -> Option<PathBuf>;

    fn _is_testing(&self) -> bool {
        false
    }
}

pub fn in_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

fn get_socket_name(options: &impl TmuxOptions) -> String {
    options
        .socket_name()
        .unwrap_or_else(|| "default".to_string())
}

pub fn startup_tmux(config: &Config, options: &impl TmuxOptions) -> Result<Vec<String>> {
    let mut current_state = gather_tmux_state(options);
    let mut commands = vec![];

    let crates = gather_crate_locations(config)?;

    match &config.tmux {
        Some(tmux) => {
            for session in &tmux.sessions {
                for (index, window) in session.windows.iter().enumerate() {
                    let commands_executed = ensure_window(
                        &session.name,
                        window,
                        &index,
                        &crates,
                        &mut current_state,
                        options,
                    )?;

                    for command in commands_executed {
                        commands.push(generate_debug_string_for_command(&command));
                    }
                }
            }

            if let Some(attach_command) = maybe_attach_tmux(config, options)? {
                // NOTE: this only runs for `--dry-run` or `--attach=false` cases
                commands.push(generate_debug_string_for_command(&attach_command));
            }
        }
        None => {
            debug!("No tmux configuration found, skipping tmux setup");
        }
    }

    Ok(commands)
}

fn maybe_attach_tmux(config: &Config, options: &impl TmuxOptions) -> Result<Option<Command>> {
    let should_attach = options.should_attach().unwrap_or_else(|| {
        trace!("`--attach` was not explicitly specified, checking $TMUX");

        !in_tmux()
    });

    if !should_attach {
        trace!("Not attaching to tmux session: options.should_attach() returned false");
        return Ok(None);
    }

    let mut cmd = Command::new("tmux");
    cmd.arg("attach");
    if let Some(tmux) = &config.tmux {
        if let Some(default_session) = &tmux.default_session {
            cmd.arg("-t").arg(default_session);
        }
    }

    let should_attach = !options.is_dry_run() && !options._is_testing();

    if should_attach {
        let result = cmd.exec();
        // SAFETY: We should never actually hit this line, as exec should replace the current process
        anyhow::bail!("Failed to execute tmux attach command: {:?}", result)
    } else {
        trace!(
            "Not attaching! Dry run: {} -- Testing: {}",
            options.is_dry_run(),
            options._is_testing()
        );
        Ok(Some(cmd))
    }
}

fn determine_commands_for_window(
    window: &Window,
    crates: &BTreeMap<String, PathBuf>,
) -> Result<Option<Vec<String>>> {
    let mut commands: Vec<String> = vec![];

    if let Some(linked_crates) = &window.linked_crates {
        for linked_crate in linked_crates {
            if let Some(crate_path) = crates.get(linked_crate) {
                let new_command = format!(
                    r#"export PATH="{}:$PATH""#,
                    crate_path.to_str().unwrap_or_default()
                );
                commands.push(new_command);
            } else {
                anyhow::bail!(
                    "Could not find crate: {} for linking into {}",
                    linked_crate,
                    window.name
                );
            }
        }
    }

    if let Some(command) = &window.command {
        match command {
            config::Command::Single(cmd) => commands.push(cmd.clone()),
            config::Command::Multiple(cmds) => commands.extend(cmds.clone()),
        };
    }

    if commands.is_empty() {
        Ok(None)
    } else {
        Ok(Some(commands))
    }
}

fn compare_presumed_vs_actual_state(current_state: &mut TmuxState, options: &impl TmuxOptions) {
    if options._is_testing() || tracing::level_enabled!(tracing::Level::TRACE) {
        let actual_state = gather_tmux_state(options);

        if *current_state != actual_state {
            let message = format!(
                "State difference - Current (presumed): {:#?}, Actual: {:#?}",
                current_state, actual_state
            );

            trace!("{}", message);

            if options._is_testing() {
                // NOTE: make the tests fail if our expected internal representation doesn't match reality
                panic!("{}", message);
            }
        }
    }
}

fn ensure_window(
    session_name: &str,
    window: &Window,
    window_index: &usize,
    crates: &BTreeMap<String, PathBuf>,
    current_state: &mut TmuxState,
    options: &impl TmuxOptions,
) -> Result<Vec<Command>> {
    let socket_name = get_socket_name(options);
    let mut commands_executed = vec![];

    compare_presumed_vs_actual_state(current_state, options);

    if let Some(windows) = current_state.get_mut(session_name) {
        if windows.contains(&window.name) {
            trace!(
                "Window {} already exists in session {}, skipping creation",
                window.name, session_name
            );
        } else {
            trace!(
                "Window {} does not exist in session {}, creating it",
                window.name, session_name
            );

            let base_index = 1;
            let target_index = base_index + window_index;

            let mut cmd = Command::new("tmux");
            cmd.arg("-L")
                .arg(&socket_name)
                .arg("new-window")
                .arg("-t")
                .arg(format!("{}:{}", session_name, target_index))
                .arg("-n")
                .arg(&window.name)
                // insert *before* any existing window at the specified index
                .arg("-b");

            if let Some(path) = &window.path {
                cmd.arg("-c").arg(path);
            }

            if let Some(env) = &window.env {
                for (key, value) in env {
                    cmd.arg("-e").arg(format!("{}={}", key, value));
                }
            }

            commands_executed.push(run_command(cmd, options)?);
            commands_executed.extend(execute_command(session_name, window, crates, options)?);

            trace!(
                "Attempting to insert window '{}' at index {} into vector of length {}",
                window.name,
                *window_index,
                windows.len()
            );
            windows.insert(*window_index, window.name.to_string());
        }
    } else {
        trace!(
            "Session {} does not exist, creating it and window {}",
            session_name, window.name
        );

        let mut cmd = Command::new("tmux");
        cmd.arg("-L")
            .arg(&socket_name)
            .arg("new-session")
            .arg("-d")
            .arg("-s")
            .arg(session_name)
            .arg("-n")
            .arg(&window.name);

        if let Some(path) = &window.path {
            cmd.arg("-c").arg(path);
        }

        if let Some(env) = &window.env {
            for (key, value) in env {
                cmd.arg("-e").arg(format!("{}={}", key, value));
            }
        }

        // push the session / window creation command
        commands_executed.push(run_command(cmd, options)?);

        // push any commands referenced in the config for the window
        commands_executed.extend(execute_command(session_name, window, crates, options)?);

        current_state.insert(session_name.to_string(), vec![window.name.to_string()]);
    }

    compare_presumed_vs_actual_state(current_state, options);

    Ok(commands_executed)
}

fn execute_command(
    session_name: &str,
    window: &Window,
    crates: &BTreeMap<String, PathBuf>,
    options: &impl TmuxOptions,
) -> Result<Vec<Command>> {
    let mut commands_executed = vec![];
    let commands_to_execute = determine_commands_for_window(window, crates)?;

    match commands_to_execute {
        None => {}
        Some(commands) => {
            for command in commands {
                let mut cmd = Command::new("tmux");
                cmd.arg("-L")
                    .arg(get_socket_name(options))
                    .arg("send-keys")
                    .arg("-t")
                    .arg(format!("{}:{}", session_name, window.name))
                    .arg(command)
                    .arg("Enter");
                let cmd = run_command(cmd, options)?;
                commands_executed.push(cmd);
            }
        }
    }

    Ok(commands_executed)
}

type TmuxState = BTreeMap<String, Vec<String>>;

/// Runs `tmux list-sessions -F #{session_name}` to gather sessions, then for each session
/// runs `tmux list-windows -F #{window_name}` and returns a HashMap where the keys are session
/// names and the values is an array of the window names.
fn gather_tmux_state(options: &impl TmuxOptions) -> TmuxState {
    let mut state = BTreeMap::new();

    let socket_name = get_socket_name(options);

    let sessions_output = Command::new("tmux")
        .arg("-L")
        .arg(&socket_name)
        .arg("list-sessions")
        .arg("-F")
        .arg("#{session_name}")
        .output()
        .expect("Failed to execute command");

    let sessions = String::from_utf8(sessions_output.stdout).unwrap();
    for session in sessions.lines() {
        let windows_output = Command::new("tmux")
            .arg("-L")
            .arg(&socket_name)
            .arg("list-windows")
            .arg("-F")
            .arg("#{window_name}")
            .arg("-t")
            .arg(session)
            .output()
            .expect("Failed to execute command");

        let windows = String::from_utf8(windows_output.stdout).unwrap();
        state.insert(
            session.to_string(),
            windows
                .lines()
                .map(|s| s.to_string())
                .collect::<Vec<String>>(),
        );
    }

    state
}

fn run_command(mut cmd: Command, opts: &impl TmuxOptions) -> Result<Command> {
    trace!("Running: {}", generate_debug_string_for_command(&cmd));

    if !opts.is_dry_run() {
        match cmd.output() {
            Ok(output) => {
                if !output.status.success() {
                    // TODO: should we bail always or only in testing?
                    anyhow::bail!(
                        "Command execution failed (exit code: {}): {:?}",
                        output.status,
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            Err(e) => {
                tracing::error!("Failed to execute command: {}", e);
                return Err(e.into());
            }
        }
    }

    Ok(cmd)
}

/// Generates a debug string representation of a `Command`.
fn generate_debug_string_for_command(cmd: &Command) -> String {
    let mut cmd_string = String::new();

    if let Some(program) = cmd.get_program().to_str() {
        cmd_string.push_str(program);
    }

    for arg in cmd.get_args() {
        if let Some(arg_str) = arg.to_str() {
            cmd_string.push(' ');

            if arg_str.contains(' ') || arg_str.contains('"') {
                cmd_string.push('\'');
                for c in arg_str.chars() {
                    if c == '\'' {
                        cmd_string.push('\\');
                    }
                    cmd_string.push(c);
                }
                cmd_string.push('\'');
            } else if arg_str.contains('\'') {
                cmd_string.push('"');
                for c in arg_str.chars() {
                    if c == '"' {
                        cmd_string.push('\\');
                    }
                    cmd_string.push(c);
                }
                cmd_string.push('"');
            } else {
                cmd_string.push_str(arg_str);
            }
        }
    }

    cmd_string
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        env, fs,
        path::Path,
        thread::sleep,
        time::{Duration, Instant},
    };

    use anyhow::Result;
    use config::Command as ConfigCommand;
    use insta::assert_debug_snapshot;
    use rand::{Rng, distr::Alphanumeric};
    use tempfile::tempdir;
    use test_utils::{FakeBin, FakePackage, create_workspace_with_packages, setup_tracing};

    use crate::build_utils::generate_symlinks;

    use super::*;
    use config::{Session, Tmux, Window};

    struct TestingTmuxOptions {
        dry_run: bool,
        debug: bool,
        attach: Option<bool>,
        socket_name: String,
        _config_file: Option<PathBuf>,
    }

    impl TmuxOptions for TestingTmuxOptions {
        fn is_dry_run(&self) -> bool {
            self.dry_run
        }

        fn is_debug(&self) -> bool {
            self.debug
        }

        fn should_attach(&self) -> Option<bool> {
            self.attach
        }

        fn socket_name(&self) -> Option<String> {
            Some(self.socket_name.clone())
        }

        fn config_file(&self) -> Option<PathBuf> {
            None
        }

        fn _is_testing(&self) -> bool {
            true
        }
    }

    impl Drop for TestingTmuxOptions {
        fn drop(&mut self) {
            kill_tmux_server(self).unwrap();
        }
    }

    fn generate_socket_name() -> String {
        let rng = rand::rng();
        let socket_name: String = rng
            .sample_iter(&Alphanumeric)
            .take(30)
            .map(char::from)
            .collect();
        socket_name
    }

    fn create_tmux_session(
        session_name: &str,
        window_name: &str,
        options: &impl TmuxOptions,
    ) -> Result<(), std::io::Error> {
        let socket_name = get_socket_name(options);

        // Create the session with the initial window
        let _ = Command::new("tmux")
            .arg("-L")
            .arg(socket_name)
            .arg("new-session")
            .arg("-d")
            .arg("-s")
            .arg(session_name)
            .arg("-n")
            .arg(window_name)
            .status()?;

        assert!(tmux_server_running(options));

        Ok(())
    }

    fn kill_tmux_server(options: &impl TmuxOptions) -> Result<()> {
        let socket_name = get_socket_name(options);

        let _ = Command::new("tmux")
            .arg("-L")
            .arg(socket_name)
            .arg("kill-server")
            .status()?;

        Ok(())
    }

    fn tmux_server_running(options: &impl TmuxOptions) -> bool {
        let socket_name = get_socket_name(options);

        Command::new("tmux")
            .arg("-L")
            .arg(socket_name)
            .arg("has-session")
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn build_testing_options() -> TestingTmuxOptions {
        // Make tests stable regardless of if we are within a TMUX session or not
        unsafe { env::remove_var("TMUX") }

        let options = TestingTmuxOptions {
            dry_run: false,
            debug: false,
            attach: None,
            socket_name: generate_socket_name(),
            _config_file: None,
        };

        assert!(
            !tmux_server_running(&options),
            "precond - tmux server should not be running on randomized socket name"
        );

        setup_tracing();

        options
    }

    fn sanitize_commands_executed(
        commands: Vec<String>,
        options: &TestingTmuxOptions,
        additional_replacements: Option<HashMap<String, String>>,
    ) -> Vec<String> {
        commands
            .iter()
            .map(|command| {
                let mut updated_command = command.replace(&options.socket_name, "[SOCKET_NAME]");
                if let Some(replacements) = &additional_replacements {
                    for (key, value) in replacements {
                        updated_command = updated_command.replace(key, value);
                    }
                }
                updated_command
            })
            .collect()
    }

    fn wait_for_file(path: &Path, message: String) {
        let start = Instant::now();
        let timeout = Duration::from_secs(2);

        loop {
            // Check if the file exists
            if path.exists() {
                break;
            }

            // Sleep for 10ms
            sleep(Duration::from_millis(10));

            // Check if the elapsed time is greater than the timeout
            if start.elapsed() > timeout {
                panic!("{}", message);
            }
        }
    }

    #[test]
    fn test_in_tmux_within_tmux() {
        temp_env::with_var(
            "TMUX",
            Some("/private/tmp/tmux-23547/default,39774,0"),
            || {
                assert!(in_tmux());
            },
        );
    }

    #[test]
    fn test_in_tmux_outside_tmux() {
        temp_env::with_var("TMUX", None::<String>, || {
            assert!(!in_tmux());
        });
    }

    #[test]
    fn test_gather_tmux_state() -> Result<()> {
        let options = build_testing_options();

        create_tmux_session("foo", "bar", &options)?;

        assert_debug_snapshot!(gather_tmux_state(&options), @r###"
        {
            "foo": [
                "bar",
            ],
        }
        "###);

        create_tmux_session("baz", "qux", &options)?;

        assert_debug_snapshot!(gather_tmux_state(&options), @r###"
        {
            "baz": [
                "qux",
            ],
            "foo": [
                "bar",
            ],
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_creates_all_windows_when_server_is_not_started() -> Result<()> {
        let options = build_testing_options();
        let config = Config {
            crate_locations: None,
            shell_caching: None,
            tmux: Some(Tmux {
                default_session: None,
                sessions: vec![Session {
                    name: "foo".to_string(),
                    windows: vec![
                        Window {
                            name: "bar".to_string(),
                            path: None,
                            command: None,
                            env: None,
                            linked_crates: None,
                        },
                        Window {
                            name: "baz".to_string(),
                            path: None,
                            command: None,
                            env: None,
                            linked_crates: None,
                        },
                        Window {
                            name: "qux".to_string(),
                            path: None,
                            command: None,
                            env: None,
                            linked_crates: None,
                        },
                        Window {
                            name: "derp".to_string(),
                            path: None,
                            command: None,
                            env: None,
                            linked_crates: None,
                        },
                    ],
                }],
            }),
        };
        let commands = startup_tmux(&config, &options)?;
        let commands = sanitize_commands_executed(commands, &options, None);

        assert_debug_snapshot!(commands, @r###"
        [
            "tmux -L [SOCKET_NAME] new-session -d -s foo -n bar",
            "tmux -L [SOCKET_NAME] new-window -t foo:2 -n baz -b",
            "tmux -L [SOCKET_NAME] new-window -t foo:3 -n qux -b",
            "tmux -L [SOCKET_NAME] new-window -t foo:4 -n derp -b",
            "tmux attach",
        ]
        "###);

        assert_debug_snapshot!(gather_tmux_state(&options), @r###"
        {
            "foo": [
                "bar",
                "baz",
                "qux",
                "derp",
            ],
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_creates_missing_windows_when_server_is_already_started() -> Result<()> {
        let options = build_testing_options();

        create_tmux_session("foo", "baz", &options)?;

        let config = Config {
            crate_locations: None,
            shell_caching: None,
            tmux: Some(Tmux {
                default_session: None,
                sessions: vec![Session {
                    name: "foo".to_string(),
                    windows: vec![
                        Window {
                            name: "baz".to_string(),
                            path: None,
                            command: None,
                            env: None,
                            linked_crates: None,
                        },
                        Window {
                            name: "bar".to_string(),
                            path: None,
                            command: None,
                            env: None,
                            linked_crates: None,
                        },
                    ],
                }],
            }),
        };

        let commands = startup_tmux(&config, &options)?;
        let commands = sanitize_commands_executed(commands, &options, None);

        assert_debug_snapshot!(commands, @r###"
        [
            "tmux -L [SOCKET_NAME] new-window -t foo:2 -n bar -b",
            "tmux attach",
        ]
        "###);

        assert_debug_snapshot!(gather_tmux_state(&options), @r###"
        {
            "foo": [
                "baz",
                "bar",
            ],
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_inserts_missing_windows_at_correct_index() -> Result<()> {
        let options = build_testing_options();

        create_tmux_session("foo", "baz", &options)?;

        let config = Config {
            crate_locations: None,
            shell_caching: None,
            tmux: Some(Tmux {
                default_session: None,
                sessions: vec![Session {
                    name: "foo".to_string(),
                    windows: vec![
                        Window {
                            name: "bar".to_string(),
                            path: None,
                            command: None,
                            env: None,
                            linked_crates: None,
                        },
                        Window {
                            name: "baz".to_string(),
                            path: None,
                            command: None,
                            env: None,
                            linked_crates: None,
                        },
                    ],
                }],
            }),
        };

        let commands = startup_tmux(&config, &options)?;
        let commands = sanitize_commands_executed(commands, &options, None);

        assert_debug_snapshot!(commands, @r###"
        [
            "tmux -L [SOCKET_NAME] new-window -t foo:1 -n bar -b",
            "tmux attach",
        ]
        "###);

        assert_debug_snapshot!(gather_tmux_state(&options), @r###"
        {
            "foo": [
                "bar",
                "baz",
            ],
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_does_nothing_if_already_started() -> Result<()> {
        let options = build_testing_options();

        create_tmux_session("foo", "bar", &options)?;

        assert_debug_snapshot!(gather_tmux_state(&options), @r###"
        {
            "foo": [
                "bar",
            ],
        }
        "###);

        let config = Config {
            crate_locations: None,
            shell_caching: None,
            tmux: Some(Tmux {
                default_session: None,
                sessions: vec![Session {
                    name: "foo".to_string(),
                    windows: vec![Window {
                        linked_crates: None,
                        name: "bar".to_string(),
                        path: None,
                        command: None,
                        env: None,
                    }],
                }],
            }),
        };
        let commands = startup_tmux(&config, &options)?;
        let commands = sanitize_commands_executed(commands, &options, None);

        assert_debug_snapshot!(commands, @r###"
        [
            "tmux attach",
        ]
        "###);

        assert_debug_snapshot!(gather_tmux_state(&options), @r###"
        {
            "foo": [
                "bar",
            ],
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_sets_specified_environment_variables_when_window_is_created() -> Result<()> {
        let options = build_testing_options();

        let temp_dir = tempdir().expect("Failed to create a temporary directory");
        let temp_path = temp_dir.into_path();
        let temp_path = temp_path.join("some-file.txt");
        let temp_path_str = temp_path
            .to_str()
            .expect("Failed to convert temp path to str");

        let mut additional_replacements = HashMap::new();
        additional_replacements.insert(
            temp_path_str.to_string(),
            "/tmp/random-value/some-file.txt".to_string(),
        );

        let config = Config {
            crate_locations: None,
            shell_caching: None,
            tmux: Some(Tmux {
                default_session: None,
                sessions: vec![Session {
                    name: "foo".to_string(),
                    windows: vec![Window {
                        linked_crates: None,
                        name: "bar".to_string(),
                        path: None,
                        command: Some(ConfigCommand::Single(format!(
                            "echo \"$FOO-$BAZ\" > {}",
                            temp_path_str
                        ))),
                        env: Some(BTreeMap::from([
                            ("FOO".to_string(), "bar".to_string()),
                            ("BAZ".to_string(), "qux".to_string()),
                        ])),
                    }],
                }],
            }),
        };

        let commands = startup_tmux(&config, &options)?;

        let commands =
            sanitize_commands_executed(commands, &options, Some(additional_replacements));
        assert_debug_snapshot!(commands, @r###"
        [
            "tmux -L [SOCKET_NAME] new-session -d -s foo -n bar -e BAZ=qux -e FOO=bar",
            "tmux -L [SOCKET_NAME] send-keys -t foo:bar 'echo \"$FOO-$BAZ\" > /tmp/random-value/some-file.txt' Enter",
            "tmux attach",
        ]
        "###);

        wait_for_file(
            &temp_path,
            format!(
                "tmux socket: {} -- file not created: {}",
                options.socket_name,
                temp_path.to_str().unwrap()
            ),
        );

        assert_eq!(std::fs::read_to_string(&temp_path)?, "bar-qux\n");

        Ok(())
    }

    #[test]
    fn test_invokes_command_when_window_is_created() -> Result<()> {
        let options = build_testing_options();

        let temp_dir = tempdir().expect("Failed to create a temporary directory");
        let temp_path = temp_dir.into_path();
        let temp_path = temp_path.join("some-file.txt");
        let temp_path_str = temp_path
            .to_str()
            .expect("Failed to convert temp path to str");

        let mut additional_replacements = HashMap::new();
        additional_replacements.insert(
            temp_path_str.to_string(),
            "/tmp/random-value/some-file.txt".to_string(),
        );

        let config = Config {
            crate_locations: None,
            shell_caching: None,
            tmux: Some(Tmux {
                default_session: None,
                sessions: vec![Session {
                    name: "foo".to_string(),
                    windows: vec![Window {
                        name: "bar".to_string(),
                        path: None,
                        command: Some(ConfigCommand::Single(format!("touch {}", temp_path_str))),
                        env: None,
                        linked_crates: None,
                    }],
                }],
            }),
        };

        let commands = startup_tmux(&config, &options)?;

        let commands =
            sanitize_commands_executed(commands, &options, Some(additional_replacements));
        assert_debug_snapshot!(commands, @r###"
        [
            "tmux -L [SOCKET_NAME] new-session -d -s foo -n bar",
            "tmux -L [SOCKET_NAME] send-keys -t foo:bar 'touch /tmp/random-value/some-file.txt' Enter",
            "tmux attach",
        ]
        "###);
        assert_debug_snapshot!(gather_tmux_state(&options), @r###"
        {
            "foo": [
                "bar",
            ],
        }
        "###);

        wait_for_file(
            &temp_path,
            format!(
                "tmux socket: {} -- file not created: {}",
                options.socket_name,
                temp_path.to_str().unwrap()
            ),
        );

        Ok(())
    }

    #[test]
    fn test_attempts_to_attach_to_default_session() -> Result<()> {
        unsafe {
            env::remove_var("TMUX");
        }

        let options = build_testing_options();

        let config = Config {
            crate_locations: None,
            shell_caching: None,
            tmux: Some(Tmux {
                default_session: Some("foo".to_string()),
                sessions: vec![Session {
                    name: "foo".to_string(),
                    windows: vec![Window {
                        name: "bar".to_string(),
                        path: None,
                        command: None,
                        env: None,
                        linked_crates: None,
                    }],
                }],
            }),
        };
        let commands = startup_tmux(&config, &options)?;
        let commands = sanitize_commands_executed(commands, &options, None);

        assert_debug_snapshot!(commands, @r###"
        [
            "tmux -L [SOCKET_NAME] new-session -d -s foo -n bar",
            "tmux attach -t foo",
        ]
        "###);

        Ok(())
    }

    #[test]
    fn test_attempts_to_attach_without_default_session() -> Result<()> {
        unsafe {
            env::remove_var("TMUX");
        }

        let options = build_testing_options();

        let config = Config {
            crate_locations: None,
            shell_caching: None,
            tmux: Some(Tmux {
                default_session: None,
                sessions: vec![Session {
                    name: "foo".to_string(),
                    windows: vec![Window {
                        name: "bar".to_string(),
                        path: None,
                        command: None,
                        env: None,
                        linked_crates: None,
                    }],
                }],
            }),
        };
        let commands = startup_tmux(&config, &options)?;
        let commands = sanitize_commands_executed(commands, &options, None);

        assert_debug_snapshot!(commands, @r###"
        [
            "tmux -L [SOCKET_NAME] new-session -d -s foo -n bar",
            "tmux attach",
        ]
        "###);

        Ok(())
    }

    #[test]
    fn determine_commands_for_window_single_command() {
        let window = Window {
            path: None,
            env: None,
            name: "test_window".to_string(),
            command: Some(config::Command::Single("echo Hello".to_string())),
            linked_crates: None,
        };
        let crates = BTreeMap::new();
        let result = determine_commands_for_window(&window, &crates).unwrap();
        assert_debug_snapshot!(result, @r###"
        Some(
            [
                "echo Hello",
            ],
        )
        "###);
    }

    #[test]
    fn determine_commands_for_window_single_command_and_linked_crates() {
        let window = Window {
            path: None,
            env: None,
            name: "test_window".to_string(),
            command: Some(config::Command::Single("echo Hello".to_string())),
            linked_crates: Some(vec!["crate1".to_string()]),
        };
        let mut crates = BTreeMap::new();
        crates.insert("crate1".to_string(), PathBuf::from("/path/to/crate1"));
        let result = determine_commands_for_window(&window, &crates).unwrap();
        assert_debug_snapshot!(result, @r###"
        Some(
            [
                "export PATH=\"/path/to/crate1:$PATH\"",
                "echo Hello",
            ],
        )
        "###);
    }

    #[test]
    fn determine_commands_for_window_multiple_commands() {
        let window = Window {
            path: None,
            env: None,
            name: "test_window".to_string(),
            command: Some(config::Command::Multiple(vec![
                "echo Hello".to_string(),
                "echo World".to_string(),
            ])),
            linked_crates: None,
        };
        let crates = BTreeMap::new();
        let result = determine_commands_for_window(&window, &crates).unwrap();
        assert_debug_snapshot!(result, @r###"
        Some(
            [
                "echo Hello",
                "echo World",
            ],
        )
        "###);
    }

    #[test]
    fn determine_commands_for_window_no_command_with_linked_crates() {
        let window = Window {
            path: None,
            env: None,
            name: "test_window".to_string(),
            command: None,
            linked_crates: Some(vec!["crate1".to_string()]),
        };
        let mut crates = BTreeMap::new();
        crates.insert("crate1".to_string(), PathBuf::from("/path/to/crate1"));
        let result = determine_commands_for_window(&window, &crates).unwrap();
        assert_debug_snapshot!(result, @r###"
        Some(
            [
                "export PATH=\"/path/to/crate1:$PATH\"",
            ],
        )
        "###);
    }

    #[test]
    fn determine_commands_for_window_no_command_no_linked_crates() {
        let window = Window {
            path: None,
            env: None,
            name: "test_window".to_string(),
            command: None,
            linked_crates: None,
        };
        let crates = BTreeMap::new();
        let result = determine_commands_for_window(&window, &crates).unwrap();
        assert_debug_snapshot!(result, @"None");
    }

    #[test]
    fn determine_commands_for_window_missing_crate() {
        let window = Window {
            path: None,
            env: None,
            name: "test_window".to_string(),
            command: None,
            linked_crates: Some(vec!["missing_crate".to_string()]),
        };
        let crates = BTreeMap::new();
        let result = determine_commands_for_window(&window, &crates);

        assert_debug_snapshot!(result, @r###"
        Err(
            "Could not find crate: missing_crate for linking into test_window",
        )
        "###);
    }

    #[test]
    fn test_linked_crates() -> Result<()> {
        let temp_dir = tempdir()?;
        let options = build_testing_options();

        let workspace_dir = temp_dir.path().join("workspace");
        create_workspace_with_packages(
            workspace_dir.as_path(),
            vec![FakePackage {
                name: "foo".to_string(),
                bins: vec![FakeBin {
                    name: "bar".to_string(),
                    contents: Some(
                        r###"
                        use std::fs::File;

                        fn main() {
                            File::create("foo.txt").unwrap();
                        }"###
                            .to_string(),
                    ),
                }],
            }],
        );

        generate_symlinks(Some(workspace_dir.clone())).unwrap();

        let working_dir = temp_dir.path().join("working_dir");
        fs::create_dir_all(&working_dir)?;

        let config = Config {
            crate_locations: Some(vec![workspace_dir.to_string_lossy().to_string()]),
            shell_caching: None,
            tmux: Some(Tmux {
                default_session: None,
                sessions: vec![Session {
                    name: "foo".to_string(),
                    windows: vec![Window {
                        name: "bar".to_string(),
                        path: Some(working_dir.to_path_buf()),
                        command: Some(ConfigCommand::Single("bar".to_string())),
                        env: None,
                        linked_crates: Some(vec!["foo".to_string()]),
                    }],
                }],
            }),
        };
        let commands = startup_tmux(&config, &options)?;
        let commands = sanitize_commands_executed(
            commands,
            &options,
            Some(HashMap::from([(
                temp_dir.path().to_string_lossy().to_string(),
                "[TEMP_DIR]".to_string(),
            )])),
        );

        assert_debug_snapshot!(commands, @r###"
        [
            "tmux -L [SOCKET_NAME] new-session -d -s foo -n bar -c [TEMP_DIR]/working_dir",
            "tmux -L [SOCKET_NAME] send-keys -t foo:bar 'export PATH=\"[TEMP_DIR]/workspace/foo/target/debug/:$PATH\"' Enter",
            "tmux -L [SOCKET_NAME] send-keys -t foo:bar bar Enter",
            "tmux attach",
        ]
        "###);

        let expected_file_path = working_dir.join("foo.txt");
        wait_for_file(
            expected_file_path.as_path(),
            format!("file not created: {}", expected_file_path.display()),
        );

        Ok(())
    }

    // TODO: validate paths eagerly (and error instead of running & failing)
}
