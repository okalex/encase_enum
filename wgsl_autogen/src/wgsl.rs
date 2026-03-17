use std::collections::HashSet;

use crate::parser::{ParsedEnum, ParsedStruct, ParsedVariant, RustType};

fn rust_type_to_wgsl(ty: &RustType) -> String {
    match ty {
        RustType::Primitive(name) => match name.as_str() {
            "f32" => "f32".to_string(),
            "f64" => "f64".to_string(),
            "u32" => "u32".to_string(),
            "i32" => "i32".to_string(),
            "u16" => "u32".to_string(),
            "i16" => "i32".to_string(),
            "bool" => "bool".to_string(),
            "Vec2" => "vec2<f32>".to_string(),
            "Vec3" => "vec3<f32>".to_string(),
            "Vec4" => "vec4<f32>".to_string(),
            "UVec2" => "vec2<u32>".to_string(),
            "UVec3" => "vec3<u32>".to_string(),
            "UVec4" => "vec4<u32>".to_string(),
            "IVec2" => "vec2<i32>".to_string(),
            "IVec3" => "vec3<i32>".to_string(),
            "IVec4" => "vec4<i32>".to_string(),
            "Mat2" => "mat2x2<f32>".to_string(),
            "Mat3" => "mat3x3<f32>".to_string(),
            "Mat4" => "mat4x4<f32>".to_string(),
            other => other.to_string(),
        },
        RustType::Named(name) => name.clone(),
        RustType::Array(elem, size) => {
            format!("array<{}, {}>", rust_type_to_wgsl(elem), size)
        }
    }
}

/// Compute the WGSL size of a type in bytes (for encase layout estimation).
fn wgsl_type_size(ty: &RustType, structs: &[ParsedStruct], enums: &[ParsedEnum]) -> usize {
    match ty {
        RustType::Primitive(name) => match name.as_str() {
            "f32" | "u32" | "i32" | "bool" => 4,
            "f64" => 8,
            "u16" | "i16" => 4, // padded to 4 in WGSL
            "Vec2" | "UVec2" | "IVec2" => 8,
            "Vec3" | "UVec3" | "IVec3" => 12,
            "Vec4" | "UVec4" | "IVec4" => 16,
            "Mat2" => 16,  // 2 * vec2, but with padding: 2 * 8
            "Mat3" => 48,  // 3 * vec3 padded to vec4 = 3 * 16
            "Mat4" => 64,  // 4 * vec4
            _ => 4,
        },
        RustType::Named(name) => {
            // Look up in structs
            if let Some(s) = structs.iter().find(|s| s.name == *name) {
                struct_size(&s.fields, structs, enums)
            } else if let Some(e) = enums.iter().find(|e| e.name == *name) {
                // ShaderEnum: material_type (u32) + data (vec4 array)
                let max = max_variant_size(&e.variants, structs, enums);
                let vec4s = (max + 15) / 16;
                4 + vec4s * 16 // u32 + array<vec4<f32>, N>
            } else {
                4 // fallback
            }
        }
        RustType::Array(elem, count) => {
            wgsl_type_size(elem, structs, enums) * count
        }
    }
}

fn struct_size(fields: &[(String, RustType)], structs: &[ParsedStruct], enums: &[ParsedEnum]) -> usize {
    // Simplified: sum of field sizes (encase handles alignment)
    fields.iter().map(|(_, ty)| wgsl_type_size(ty, structs, enums)).sum()
}

fn max_variant_size(variants: &[ParsedVariant], structs: &[ParsedStruct], enums: &[ParsedEnum]) -> usize {
    variants
        .iter()
        .map(|v| struct_size(&v.fields, structs, enums))
        .max()
        .unwrap_or(0)
}

fn generate_struct_wgsl(name: &str, fields: &[(String, RustType)]) -> String {
    let mut out = format!("struct {} {{\n", name);
    for (field_name, ty) in fields {
        out.push_str(&format!("    {}: {},\n", field_name, rust_type_to_wgsl(ty)));
    }
    out.push('}');
    out
}

fn dependency_names(fields: &[(String, RustType)]) -> Vec<String> {
    fields
        .iter()
        .filter_map(|(_, ty)| match ty {
            RustType::Named(name) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// Topologically sort type definitions so dependencies come first.
fn topo_sort(
    names: &[String],
    deps: &std::collections::HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut visited = HashSet::new();
    let mut result = Vec::new();
    let name_set: HashSet<_> = names.iter().cloned().collect();

    fn visit(
        name: &str,
        deps: &std::collections::HashMap<String, Vec<String>>,
        name_set: &HashSet<String>,
        visited: &mut HashSet<String>,
        result: &mut Vec<String>,
    ) {
        if visited.contains(name) {
            return;
        }
        visited.insert(name.to_string());
        if let Some(dep_list) = deps.get(name) {
            for dep in dep_list {
                if name_set.contains(dep) {
                    visit(dep, deps, name_set, visited, result);
                }
            }
        }
        result.push(name.to_string());
    }

    for name in names {
        visit(name, deps, &name_set, &mut visited, &mut result);
    }

    result
}

pub fn generate_wgsl(structs: &[ParsedStruct], enums: &[ParsedEnum]) -> String {
    let mut definitions: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut deps: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut all_names: Vec<String> = Vec::new();

    // Process regular structs
    for s in structs {
        let wgsl = generate_struct_wgsl(&s.name, &s.fields);
        let dep_names = dependency_names(&s.fields);
        definitions.insert(s.name.clone(), wgsl);
        deps.insert(s.name.clone(), dep_names);
        all_names.push(s.name.clone());
    }

    // Process enums
    for e in enums {
        let wrapper_name = e.name.clone();
        let mut enum_deps = Vec::new();

        // Variant data structs
        for variant in e.variants.iter() {
            if !variant.fields.is_empty() {
                let vs_name = format!("{}{}Data", e.name, variant.name);
                let wgsl = generate_struct_wgsl(&vs_name, &variant.fields);
                let dep_names = dependency_names(&variant.fields);
                enum_deps.extend(dep_names.clone());
                definitions.insert(vs_name.clone(), wgsl);
                deps.insert(vs_name.clone(), dep_names);
                all_names.push(vs_name.clone());
                enum_deps.push(vs_name);
            }
        }

        // Discriminant constants
        let mut const_block = String::new();
        for (i, variant) in e.variants.iter().enumerate() {
            const_block.push_str(&format!(
                "const {}_{}: u32 = {}u;\n",
                e.name.to_uppercase(),
                variant.name.to_uppercase(),
                i
            ));
        }

        // Wrapper struct
        let max = max_variant_size(&e.variants, structs, enums);
        let vec4s = if max == 0 { 1 } else { (max + 15) / 16 };
        let wrapper_wgsl = format!(
            "{}struct {} {{\n    material_type: u32,\n    data: array<vec4<f32>, {}>,\n}}",
            if const_block.is_empty() { String::new() } else { format!("{}\n", const_block) },
            wrapper_name,
            vec4s
        );

        definitions.insert(wrapper_name.clone(), wrapper_wgsl);
        deps.insert(wrapper_name.clone(), enum_deps);
        all_names.push(wrapper_name);
    }

    // Topologically sort and output
    let sorted = topo_sort(&all_names, &deps);
    let mut output = String::new();
    output.push_str("// Auto-generated by wgsl_autogen — do not edit\n\n");
    for (i, name) in sorted.iter().enumerate() {
        if let Some(def) = definitions.get(name) {
            if i > 0 {
                output.push_str("\n\n");
            }
            output.push_str(def);
        }
    }
    output.push('\n');
    output
}
