use mlua::{Lua, LuaSerdeExt};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, error, trace};

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Configuration for the application.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Config {
    /// Optional tmux configuration. Including sessions and windows to be created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmux: Option<Tmux>,

    /// Optional configuration for cache-shell-setup
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_caching: Option<ShellCache>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShellCache {
    pub source: String,
    pub destination: String,
}

/// Tmux configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tmux {
    /// List of tmux sessions.
    pub sessions: Vec<Session>,

    /// The default session to attach to when `startup-tmux --attach` is ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_session: Option<String>,
}

/// Configuration for a tmux session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    /// Name of the session.
    pub name: String,
    /// List of windows in the session.
    pub windows: Vec<Window>,
}

/// Command to be executed in a tmux window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Command {
    /// A single command as a string.
    Single(String),
    /// Multiple commands as a list of strings.
    Multiple(Vec<String>),
}

/// Configuration for a tmux window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Window {
    /// Name of the window.
    pub name: String,
    /// Optional path to set as the working directory for the window.
    #[serde(
        default,
        serialize_with = "path_to_string",
        deserialize_with = "string_to_path",
        skip_serializing_if = "Option::is_none"
    )]
    pub path: Option<PathBuf>,

    /// Optional command to run in the window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Command>,

    /// Additional environment variables to set in the window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
}

pub fn default_config() -> Config {
    Config {
        shell_caching: None,
        tmux: Some(Tmux {
            default_session: None,
            sessions: vec![],
        }),
    }
}

fn path_to_string<S>(path: &Option<PathBuf>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match path {
        Some(p) => {
            let path_str = revert_tokens_in_path(p);
            serializer.serialize_some(&path_str)
        }
        None => serializer.serialize_none(),
    }
}

fn string_to_path<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) => Ok(Some(PathBuf::from(replace_tokens_in_path(&s)))),
        None => Ok(None),
    }
}

fn replace_tokens_in_path(path: &str) -> String {
    match path.strip_prefix('~') {
        Some(stripped) => {
            let home_dir = env::var("HOME").expect("HOME environment variable not set");

            format!("{}{}", home_dir, stripped)
        }
        None => path.to_string(),
    }
}

fn revert_tokens_in_path(path: &Path) -> String {
    let home_dir = env::var("HOME").unwrap_or_default();
    let path_str = path.to_str().unwrap_or("");

    match path_str.strip_prefix(&home_dir) {
        Some(stripped) => {
            format!("~{}", stripped)
        }
        None => path_str.to_string(),
    }
}

fn expand_tilde(path: PathBuf) -> PathBuf {
    let path_str = path.to_str().unwrap_or_default();

    match path_str.strip_prefix('~') {
        Some(stripped) => {
            let home_dir = env::var("HOME").expect("HOME environment variable not set");
            let expanded = format!("{}{}", home_dir, stripped);

            PathBuf::from(expanded)
        }
        None => path,
    }
}

pub fn read_config(config_path: Option<PathBuf>) -> Result<Config> {
    let config_path = match config_path {
        Some(config_path) => {
            let config_path = expand_tilde(config_path);

            if !config_path.is_file() {
                error!(
                    "The specified config path is not a file: {}",
                    config_path.display()
                );

                return Err(anyhow::anyhow!(
                    "The specified config path is not a file: {}",
                    config_path.display()
                ));
            }

            config_path
        }
        None => {
            trace!("No config path specified, using default config path");

            let home_dir = env::var("HOME").expect("HOME environment variable not set");

            let local_config_file = Path::new(&home_dir).join(".config/binutils/local.config.lua");
            if local_config_file.exists() {
                local_config_file
            } else {
                Path::new(&home_dir).join(".config/binutils/config.lua")
            }
        }
    };

    let config = if config_path.is_file() {
        debug!("Reading config from: {}", config_path.display());

        let lua = Lua::new();
        let globals = lua.globals();
        let config_dir = config_path.parent().map(PathBuf::from).unwrap_or_else(|| {
            let home_dir = env::var("HOME").expect("HOME environment variable not set");
            PathBuf::from(home_dir)
        });

        let package: mlua::Table = globals.get("package")?;
        let package_path: String = package.get("path")?;

        let new_package_path = format!(
            "{}/?.lua;{}/?/init.lua;{}",
            config_dir.display(),
            config_dir.display(),
            package_path
        );
        package.set("path", new_package_path)?;
        let config_str = fs::read_to_string(&config_path).with_context(|| {
            format!(
                "Could not read config file from: {}",
                &config_path.display()
            )
        })?;
        let result = lua
            .load(&config_str)
            .set_name(config_path.to_string_lossy())
            .eval()?;

        let config: Config = lua.from_value(result)?;

        config
    } else {
        debug!(
            "Using default config. No config file found at: {}",
            config_path.display()
        );

        default_config()
    };

    trace!("Config: {:?}", config);

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_debug_snapshot, assert_snapshot};
    use std::env;
    use tempfile::tempdir;
    use test_utils::{setup_test_environment, stabilize_home_paths};

    #[test]
    fn test_replace_tokens_in_path_with_home() {
        let home_dir = env::var("HOME").expect("HOME not set");
        let path = "~/some/path";
        assert_eq!(
            replace_tokens_in_path(path),
            format!("{}/some/path", home_dir)
        );
    }

    #[test]
    fn test_revert_tokens_in_path_to_home() {
        let home_dir = env::var("HOME").expect("HOME not set");
        let path = PathBuf::from(format!("{}/some/path", home_dir));
        assert_eq!(revert_tokens_in_path(&path), "~/some/path");
    }

    #[test]
    fn test_replace_tokens_in_path_without_home() {
        let path = "/some/other/path";
        assert_eq!(replace_tokens_in_path(path), "/some/other/path");
    }

    #[test]
    fn test_revert_tokens_in_path_without_home() {
        let path = PathBuf::from("/some/other/path");
        assert_eq!(revert_tokens_in_path(&path), "/some/other/path");
    }

    #[test]
    fn test_replace_empty_path() {
        assert_eq!(replace_tokens_in_path(""), "");
    }

    #[test]
    fn test_revert_empty_path() {
        let path = PathBuf::from("");
        assert_eq!(revert_tokens_in_path(&path), "");
    }

    #[test]
    fn test_path_just_home_token() {
        let home_dir = env::var("HOME").expect("HOME not set");
        assert_eq!(replace_tokens_in_path("~"), home_dir);
    }

    #[test]
    fn test_path_just_home_directory() {
        let home_dir = env::var("HOME").expect("HOME not set");
        let path = PathBuf::from(&home_dir);
        assert_eq!(revert_tokens_in_path(&path), "~");
    }

    // This test ensures that paths without the home directory are handled correctly
    #[test]
    fn test_temporary_directory_handling() {
        let temp_dir = tempdir().expect("Failed to create a temporary directory");
        let temp_path = temp_dir.path();
        let temp_path_str = temp_path
            .to_str()
            .expect("Failed to convert temp path to str");

        assert_eq!(
            replace_tokens_in_path(temp_path_str),
            temp_path_str,
            "Temporary paths should not be altered if they do not contain the home directory."
        );
    }

    #[test]
    fn test_read_config_missing_file() {
        let _env = setup_test_environment();

        let config = read_config(None).expect("read_config(None) when no config exists failed");

        assert_debug_snapshot!(config, @r###"
        Config {
            tmux: Some(
                Tmux {
                    sessions: [],
                    default_session: None,
                },
            ),
            shell_caching: None,
        }
        "###);
    }

    #[test]
    fn test_read_config_custom_config_file_path() {
        let env = setup_test_environment();

        let config_file_path = env.config_dir.join("custom-config.lua");
        fs::write(&config_file_path, r###"return {}"###).unwrap();

        let config = read_config(Some(config_file_path)).expect("error reading from config");

        assert_debug_snapshot!(config, @r###"
        Config {
            tmux: None,
            shell_caching: None,
        }
        "###);
    }

    #[test]
    fn test_read_default_local_config_file() {
        let env = setup_test_environment();

        let config_file_path = env.config_dir.join("local.config.lua");
        let config_str = r###"
        return {
            shell_caching = nil,
            tmux = {
                default_session = "Test Session",
                sessions = {
                    {
                        name = "Test Session",
                        windows = {
                            {
                                name = "Test Window",
                                command = "echo 'Hello, world!'",
                            }
                        }
                    }
                }
            }
        }"###;

        fs::write(&config_file_path, config_str).expect("Could not write to config file");

        let config = read_config(None).expect("error reading from config");

        assert_debug_snapshot!(config, @r###"
        Config {
            tmux: Some(
                Tmux {
                    sessions: [
                        Session {
                            name: "Test Session",
                            windows: [
                                Window {
                                    name: "Test Window",
                                    path: None,
                                    command: Some(
                                        Single(
                                            "echo 'Hello, world!'",
                                        ),
                                    ),
                                    env: None,
                                },
                            ],
                        },
                    ],
                    default_session: Some(
                        "Test Session",
                    ),
                },
            ),
            shell_caching: None,
        }
        "###);
    }

    #[test]
    fn test_read_config_can_require_local_lua_files() {
        let env = setup_test_environment();

        fixturify::write(
            &env.config_dir,
            BTreeMap::from([
                (
                    "local.config.lua".to_string(),
                    r###"
                    local config = require("./other/config");
                    local additional_sessions = {
                        {
                            name = "huzza!",
                            windows = { { name = "wheeeeee" } }
                        },
                    };

                    for _, session in ipairs(additional_sessions) do
                        table.insert(config.tmux.sessions, session)
                    end

                    return config;
                    "###
                    .to_string(),
                ),
                (
                    "other/config.lua".to_string(),
                    r###"return {
                        tmux = {
                            default_session = "Test Session",
                            sessions = {
                                {
                                    name = "Test Session",
                                    windows = {
                                        {
                                            name = "Test Window",
                                            command = "echo 'Hello, world!'",
                                        }
                                    }
                                }
                            }
                        }
                    }"###
                        .to_string(),
                ),
            ]),
        )
        .unwrap();

        let config = read_config(None).expect("error reading from config");

        assert_debug_snapshot!(config, @r###"
        Config {
            tmux: Some(
                Tmux {
                    sessions: [
                        Session {
                            name: "Test Session",
                            windows: [
                                Window {
                                    name: "Test Window",
                                    path: None,
                                    command: Some(
                                        Single(
                                            "echo 'Hello, world!'",
                                        ),
                                    ),
                                    env: None,
                                },
                            ],
                        },
                        Session {
                            name: "huzza!",
                            windows: [
                                Window {
                                    name: "wheeeeee",
                                    path: None,
                                    command: None,
                                    env: None,
                                },
                            ],
                        },
                    ],
                    default_session: Some(
                        "Test Session",
                    ),
                },
            ),
            shell_caching: None,
        }
        "###);
    }

    #[test]
    fn test_read_config_local_config_wins() {
        let env = setup_test_environment();

        let local_config_path = env.config_dir.join("local.config.lua");
        fs::write(&local_config_path, r###"return {}"###).unwrap();

        let config_path = &env.config_file;
        fs::write(
            config_path,
            r###"return { shell_caching = { source = "~/foo", destination = "~/foo/dist" } }"###,
        )
        .expect("Could not write to config file");

        let config = read_config(None).expect("error reading from config");

        assert_debug_snapshot!(config, @r###"
        Config {
            tmux: None,
            shell_caching: None,
        }
        "###);
    }

    #[test]
    fn test_read_config_custom_file_with_tilde() {
        let env = setup_test_environment();

        let config_file_path = env.home.join("custom-config.lua");
        let config_str = r###"
        {
            tmux = {
                default_session = "Test Session",
                sessions = {
                    {
                        name = "Test Session",
                        windows = {
                            {
                                name = "Test Window",
                                command = "echo 'Hello, world!'",
                            }
                        }
                    }
                }
            }
        }"###;
        fs::write(&config_file_path, config_str).unwrap();

        let config =
            read_config(Some(PathBuf::from("~/custom-config.lua"))).expect("could not read config");

        assert_debug_snapshot!(config, @r###"
        Config {
            tmux: Some(
                Tmux {
                    sessions: [
                        Session {
                            name: "Test Session",
                            windows: [
                                Window {
                                    name: "Test Window",
                                    path: None,
                                    command: Some(
                                        Single(
                                            "echo 'Hello, world!'",
                                        ),
                                    ),
                                    env: None,
                                },
                            ],
                        },
                    ],
                    default_session: Some(
                        "Test Session",
                    ),
                },
            ),
            shell_caching: None,
        }
        "###);
    }

    #[test]
    fn test_read_config_invalid_lua() -> Result<()> {
        let env = setup_test_environment();

        fs::write(&env.config_file, b"invalid lua contents")?;

        let err = read_config(None).unwrap_err();

        // when the filename is very long it gets truncated in the lua syntax error, on macos the
        // tmpdir path is very long
        let error_string = stabilize_home_paths(&env, &err.to_string());

        let re = regex::Regex::new(r#"\[string "(.*?)"\]"#).unwrap();
        let error_string = re.replace_all(&error_string, |caps: &regex::Captures| {
            let path = &caps[1];
            println!("len: {}; value: {}", path.len(), path);
            if path.len() == 48 {
                let truncated_path = &env.home.to_string_lossy()[..45];
                assert_eq!(path, format!("{}...", truncated_path));
            } else {
                assert_eq!(
                    path,
                    stabilize_home_paths(&env, &env.config_file.to_string_lossy())
                );
            }
            "[string \"{truncated path}\"]"
        });

        assert_snapshot!(error_string, @r###"syntax error: [string "{truncated path}"]:1: syntax error near 'lua'"###);

        Ok(())
    }

    #[test]
    fn test_read_config_missing_file_specified() {
        let _env = setup_test_environment();

        let result = read_config(Some(PathBuf::from("/some/nonexistent/file")));
        let err = result.unwrap_err();

        assert_snapshot!(err, @"The specified config path is not a file: /some/nonexistent/file");
    }

    #[test]
    fn test_read_config_tmux_windows_without_path() {
        let env = setup_test_environment();

        let config_str = r###"
        return {
            tmux = {
                sessions = {
                    {
                        name = "Test Session",
                        windows = {
                            {
                                name = "Test Window",
                                command = "echo 'Hello, world!'"
                            }
                        }
                    }
                },
                default_session = "Test Session"
            }
        }
        "###;
        fs::write(&env.config_file, config_str).unwrap();

        let actual = read_config(None).expect("Failed to read config");

        let expected = Config {
            shell_caching: None,
            tmux: Some(Tmux {
                default_session: Some("Test Session".to_string()),
                sessions: vec![Session {
                    name: "Test Session".to_string(),
                    windows: vec![Window {
                        name: "Test Window".to_string(),
                        path: None,
                        command: Some(Command::Single("echo 'Hello, world!'".to_string())),
                        env: None,
                    }],
                }],
            }),
        };

        assert_eq!(expected, actual);
    }
}
