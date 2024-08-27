pub mod prelude;
// I don't particularly want to expose these as shell::*;
// Without this line, it works fine to use shell::prelude::* for cargo, but rust-analyzer complains
// it can't find the macro.
pub use prelude::*;
