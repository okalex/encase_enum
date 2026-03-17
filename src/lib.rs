pub use encase_enum_derive::ShaderEnum;

pub trait ShaderEnumType {
    type GpuRepr;
    fn to_gpu(&self) -> Self::GpuRepr;
}
