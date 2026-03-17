use std::collections::HashMap;
use syn::{Fields, File, Item, Type};

#[derive(Debug, Clone)]
pub struct ParsedStruct {
    pub name: String,
    pub fields: Vec<(String, RustType)>,
}

#[derive(Debug, Clone)]
pub struct ParsedEnum {
    pub name: String,
    pub variants: Vec<ParsedVariant>,
}

#[derive(Debug, Clone)]
pub struct ParsedVariant {
    pub name: String,
    pub fields: Vec<(String, RustType)>,
}

#[derive(Debug, Clone)]
pub enum RustType {
    Primitive(String),
    Named(String),
    Array(Box<RustType>, usize),
    RuntimeArray(Box<RustType>),
}

fn has_derive(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("derive") {
            return false;
        }
        let Ok(meta_list) = attr.meta.require_list() else {
            return false;
        };
        let tokens = meta_list.tokens.to_string();
        tokens.split(',').any(|t| {
            let t = t.trim();
            t == name || t.ends_with(&format!("::{}", name))
        })
    })
}

fn parse_type(ty: &Type) -> RustType {
    match ty {
        Type::Path(type_path) => {
            let segments: Vec<_> = type_path.path.segments.iter().collect();
            let last = segments.last().unwrap();
            let name = last.ident.to_string();

            // Handle Vec<T> as a runtime-sized array
            if name == "Vec" {
                if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return RustType::RuntimeArray(Box::new(parse_type(inner)));
                    }
                }
            }

            name_to_rust_type(&name)
        }
        Type::Array(arr) => {
            let elem = parse_type(&arr.elem);
            if let syn::Expr::Lit(lit) = &arr.len {
                if let syn::Lit::Int(int) = &lit.lit {
                    let n: usize = int.base10_parse().unwrap_or(0);
                    return RustType::Array(Box::new(elem), n);
                }
            }
            // Non-literal array length — treat as named type
            RustType::Named(format!("array_{}", 0))
        }
        _ => RustType::Named("unknown".to_string()),
    }
}

fn name_to_rust_type(name: &str) -> RustType {
    match name {
        "f32" | "f64" | "u32" | "i32" | "u16" | "i16" | "bool" => {
            RustType::Primitive(name.to_string())
        }
        "Vec2" | "Vec3" | "Vec4" | "UVec2" | "UVec3" | "UVec4" | "IVec2" | "IVec3" | "IVec4"
        | "Mat2" | "Mat3" | "Mat4" => RustType::Primitive(name.to_string()),
        _ => RustType::Named(name.to_string()),
    }
}

fn extract_fields(fields: &Fields) -> Vec<(String, RustType)> {
    match fields {
        Fields::Named(f) => f
            .named
            .iter()
            .map(|field| {
                let name = field.ident.as_ref().unwrap().to_string();
                let ty = parse_type(&field.ty);
                (name, ty)
            })
            .collect(),
        Fields::Unnamed(f) => f
            .unnamed
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let name = format!("field{}", i);
                let ty = parse_type(&field.ty);
                (name, ty)
            })
            .collect(),
        Fields::Unit => vec![],
    }
}

pub fn extract_types(
    file: &File,
) -> (
    Vec<ParsedStruct>,
    Vec<ParsedEnum>,
    HashMap<String, RustType>,
) {
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut aliases = HashMap::new();

    // First pass: collect type aliases
    for item in &file.items {
        if let Item::Type(t) = item {
            let name = t.ident.to_string();
            let resolved = parse_type(&t.ty);
            aliases.insert(name, resolved);
        }
    }

    for item in &file.items {
        match item {
            Item::Struct(s) => {
                if has_derive(&s.attrs, "ShaderType") || has_derive(&s.attrs, "WgslStruct") {
                    structs.push(ParsedStruct {
                        name: s.ident.to_string(),
                        fields: extract_fields(&s.fields),
                    });
                }
            }
            Item::Enum(e) => {
                if has_derive(&e.attrs, "ShaderEnum") {
                    let variants = e
                        .variants
                        .iter()
                        .map(|v| ParsedVariant {
                            name: v.ident.to_string(),
                            fields: extract_fields(&v.fields),
                        })
                        .collect();
                    enums.push(ParsedEnum {
                        name: e.ident.to_string(),
                        variants,
                    });
                }
            }
            _ => {}
        }
    }

    (structs, enums, aliases)
}

fn resolve_type(ty: &RustType, aliases: &HashMap<String, RustType>) -> RustType {
    match ty {
        RustType::Named(name) => {
            if let Some(resolved) = aliases.get(name) {
                resolve_type(resolved, aliases)
            } else {
                ty.clone()
            }
        }
        RustType::Array(elem, size) => {
            RustType::Array(Box::new(resolve_type(elem, aliases)), *size)
        }
        RustType::RuntimeArray(elem) => {
            RustType::RuntimeArray(Box::new(resolve_type(elem, aliases)))
        }
        _ => ty.clone(),
    }
}

pub fn resolve_aliases(
    structs: &mut [ParsedStruct],
    enums: &mut [ParsedEnum],
    aliases: &HashMap<String, RustType>,
) {
    for s in structs.iter_mut() {
        for (_, ty) in s.fields.iter_mut() {
            *ty = resolve_type(ty, aliases);
        }
    }
    for e in enums.iter_mut() {
        for v in e.variants.iter_mut() {
            for (_, ty) in v.fields.iter_mut() {
                *ty = resolve_type(ty, aliases);
            }
        }
    }
}
