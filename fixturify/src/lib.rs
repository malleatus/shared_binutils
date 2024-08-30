use anyhow::{Context, Result};
use ignore::WalkBuilder;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Reads a file from the given path and returns its contents as a `BTreeMap<String, String>`.
///
/// # Parameters
/// - `from`: A path to the file to be read. This can be any type that implements `AsRef<Path>`.
///
/// # Returns
/// - `Result<BTreeMap<String, String>>`: A result containing a `BTreeMap` with the file's contents if successful, or an error if the operation fails.
///
/// # Example
/// ```
/// use std::collections::BTreeMap;
/// use std::path::Path;
/// use fixturify::read;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let path = Path::new("example.txt");
///     let contents: BTreeMap<String, String> = read(path)?;
///     for (key, value) in contents.iter() {
///         println!("{}: {}", key, value);
///     }
///     Ok(())
/// }
/// ```
pub fn read<S: AsRef<Path>>(from: S) -> Result<BTreeMap<String, String>> {
    let path = from.as_ref();
    let mut file_map = BTreeMap::new();

    let walker = WalkBuilder::new(path)
        // NOTE: explicitly disable ignoring hidden files (i.e. we should include `.foo/bar.txt`)
        // https://docs.rs/ignore/0.4.22/ignore/struct.WalkBuilder.html#method.hidden
        .hidden(false)
        .filter_entry(|entry| {
            // NOTE: since we turn off hidden file ignoring, the `.git` directory is now included
            entry
                .path()
                .file_name()
                .map_or(true, |file_name| file_name != ".git")
        })
        .build();

    for result in walker {
        let entry = result?;
        let path = entry.path();

        if path.is_file() {
            let relative_path = path
                .strip_prefix(&from)
                .with_context(|| {
                    format!(
                        "Failed to strip prefix from path: {:?} with prefix: {:?}",
                        path, path
                    )
                })?
                .to_path_buf();
            let file_content = fs::read_to_string(path)
                .with_context(|| format!("Failed to read file: {:?}", path))?;
            file_map.insert(relative_path.to_string_lossy().to_string(), file_content);
        }
    }
    Ok(file_map)
}

/// Writes the given `BTreeMap<String, String>` to a file at the specified path.
///
/// # Parameters
/// - `to`: A path to the file where the contents will be written. This can be any type that implements `AsRef<Path>`.
/// - `contents`: A `BTreeMap<String, String>` containing the data to be written to the file.
///
/// # Returns
/// - `Result<()>`: A result indicating success or failure of the write operation.
///
/// # Example
/// ```
/// use std::collections::BTreeMap;
/// use std::path::Path;
/// use fixturify::write;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let path = Path::new("example.txt");
///     let mut contents = BTreeMap::new();
///     contents.insert("key1".to_string(), "value1".to_string());
///     contents.insert("key2".to_string(), "value2".to_string());
///     write(path, &contents)?;
///     Ok(())
/// }
/// ```
pub fn write<S: AsRef<Path>>(to: S, file_map: &BTreeMap<String, String>) -> Result<()> {
    let base_path = to.as_ref();

    for (relative_path, content) in file_map {
        let full_path = base_path.join(relative_path);

        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directories for path: {:?}", parent))?;
        }

        fs::write(&full_path, content).with_context(|| {
            format!("Failed to write file: {:?}, with:\n{}", full_path, content)
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;
    use std::fs::{self, File};
    use std::io::Write;
    use std::process::Command;
    use tempfile::{tempdir, TempDir};

    fn write_file(dir: &TempDir, file_name: &str, content: &str) -> Result<()> {
        let file_path = dir.path().join(file_name);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(&file_path)?;
        writeln!(file, "{}", content)?;

        Ok(())
    }

    #[test]
    fn test_read() -> Result<()> {
        let dir = tempdir()?;

        write_file(&dir, "test.txt", "Hello, world!")?;
        write_file(&dir, "other/path.txt", "Hello, world!")?;

        let result = read(dir.path())?;
        assert_debug_snapshot!(result, @r###"
        {
            "other/path.txt": "Hello, world!\n",
            "test.txt": "Hello, world!\n",
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_write() -> Result<()> {
        let dir = tempdir()?;
        let file_map = BTreeMap::from([
            ("test.txt".to_string(), "Hello, world!".to_string()),
            ("other/path.txt".to_string(), "Hello, world!".to_string()),
        ]);

        write(dir.path(), &file_map)?;

        let file_path = dir.path().join("test.txt");
        let content = fs::read_to_string(file_path)?;
        assert_debug_snapshot!(content, @r###""Hello, world!""###);

        Ok(())
    }

    #[test]
    fn test_read_write_cycle() -> Result<()> {
        let dir = tempdir()?;
        write_file(&dir, "test.txt", "Hello, world!")?;
        write_file(&dir, "other/path.txt", "Hello, world!")?;

        let file_map = read(dir.path())?;
        let new_dir = tempdir()?;
        write(new_dir.path(), &file_map)?;

        let updated_file_map = read(new_dir.path())?;

        assert_eq!(file_map, updated_file_map);

        Ok(())
    }

    #[test]
    fn test_ignore_git_objects() -> Result<()> {
        let dir = tempdir()?;

        Command::new("git").arg("init").current_dir(&dir).output()?;

        write_file(&dir, "test.txt", "Hello, world!")?;
        write_file(
            &dir,
            ".git/objects/00/6b14c2f67dbf09234f304a8b63b2e56ca8c516",
            "This should be ignored",
        )?;

        let result = read(dir.path())?;
        assert_debug_snapshot!(result, @r###"
        {
            "test.txt": "Hello, world!\n",
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_gitignored_files_in_git_repo() -> Result<()> {
        let dir = tempdir()?;

        Command::new("git").arg("init").current_dir(&dir).output()?;

        write_file(&dir, "test.txt", "Hello, world!")?;
        write_file(&dir, ".gitignore", "ignored.txt")?;
        write_file(&dir, "ignored.txt", "This should be ignored")?;

        let result = read(dir.path())?;
        assert_debug_snapshot!(result, @r###"
        {
            ".gitignore": "ignored.txt\n",
            "test.txt": "Hello, world!\n",
        }
        "###);

        Ok(())
    }

    #[test]
    fn test_reads_dot_files() -> Result<()> {
        let dir = tempdir()?;
        write_file(&dir, "test.txt", "Hello, world!")?;
        write_file(&dir, ".dotfile", "This should not be ignored")?;
        let result = read(dir.path())?;
        assert_debug_snapshot!(result, @r###"
        {
            ".dotfile": "This should not be ignored\n",
            "test.txt": "Hello, world!\n",
        }
        "###);
        Ok(())
    }
}
