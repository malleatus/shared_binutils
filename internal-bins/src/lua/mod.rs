use std::fs;
use std::path::Path;
use syn::{punctuated::Punctuated, Attribute, File, Item, Token, Type, TypePath};

pub fn process_file<S: AsRef<Path>>(input_path: S, output_path: S) {
    let lua_types = generate_lua_types_from_file(input_path);
    fs::write(output_path, &lua_types).unwrap();
    println!("{}", lua_types);
}

fn generate_lua_types_from_file<S: AsRef<Path>>(file_path: S) -> String {
    let content = fs::read_to_string(file_path).expect("Unable to read file");
    let syntax: File = syn::parse_file(&content).expect("Unable to parse file");

    let mut output = String::new();

    for item in syntax.items {
        if let Item::Struct(item_struct) = item {
            if has_derive_deserialize(&item_struct.attrs) {
                output.push_str(&generate_lua_types_for_struct(&item_struct));
                output.push('\n');
            }
        } else if let Item::Enum(item_enum) = item {
            if has_derive_deserialize(&item_enum.attrs) {
                output.push_str(&generate_lua_enum_alias(&item_enum));
                output.push('\n');
            }
        }
    }

    output
}

fn has_derive_deserialize(attrs: &[Attribute]) -> bool {
    for attr in attrs {
        if attr.path().is_ident("derive") {
            let result = attr.parse_args_with(|input: syn::parse::ParseStream| {
                let paths = Punctuated::<syn::Path, Token![,]>::parse_terminated(input)?;
                for path in paths {
                    if path.is_ident("Deserialize") {
                        return Ok(true);
                    }
                }
                Ok(false)
            });
            if let Ok(true) = result {
                return true;
            }
        }
    }
    false
}

fn generate_lua_types_for_struct(item_struct: &syn::ItemStruct) -> String {
    let struct_name = &item_struct.ident.to_string();
    let mut lua_type_def = String::new();

    if let Some(doc) = extract_docs(&item_struct.attrs) {
        lua_type_def.push_str(&format!("--- {}\n", doc));
    }

    lua_type_def.push_str(&format!("---@class {}\n", struct_name));

    if let syn::Fields::Named(fields) = &item_struct.fields {
        for field in fields.named.iter() {
            if let Some(field_doc) = extract_docs(&field.attrs) {
                lua_type_def.push_str(&format!("--- {}\n", field_doc));
            }

            let field_name = field.ident.as_ref().unwrap().to_string();
            let lua_type = get_lua_type(&field.ty);
            lua_type_def.push_str(&format!("---@field {} {}\n", field_name, lua_type));
        }
    }

    lua_type_def
}

fn extract_docs(attrs: &[Attribute]) -> Option<String> {
    let docs: Vec<String> = attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(meta) = attr.meta.clone() {
                    if let syn::Expr::Lit(expr_lit) = meta.value {
                        if let syn::Lit::Str(lit) = expr_lit.lit {
                            return Some(lit.value());
                        }
                    }
                }
            }
            None
        })
        .collect();

    if docs.is_empty() {
        None
    } else {
        Some(docs.join(" "))
    }
}

fn get_lua_type(ty: &Type) -> String {
    if let Type::Path(TypePath { path, .. }) = ty {
        let segment = &path.segments.last().unwrap().ident.to_string();

        match segment.as_str() {
            "String" => "string".to_string(),
            "Option" => {
                // Handle Option<T>
                if let Some(inner_type) = get_generic_type_arg(path) {
                    format!("{}|nil", get_lua_type(&inner_type))
                } else {
                    "any|nil".to_string()
                }
            }
            "Vec" => {
                // Handle Vec<T>
                if let Some(inner_type) = get_generic_type_arg(path) {
                    format!("{}[]", get_lua_type(&inner_type))
                } else {
                    "any[]".to_string()
                }
            }
            "HashMap" => {
                // Handle HashMap<K, V>
                if let Some((key_type, value_type)) = get_map_type_args(path) {
                    format!(
                        "table<{}, {}>",
                        get_lua_type(&key_type),
                        get_lua_type(&value_type)
                    )
                } else {
                    "table<any, any>".to_string()
                }
            }
            "BTreeMap" => {
                // Handle BTreeMap<K, V>
                if let Some((key_type, value_type)) = get_map_type_args(path) {
                    format!(
                        "table<{}, {}>",
                        get_lua_type(&key_type),
                        get_lua_type(&value_type)
                    )
                } else {
                    "table<any, any>".to_string()
                }
            }
            "PathBuf" => "string".to_string(), // Treat PathBuf as a string
            _ => segment.clone(),              // Fallback: use the Rust type name directly
        }
    } else {
        "unknown".to_string() // Fallback type
    }
}

// Helper function to extract the generic type argument (for Option<T> and Vec<T>)
fn get_generic_type_arg(path: &syn::Path) -> Option<Type> {
    if let Some(segment) = path.segments.last() {
        if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
            if let Some(syn::GenericArgument::Type(ty)) = args.args.first() {
                return Some(ty.clone());
            }
        }
    }
    None
}

// Helper function to extract key-value types for maps (e.g., HashMap<K, V>)
fn get_map_type_args(path: &syn::Path) -> Option<(Type, Type)> {
    if let Some(segment) = path.segments.last() {
        if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
            let mut types = args.args.iter().filter_map(|arg| {
                if let syn::GenericArgument::Type(ty) = arg {
                    Some(ty.clone())
                } else {
                    None
                }
            });

            let key_type = types.next()?;
            let value_type = types.next()?;
            return Some((key_type, value_type));
        }
    }
    None
}

fn generate_lua_enum_alias(item_enum: &syn::ItemEnum) -> String {
    let enum_name = &item_enum.ident.to_string();
    let mut alias_definition = String::new();

    let mut variant_types = vec![];

    for variant in &item_enum.variants {
        match &variant.fields {
            syn::Fields::Unit => {
                // Unit variant, detect underlying type if possible
                todo!("Unit variants are not supported yet")
            }
            syn::Fields::Unnamed(fields) => {
                // Tuple variant (e.g., Variant2(String, i32))
                if fields.unnamed.len() == 1 {
                    // Single-field tuple variant
                    let field_type = get_lua_type(&fields.unnamed[0].ty);
                    variant_types.push(field_type);
                } else {
                    // Multi-field tuple variant, treat as a tuple
                    let tuple_type = fields
                        .unnamed
                        .iter()
                        .map(|field| get_lua_type(&field.ty))
                        .collect::<Vec<_>>()
                        .join(", ");
                    variant_types.push(format!("({})", tuple_type));
                }
            }
            syn::Fields::Named(fields) => {
                // Struct variant (e.g., Variant3 { field1: String, field2: i32 })
                let struct_fields = fields
                    .named
                    .iter()
                    .map(|field| {
                        let field_name = field.ident.as_ref().unwrap().to_string();
                        let field_type = get_lua_type(&field.ty);
                        format!("{}: {}", field_name, field_type)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                variant_types.push(format!("{{ {} }}", struct_fields)); // Represent as a Lua table
            }
        }
    }

    // Deduplicate and sort the types
    variant_types.sort();
    variant_types.dedup();

    // Join all the types into a single alias
    alias_definition.push_str(&format!(
        "---@alias {} {}\n",
        enum_name,
        variant_types.join("|")
    ));

    alias_definition
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use tempfile::tempdir;

    #[test]
    fn test_run_generates_lua_types() {
        // Create a temporary directory
        let temp_dir = tempdir().expect("Failed to create temp dir");
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();

        let source_file = temp_dir.path().join("src/lib.rs");
        fs::write(
            &source_file,
            r###"
/// Configuration for the application.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Optional tmux configuration. Including sessions and windows to be created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmux: Option<Tmux>,

    /// Optional configuration for cache-shell-setup
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_caching: Option<ShellCache>,

    /// Optional list of crate locations (used as a lookup path for tmux windows `linked_crates`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_locations: Option<Vec<String>>,
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

    /// The names of any of the workspaces crates that provide binaries that should be available on
    /// $PATH inside the new window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_crates: Option<Vec<String>>,
}
        "###,
        )
        .unwrap();

        let output_path = temp_dir.path().join("init.lua");

        process_file(source_file, output_path.clone());

        assert_snapshot!(fs::read_to_string(output_path).unwrap(), @r###"
        ---  Configuration for the application.
        ---@class Config
        ---  Optional tmux configuration. Including sessions and windows to be created.
        ---@field tmux Tmux|nil
        ---  Optional configuration for cache-shell-setup
        ---@field shell_caching ShellCache|nil
        ---  Optional list of crate locations (used as a lookup path for tmux windows `linked_crates`)
        ---@field crate_locations string[]|nil

        ---@class ShellCache
        ---@field source string
        ---@field destination string

        ---  Tmux configuration.
        ---@class Tmux
        ---  List of tmux sessions.
        ---@field sessions Session[]
        ---  The default session to attach to when `startup-tmux --attach` is ran.
        ---@field default_session string|nil

        ---  Configuration for a tmux session.
        ---@class Session
        ---  Name of the session.
        ---@field name string
        ---  List of windows in the session.
        ---@field windows Window[]

        ---@alias Command string|string[]

        ---  Configuration for a tmux window.
        ---@class Window
        ---  Name of the window.
        ---@field name string
        ---  Optional path to set as the working directory for the window.
        ---@field path string|nil
        ---  Optional command to run in the window.
        ---@field command Command|nil
        ---  Additional environment variables to set in the window.
        ---@field env table<string, string>|nil
        ---  The names of any of the workspaces crates that provide binaries that should be available on  $PATH inside the new window.
        ---@field linked_crates string[]|nil

        "###);
    }
}
