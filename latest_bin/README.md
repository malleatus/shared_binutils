# latest_bin

This library provides utilities to ensure that you are always running the
latest version of your binaries when in debug mode. It checks if any files in
your crate have been updated since the last build and, if so, rebuilds the
crate and re-executes the updated binary.

## Usage

To use this library, add it as a dependency in your `Cargo.toml`:

```toml
[dependencies]
latest_bin = { git = "https://github.com/malleatus/shared_binutils.git", subdir = "latest_bin" }

```

Then, in your main function, call `ensure_latest_bin` before running your
actual code:

```rust
fn main() {
    latest_bin::ensure_latest_bin()?;

    // ... actual code :D
}
```

## Functions

### `ensure_latest_bin() -> Result<()>`

Checks if any files in the crate have been updated since the last build. If
updates are found, it rebuilds the crate and re-executes the updated binary.

### `needs_rebuild() -> Result<bool>`

Determines if there are any updated files that require a rebuild.

### `run_cargo_build() -> Result<()>`

Runs `cargo build` in the crate root directory.

### `exec_updated_bin() -> Result<()>`

Re-executes the current binary.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file
for details.
