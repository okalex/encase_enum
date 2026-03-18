use proc_macro::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[proc_macro_derive(ShaderEnum)]
pub fn derive_encase_enum(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let enum_name = &input.ident;

    // Only works on enums
    let Data::Enum(data_enum) = &input.data else {
        panic!("ShaderEnum can only be derived on enums");
    };

    // Generate a struct per variant to leverage encase's size calculation
    let variant_struct_idents: Vec<_> = data_enum
        .variants
        .iter()
        .map(|v| {
            let name = format!("{}{}Data", enum_name, v.ident);
            syn::Ident::new(&name, v.ident.span())
        })
        .collect();

    let variant_structs = data_enum
        .variants
        .iter()
        .zip(variant_struct_idents.iter())
        .map(|(variant, struct_ident)| {
            let fields = match &variant.fields {
                Fields::Named(f) => {
                    let fs = f.named.iter().map(|field| {
                        let ident = &field.ident;
                        let ty = &field.ty;
                        quote! { #ident: #ty }
                    });
                    quote! { #(#fs,)* }
                }
                Fields::Unnamed(f) => {
                    let fs = f.unnamed.iter().enumerate().map(|(i, field)| {
                        let ident = syn::Ident::new(&format!("field{}", i), field.span());
                        let ty = &field.ty;
                        quote! { #ident: #ty }
                    });
                    quote! { #(#fs,)* }
                }
                Fields::Unit => quote! {},
            };
            quote! {
                #[derive(encase::ShaderType)]
                pub struct #struct_ident {
                    #fields
                }
            }
        });

    let struct_ident = syn::Ident::new(&format!("ShaderEnum{}", enum_name), enum_name.span());

    // Generate the size constant — max across all variant structs
    let size_expr = variant_struct_idents.iter().map(|ident| {
        quote! { <#ident as encase::ShaderType>::METADATA.min_size().get() }
    });

    // Generate From impl match arms
    let from_arms = data_enum
        .variants
        .iter()
        .zip(variant_struct_idents.iter())
        .enumerate()
        .map(|(i, (variant, vs_ident))| {
            let variant_ident = &variant.ident;
            let discriminant = i as u32;

            let (fields_pattern, write_stmts) = match &variant.fields {
                Fields::Named(f) => {
                    let field_names: Vec<_> =
                        f.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
                    let field_types: Vec<_> = f.named.iter().map(|f| &f.ty).collect();
                    let writes = field_names.iter().zip(field_types.iter()).enumerate().map(|(idx, (ident, ty))| {
                        let idx = syn::Index::from(idx);
                        quote! {
                            <#ty as encase::internal::WriteInto>::write_into(#ident, &mut writer);
                            writer.advance(<#vs_ident as encase::ShaderType>::METADATA.padding(#idx) as usize);
                        }
                    });
                    (quote! { { #(#field_names,)* } }, quote! { #(#writes)* })
                }
                Fields::Unnamed(f) => {
                    let field_names: Vec<_> = (0..f.unnamed.len())
                        .map(|i| syn::Ident::new(&format!("field{}", i), enum_name.span()))
                        .collect();
                    let field_types: Vec<_> = f.unnamed.iter().map(|f| &f.ty).collect();
                    let writes = field_names.iter().zip(field_types.iter()).enumerate().map(|(idx, (ident, ty))| {
                        let idx = syn::Index::from(idx);
                        quote! {
                            <#ty as encase::internal::WriteInto>::write_into(#ident, &mut writer);
                            writer.advance(<#vs_ident as encase::ShaderType>::METADATA.padding(#idx) as usize);
                        }
                    });
                    (quote! { (#(#field_names,)*) }, quote! { #(#writes)* })
                }
                Fields::Unit => (quote! {}, quote! {}),
            };

            quote! {
                #enum_name::#variant_ident #fields_pattern => {
                    let scratch: #vs_ident = unsafe { core::mem::zeroed() };
                    let mut writer = encase::internal::Writer::new(
                        &scratch,
                        &mut data_bytes,
                        0
                    ).unwrap();
                    core::mem::forget(scratch);
                    #write_stmts
                    #discriminant
                }
            }
        });

    let max_variant_size_fn = syn::Ident::new(
        &format!("__shader_enum_{}_max_variant_size", enum_name).to_lowercase(),
        enum_name.span(),
    );
    let vec4s_needed_fn = syn::Ident::new(
        &format!("__shader_enum_{}_vec4s_needed", enum_name).to_lowercase(),
        enum_name.span(),
    );

    let expanded = quote! {
        #(#variant_structs)*

        const fn #max_variant_size_fn() -> u64 {
            let sizes = [#(#size_expr,)*];
            let mut max = 0u64;
            let mut i = 0;
            while i < sizes.len() {
                if sizes[i] > max { max = sizes[i]; }
                i += 1;
            }
            max
        }

        const fn #vec4s_needed_fn() -> usize {
            ((#max_variant_size_fn() + 15) / 16) as usize
        }

        #[derive(encase::ShaderType)]
        pub struct #struct_ident {
            pub material_type: u32,
            pub data: [glam::Vec4; #vec4s_needed_fn()],
        }

        impl From<&#enum_name> for #struct_ident {
            fn from(value: &#enum_name) -> Self {
                let mut data_bytes = vec![0u8; #max_variant_size_fn() as usize];
                let material_type = match value {
                    #(#from_arms,)*
                };
                let mut data = [glam::Vec4::ZERO; #vec4s_needed_fn()];
                let data_raw = unsafe {
                    core::slice::from_raw_parts_mut(
                        data.as_mut_ptr() as *mut u8,
                        #vec4s_needed_fn() * core::mem::size_of::<glam::Vec4>(),
                    )
                };
                data_raw[..data_bytes.len()].copy_from_slice(&data_bytes);
                #struct_ident { material_type, data }
            }
        }

        impl encase_enum::ShaderEnumType for #enum_name {
            type GpuRepr = #struct_ident;
            fn to_gpu(&self) -> #struct_ident {
                #struct_ident::from(self)
            }
        }

        impl encase::ShaderType for #enum_name {
            type ExtraMetadata = <#struct_ident as encase::ShaderType>::ExtraMetadata;
            const METADATA: encase::private::Metadata<Self::ExtraMetadata> =
                <#struct_ident as encase::ShaderType>::METADATA;
        }

        impl encase::ShaderSize for #enum_name {}

        impl encase::internal::WriteInto for #enum_name {
            fn write_into<B: encase::internal::BufferMut>(
                &self,
                writer: &mut encase::internal::Writer<B>,
            ) {
                let gpu_repr = #struct_ident::from(self);
                encase::internal::WriteInto::write_into(&gpu_repr, writer);
            }
        }
    };

    expanded.into()
}
