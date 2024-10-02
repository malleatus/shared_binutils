use anyhow::anyhow;
use mlua::{Lua, LuaSerdeExt};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::fs;
use std::path::Path;
use tracing::{debug, error, trace};

use anyhow::{Context, Result};

pub mod lua_type_gen;

pub fn read_config<T: DeserializeOwned + Debug>(config_path: &Path) -> Result<T> {
    if !config_path.is_file() {
        error!(
            "The specified config path is not a file: {}",
            config_path.display()
        );

        anyhow::bail!(
            "The specified config path is not a file: {}",
            config_path.display()
        );
    }

    debug!("Reading config from: {}", config_path.display());

    let lua = Lua::new();
    let globals = lua.globals();
    let config_dir = config_path.parent().ok_or_else(|| {
        anyhow!(
            "Could not get parent directory of config_path: {}",
            config_path.display()
        )
    })?;

    let package: mlua::Table = globals.get("package")?;
    let package_path: String = package.get("path")?;

    let new_package_path = format!(
        "{}/?.lua;{}/?/init.lua;{}",
        config_dir.display(),
        config_dir.display(),
        package_path
    );
    package.set("path", new_package_path)?;
    let config_str = fs::read_to_string(config_path).with_context(|| {
        format!(
            "Could not read config file from: {}",
            &config_path.display()
        )
    })?;
    let result = lua
        .load(&config_str)
        .set_name(config_path.to_string_lossy())
        .eval()?;

    let config: T = lua.from_value(result)?;

    trace!("Config: {:?}", config);

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_debug_snapshot, assert_snapshot};
    use std::fs;
    use test_utils::{setup_test_environment, stabilize_home_paths};

    #[test]
    fn test_read_config_basically_works() -> Result<()> {
        let env = setup_test_environment();
        fs::write(&env.config_file, b"{ test = 42 }")?;

        #[derive(serde::Deserialize, Debug)]
        struct TestConfig {
            #[allow(dead_code)]
            test: i32,
        }

        let config: TestConfig = read_config(&env.config_file)?;

        assert_debug_snapshot!(config, @r###"
        TestConfig {
            test: 42,
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_read_config_invalid_lua() -> Result<()> {
        let env = setup_test_environment();

        fs::write(&env.config_file, b"invalid lua contents")?;

        #[derive(serde::Deserialize, Debug)]
        struct TestConfig {}

        let err = read_config::<TestConfig>(&env.config_file).unwrap_err();

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
}
