use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

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
        config_file: config_dir.join("config.lua"),
        config_dir,
        original_home,
    }
}

pub fn stabilize_home_paths(env: &TestEnvironment, input: &str) -> String {
    let home_str = env.home.to_str().expect("Failed to convert PathBuf to str");
    input.replace(home_str, "~")
}

pub struct FakePackage {
    pub name: String,
    pub bins: Vec<FakeBin>,
}

pub struct FakeBin {
    pub name: String,
    pub contents: Option<String>,
}

pub fn create_workspace_with_packages(workspace_dir: &Path, packages: Vec<FakePackage>) {
    // Create workspace Cargo.toml
    let workspace_toml = workspace_dir.join("Cargo.toml");

    let mut workspace_file = File::create(workspace_toml).unwrap();
    writeln!(
        workspace_file,
        "[workspace]\nmembers = [{}]\n",
        packages
            .iter()
            .map(|pkg| format!("\"{}\"", pkg.name))
            .collect::<Vec<_>>()
            .join(", ")
    )
    .unwrap();

    for package in packages {
        // Create package directory
        let package_dir = workspace_dir.join(&package.name);
        fs::create_dir_all(&package_dir).unwrap();

        // Create package Cargo.toml
        let package_toml = package_dir.join("Cargo.toml");
        let mut package_file = File::create(&package_toml).unwrap();
        writeln!(
            package_file,
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2018\"\n",
            package.name
        )
        .unwrap();

        // Create src/bin directory and bin files
        let src_bin_dir = package_dir.join("src/bin");
        fs::create_dir_all(&src_bin_dir).unwrap();
        for bin in package.bins {
            let bin_rs = src_bin_dir.join(format!("{}.rs", bin.name));
            let contents = bin.contents.unwrap_or_else(|| {
                "fn main() {{\n    println!(\"{{:?}}\", std::env::current_exe().unwrap());\n}}\n".to_string()
            });
            fs::write(bin_rs, contents).unwrap();
        }
    }

    // Run `cargo build` within the workspace to generate the `target/` directory and binaries
    let output = Command::new("cargo")
        .arg("build")
        .current_dir(workspace_dir)
        .output()
        .expect("Failed to run `cargo build`");

    if !output.status.success() {
        panic!(
            "cargo build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
