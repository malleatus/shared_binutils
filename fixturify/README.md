# fixturify

`fixturify` is a Rust crate that provides utilities for reading and writing
files as `BTreeMap<String, String>`. This can be useful for managing
configuration files, fixtures, or any other structured data stored in files.
This crate is inspired by the Node.js
[`fixturify`](https://github.com/joliss/fixturify) project by
[@joliss](https://github.com/joliss).

## Installation

Add `fixturify` to your `Cargo.toml`:

```toml
[dependencies]
fixturify = { git = "https://github.com/malleatus/shared_binutils.git" }
```

## Usage

### Reading from a file

The `read` function reads a file from the given path and returns its contents
as a `BTreeMap<String, String>`.

#### Example

```rust
use std::collections::BTreeMap;
use std::path::Path;
use fixturify::read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new("example.txt");
    let contents: BTreeMap<String, String> = read(path)?;
    for (key, value) in contents.iter() {
        println!("{}: {}", key, value);
    }
    Ok(())
}
```

### Writing to a file

The `write` function writes the given `BTreeMap<String, String>` to a file at
the specified path.

#### Example

```rust
use std::collections::BTreeMap;
use std::path::Path;
use fixturify::write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new("example.txt");
    let mut contents = BTreeMap::new();
    contents.insert("key1".to_string(), "value1".to_string());
    contents.insert("key2".to_string(), "value2".to_string());
    write(path, &contents)?;
    Ok(())
}
```

## API

### `read`

Reads a file from the given path and returns its contents as a
`BTreeMap<String, String>`.

#### Parameters:

- `from`: A path to the file to be read. This can be any type that implements `AsRef<Path>`.

#### Returns:

- `Result<BTreeMap<String, String>>`: A result containing a `BTreeMap` with the
file's contents if successful, or an error if the operation fails.

### `write`

Writes the given `BTreeMap<String, String>` to a file at the specified path.

#### Parameters:

- `to`: A path to the file where the contents will be written. This can be any
type that implements `AsRef<Path>`.
- `contents`: A `BTreeMap<String, String>` containing the data to be written to
the file.

#### Returns:

- `Result<()>`: A result indicating success or failure of the write operation.

## License

This project is licensed under the MIT License. See the [LICENSE](../LICENSE) file
for details.
