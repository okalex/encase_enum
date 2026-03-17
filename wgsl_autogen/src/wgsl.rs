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
        RustType::RuntimeArray(elem) => {
            format!("array<{}>", rust_type_to_wgsl(elem))
        }
    }
}

fn round_up(val: usize, align: usize) -> usize {
    (val + align - 1) / align * align
}

/// Compute WGSL (align, size) for a type following the WGSL alignment spec.
fn wgsl_align_size(ty: &RustType, structs: &[ParsedStruct], enums: &[ParsedEnum]) -> (usize, usize) {
    match ty {
        RustType::Primitive(name) => match name.as_str() {
            "f32" | "u32" | "i32" | "bool" | "u16" | "i16" => (4, 4),
            "f64" => (8, 8),
            "Vec2" | "UVec2" | "IVec2" => (8, 8),
            "Vec3" | "UVec3" | "IVec3" => (16, 12),
            "Vec4" | "UVec4" | "IVec4" => (16, 16),
            "Mat2" => (8, 16),   // 2 columns of vec2
            "Mat3" => (16, 48),  // 3 columns of vec3 (each padded to 16)
            "Mat4" => (16, 64),  // 4 columns of vec4
            _ => (4, 4),
        },
        RustType::Named(name) => {
            if let Some(s) = structs.iter().find(|s| s.name == *name) {
                struct_align_size(&s.fields, structs, enums)
            } else if let Some(e) = enums.iter().find(|e| e.name == *name) {
                // Enum wrapper: { material_type: u32, data: array<vec4<f32>, N> }
                // u32 at offset 0 (align 4), array at offset 16 (align 16)
                let max = max_variant_size(&e.variants, structs, enums);
                let vec4s = if max == 0 { 1 } else { (max + 15) / 16 };
                (16, 16 + vec4s * 16)
            } else {
                (4, 4)
            }
        }
        RustType::Array(elem, count) => {
            let (ea, es) = wgsl_align_size(elem, structs, enums);
            let stride = round_up(es, ea);
            (ea, stride * count)
        }
        RustType::RuntimeArray(elem) => {
            let (ea, _) = wgsl_align_size(elem, structs, enums);
            (ea, 0) // runtime-sized, no static size
        }
    }
}

fn struct_align_size(
    fields: &[(String, RustType)],
    structs: &[ParsedStruct],
    enums: &[ParsedEnum],
) -> (usize, usize) {
    let mut max_align = 1;
    let mut offset = 0;
    for (_, ty) in fields {
        let (align, size) = wgsl_align_size(ty, structs, enums);
        if align > max_align {
            max_align = align;
        }
        offset = round_up(offset, align);
        offset += size;
    }
    (max_align, round_up(offset, max_align))
}

fn max_variant_size(
    variants: &[ParsedVariant],
    structs: &[ParsedStruct],
    enums: &[ParsedEnum],
) -> usize {
    variants
        .iter()
        .map(|v| {
            let (_, size) = struct_align_size(&v.fields, structs, enums);
            size
        })
        .max()
        .unwrap_or(0)
}

// --- Unpack expression generation ---

fn unpack_field_expr(
    ty: &RustType,
    data_var: &str,
    offset: usize,
    structs: &[ParsedStruct],
    enums: &[ParsedEnum],
) -> String {
    match ty {
        RustType::Primitive(name) => {
            let vi = offset / 16;
            let ci = (offset % 16) / 4;
            let comp = ["x", "y", "z", "w"][ci];
            match name.as_str() {
                "f32" => format!("{data_var}[{vi}].{comp}"),
                "u32" | "u16" | "i16" => format!("bitcast<u32>({data_var}[{vi}].{comp})"),
                "i32" => format!("bitcast<i32>({data_var}[{vi}].{comp})"),
                "bool" => format!("(bitcast<u32>({data_var}[{vi}].{comp}) != 0u)"),
                "Vec2" => {
                    let comps = if ci == 0 { "xy" } else { "zw" };
                    format!("{data_var}[{vi}].{comps}")
                }
                "UVec2" => {
                    let comps = if ci == 0 { "xy" } else { "zw" };
                    format!("bitcast<vec2<u32>>({data_var}[{vi}].{comps})")
                }
                "IVec2" => {
                    let comps = if ci == 0 { "xy" } else { "zw" };
                    format!("bitcast<vec2<i32>>({data_var}[{vi}].{comps})")
                }
                "Vec3" => format!("{data_var}[{vi}].xyz"),
                "UVec3" => format!("bitcast<vec3<u32>>({data_var}[{vi}].xyz)"),
                "IVec3" => format!("bitcast<vec3<i32>>({data_var}[{vi}].xyz)"),
                "Vec4" => format!("{data_var}[{vi}]"),
                "UVec4" => format!("bitcast<vec4<u32>>({data_var}[{vi}])"),
                "IVec4" => format!("bitcast<vec4<i32>>({data_var}[{vi}])"),
                "Mat2" => {
                    let c0 = if ci == 0 {
                        format!("{data_var}[{vi}].xy")
                    } else {
                        format!("{data_var}[{vi}].zw")
                    };
                    let c1_offset = offset + 8;
                    let c1_vi = c1_offset / 16;
                    let c1_ci = (c1_offset % 16) / 4;
                    let c1 = if c1_ci == 0 {
                        format!("{data_var}[{c1_vi}].xy")
                    } else {
                        format!("{data_var}[{c1_vi}].zw")
                    };
                    format!("mat2x2<f32>({c0}, {c1})")
                }
                "Mat3" => {
                    format!(
                        "mat3x3<f32>({data_var}[{vi}].xyz, {data_var}[{}].xyz, {data_var}[{}].xyz)",
                        vi + 1,
                        vi + 2
                    )
                }
                "Mat4" => {
                    format!(
                        "mat4x4<f32>({data_var}[{vi}], {data_var}[{}], {data_var}[{}], {data_var}[{}])",
                        vi + 1,
                        vi + 2,
                        vi + 3
                    )
                }
                _ => format!("{data_var}[{vi}].{comp}"),
            }
        }
        RustType::Named(name) => {
            if let Some(s) = structs.iter().find(|s| s.name == *name) {
                let mut args = Vec::new();
                let mut field_offset = offset;
                for (_, field_ty) in &s.fields {
                    let (align, size) = wgsl_align_size(field_ty, structs, enums);
                    field_offset = round_up(field_offset, align);
                    args.push(unpack_field_expr(field_ty, data_var, field_offset, structs, enums));
                    field_offset += size;
                }
                format!("{}({})", name, args.join(", "))
            } else if let Some(e) = enums.iter().find(|e| e.name == *name) {
                // Nested enum: { material_type: u32 at offset, data: array<vec4, N> at round_up(offset+4, 16) }
                let max = max_variant_size(&e.variants, structs, enums);
                let vec4s = if max == 0 { 1 } else { (max + 15) / 16 };
                let mt_vi = offset / 16;
                let mt_ci = (offset % 16) / 4;
                let mt_comp = ["x", "y", "z", "w"][mt_ci];
                let mt_expr = format!("bitcast<u32>({data_var}[{mt_vi}].{mt_comp})");
                let data_offset = round_up(offset + 4, 16);
                let data_vi = data_offset / 16;
                let inner: Vec<String> = (0..vec4s)
                    .map(|i| format!("{data_var}[{}]", data_vi + i))
                    .collect();
                format!(
                    "{}({}, array<vec4<f32>, {}>({}))",
                    name,
                    mt_expr,
                    vec4s,
                    inner.join(", ")
                )
            } else {
                format!("/* unknown type: {} */", name)
            }
        }
        RustType::Array(elem, count) => {
            let (ea, es) = wgsl_align_size(elem, structs, enums);
            let stride = round_up(es, ea);
            let elems: Vec<String> = (0..*count)
                .map(|i| unpack_field_expr(elem, data_var, offset + i * stride, structs, enums))
                .collect();
            format!(
                "array<{}, {}>({})",
                rust_type_to_wgsl(elem),
                count,
                elems.join(", ")
            )
        }
        RustType::RuntimeArray(_) => {
            "/* runtime-sized array — cannot unpack statically */".to_string()
        }
    }
}

fn generate_unpack_fns(enums: &[ParsedEnum], structs: &[ParsedStruct]) -> String {
    let mut output = String::new();

    // Unpack functions for ShaderType structs (skip if has runtime-sized arrays)
    for s in structs {
        let has_runtime = s.fields.iter().any(|(_, ty)| matches!(ty, RustType::RuntimeArray(_)));
        if has_runtime {
            continue;
        }

        let (_, total_size) = struct_align_size(&s.fields, structs, enums);
        let vec4s = if total_size == 0 { 1 } else { (total_size + 15) / 16 };

        let mut args = Vec::new();
        let mut offset = 0usize;
        for (_, ty) in &s.fields {
            let (align, size) = wgsl_align_size(ty, structs, enums);
            offset = round_up(offset, align);
            args.push(unpack_field_expr(ty, "data", offset, structs, enums));
            offset += size;
        }

        output.push_str(&format!(
            "fn unpack_{}(data: array<vec4<f32>, {}>) -> {} {{\n    return {}({});\n}}\n\n",
            s.name, vec4s, s.name, s.name, args.join(", ")
        ));
    }

    // Unpack functions for enum variants
    for e in enums {
        for variant in &e.variants {
            if variant.fields.is_empty() {
                continue;
            }
            let fn_name = format!("unpack_{}_{}", e.name, variant.name);
            let wrapper_name = &e.name;
            let data_name = format!("{}{}Data", e.name, variant.name);

            let mut args = Vec::new();
            let mut offset = 0usize;
            for (_, ty) in &variant.fields {
                let (align, size) = wgsl_align_size(ty, structs, enums);
                offset = round_up(offset, align);
                args.push(unpack_field_expr(ty, "v.data", offset, structs, enums));
                offset += size;
            }

            output.push_str(&format!(
                "fn {}(v: {}) -> {} {{\n    return {}({});\n}}\n\n",
                fn_name, wrapper_name, data_name, data_name, args.join(", ")
            ));
        }
    }
    output
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
            RustType::RuntimeArray(elem) => match elem.as_ref() {
                RustType::Named(name) => Some(name.clone()),
                _ => None,
            },
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

pub fn generate_wgsl(structs: &[ParsedStruct], enums: &[ParsedEnum], define_import_path: bool) -> String {
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
    if define_import_path {
        output.push_str("#define_import_path types\n\n");
    }
    output.push_str("// Auto-generated by wgsl_autogen — do not edit\n\n");
    for (i, name) in sorted.iter().enumerate() {
        if let Some(def) = definitions.get(name) {
            if i > 0 {
                output.push_str("\n\n");
            }
            output.push_str(def);
        }
    }

    // Generate unpack functions
    let unpack = generate_unpack_fns(enums, structs);
    if !unpack.is_empty() {
        output.push_str("\n\n");
        output.push_str(&unpack.trim_end());
    }

    output.push('\n');
    output
}
