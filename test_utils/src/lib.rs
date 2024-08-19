use std::env;
use std::fs;
use std::path::PathBuf;

use tempfile::tempdir;

#[derive(Debug, Clone)]
pub struct TestEnvironment {
    pub home: PathBuf,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub original_home: Option<String>,
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        match &self.original_home {
            Some(home) => unsafe { env::set_var("HOME", home) },
            None => unsafe { env::remove_var("HOME") },
        }
    }
}

pub fn setup_test_environment() -> TestEnvironment {
    let original_home = env::var("HOME").ok();
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let temp_home = temp_dir.into_path();

    unsafe {
        env::set_var(
            "HOME",
            temp_home
                .to_str()
                .expect("Failed to convert temp path to str"),
        );
    }

    // Ensure config directory exists
    let config_dir = temp_home.join(".config/binutils");
    fs::create_dir_all(&config_dir).expect("Failed to create .config directory");

    TestEnvironment {
        home: temp_home,
        config_file: config_dir.join("config.yaml"),
        config_dir,
        original_home,
    }
}

pub fn stabilize_home_paths(env: &TestEnvironment, input: &str) -> String {
    let home_str = env.home.to_str().expect("Failed to convert PathBuf to str");
    input.replace(home_str, "~")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_setup_test_environment() {
        let test_env = setup_test_environment();

        let home_var = env::var("HOME").expect("HOME variable not set");
        assert_eq!(
            home_var,
            test_env
                .home
                .to_str()
                .expect("Failed to convert PathBuf to str")
        );

        assert!(test_env.config_dir.exists());

        assert_ne!(test_env.original_home, env::var("HOME").ok());
    }

    #[test]
    fn test_stabilize_home_paths() {
        let test_env = setup_test_environment();
        let home_str = test_env
            .home
            .to_str()
            .expect("Failed to convert PathBuf to str");

        // Create a sample input string containing the home path
        let input = format!("{}/some/path", home_str);
        let expected_output = "~/some/path";

        // Check if the home path is replaced with ~
        let output = stabilize_home_paths(&test_env, &input);
        assert_eq!(output, expected_output);
    }

    #[test]
    fn test_drop_test_environment() {
        let original_home = env::var("HOME").ok();
        {
            let test_env = setup_test_environment();

            let home_var = env::var("HOME").expect("HOME variable not set");
            assert_eq!(
                home_var,
                test_env
                    .home
                    .to_str()
                    .expect("Failed to convert PathBuf to str")
            );
        }

        let home_var = env::var("HOME").ok();
        assert_eq!(home_var, original_home);
    }

    #[test]
    fn test_home_variable_updated() {
        let original_home = env::var("HOME").expect("HOME variable not set");

        let test_env = setup_test_environment();

        let new_home = env::var("HOME").expect("HOME variable not set");

        assert_ne!(original_home, new_home);

        assert_eq!(
            new_home,
            test_env
                .home
                .to_str()
                .expect("Failed to convert PathBuf to str")
        );
    }
}
