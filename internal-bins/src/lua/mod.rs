use std::fs;
use std::path::Path;
use syn::{Attribute, File, Item, Type, TypePath};

pub fn process_file<S: AsRef<Path>>(input_path: S, output_path: S, identifiers: Vec<&str>) {
    let lua_types = generate_lua_types_from_file(input_path, &identifiers);
    fs::write(output_path, &lua_types).unwrap();
    println!("{}", lua_types);
}

fn generate_lua_types_from_file<S: AsRef<Path>>(file_path: S, identifiers: &[&str]) -> String {
    let content = fs::read_to_string(file_path).expect("Unable to read file");
    let syntax: File = syn::parse_file(&content).expect("Unable to parse file");

    let mut output = String::new();

    for item in syntax.items {
        if let Item::Struct(item_struct) = item {
            let struct_name = item_struct.ident.to_string();

            if identifiers.contains(&struct_name.as_str()) {
                output.push_str(&generate_lua_types_for_struct(&item_struct));
                output.push('\n');
            }
        } else if let Item::Enum(item_enum) = item {
            let enum_name = item_enum.ident.to_string();

            // Handle the specific Command enum or any enum you want to alias
            if identifiers.contains(&enum_name.as_str()) {
                output.push_str(&generate_lua_enum_alias(&item_enum));
                output.push('\n');
            }
        }
    }

    output
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
