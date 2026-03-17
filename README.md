# encase_enum

**⚠️ Not recommended for production use.** This is an experimental project and its API is subject to change without notice.

`encase_enum` adds enum support to [encase](https://crates.io/crates/encase), allowing Rust enums to be serialized into GPU buffers as tagged unions. It also includes a companion tool for generating WGSL type definitions from Rust source files.

## How It Works

Deriving `ShaderEnum` on an enum generates:

- A wrapper struct with a `u32` discriminant and a `data` field (an `array<vec4<f32>, N>` sized to fit the largest variant)
- `ShaderType`, `ShaderSize`, and `WriteInto` implementations, so the enum can be used directly as a field in other `ShaderType` structs
- A `ShaderEnumType` trait implementation with a `to_gpu()` method for explicit conversion

## Usage

### 1. Add dependencies

```toml
[dependencies]
encase_enum = { path = "../encase_enum" }
encase = "0.12"
glam = "0.29"
```

### 2. Derive `ShaderEnum` on your enums

```rust
use encase::ShaderType;
use encase_enum::ShaderEnum;

#[derive(ShaderType)]
pub struct Color {
    pub base: glam::Vec3,
}

#[derive(ShaderEnum)]
pub enum GpuTexture {
    SolidColor { albedo: Color },
    Checkerboard { color_a: Color, color_b: Color, scale: f32 },
}

#[derive(ShaderEnum)]
pub enum GpuMaterial {
    Lambertian { texture: GpuTexture },
    Metal { albedo: Color, fuzz: f32 },
    Dielectric { ir: f32 },
}
```

Enums with `ShaderEnum` can be used directly as fields in `#[derive(ShaderType)]` structs — no wrapper types or manual conversion needed.

## WGSL Generation

The `wgsl_autogen` tool scans Rust source files for types with `#[derive(ShaderType)]` or `#[derive(ShaderEnum)]` and generates corresponding WGSL struct definitions, discriminant constants, and wrapper structs. It also resolves type aliases (e.g. `type Vector = Vec3;`).

### As a build dependency (recommended)

Add to your project's `Cargo.toml`:

```toml
[build-dependencies]
wgsl_autogen = { path = "../encase_enum/wgsl_autogen" }
```

Then create a `build.rs`:

```rust
use std::path::PathBuf;

fn main() {
    let input = vec![PathBuf::from("src/")];
    let output = PathBuf::from("assets/shaders/types.wgsl");

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let wgsl = wgsl_autogen::generate_wgsl_from_files(&input)
        .expect("failed to generate WGSL");
    std::fs::write(&output, wgsl)
        .expect("failed to write WGSL file");

    println!("cargo:rerun-if-changed=src/");
}
```

### As a CLI tool

```sh
cargo install --path encase_enum/wgsl_autogen --features cli
wgsl_autogen --input src/ --output assets/shaders/types.wgsl
```

### Example output

Given the Rust types above, `wgsl_autogen` generates:

```wgsl
struct Color {
    base: vec3<f32>,
}

struct GpuTextureSolidColorData {
    albedo: Color,
}

struct GpuTextureCheckerboardData {
    color_a: Color,
    color_b: Color,
    scale: f32,
}

const GPUTEXTURE_SOLIDCOLOR: u32 = 0u;
const GPUTEXTURE_CHECKERBOARD: u32 = 1u;

struct GpuTexture {
    material_type: u32,
    data: array<vec4<f32>, 2>,
}
```

## Project Structure

- `src/` — Root crate: re-exports the `ShaderEnum` derive and defines the `ShaderEnumType` trait
- `derive/` — Proc-macro crate: implements the `ShaderEnum` derive macro
- `wgsl_autogen/` — WGSL generation tool (library + optional CLI binary)
