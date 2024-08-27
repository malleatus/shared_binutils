# Shell Utilities

Example Usage:

```rust
use shell::*;

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
}
```
