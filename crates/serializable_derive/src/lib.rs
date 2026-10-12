//! Derive macro for the `Serializable` trait used by Nubo's save states.
//!
//! Structs with named fields write and read each field in declaration order,
//! except fields marked `#[serializable(skip)]`, which reading leaves as they
//! are. Enums without data store the variant's index as a single byte. The
//! generated code names the trait by its full path in `nubo-core`
//! (`crate::serializable::Serializable`), so it only works inside that crate,
//! and the trait doesn't need to be imported.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Data, DataEnum, DataStruct, DeriveInput, Error, Field, Fields, Generics, Ident,
    parse_macro_input,
};

/// Implements `Serializable` for a struct with named fields or an enum whose
/// variants carry no data.
#[proc_macro_derive(Serializable, attributes(serializable))]
pub fn derive_serializable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let expanded = match &input.data {
        Data::Struct(data) => struct_impl(&input.ident, &input.generics, data),
        Data::Enum(data) => enum_impl(&input.ident, &input.generics, data),
        Data::Union(_) => Err(Error::new_spanned(
            &input.ident,
            "Serializable can only be derived for structs and enums",
        )),
    };
    expanded.unwrap_or_else(Error::into_compile_error).into()
}

/// Whether a field is marked `#[serializable(skip)]`, which leaves it out of
/// save states.
fn is_skipped(field: &Field) -> syn::Result<bool> {
    let mut skipped = false;
    for attribute in &field.attrs {
        if attribute.path().is_ident("serializable") {
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    skipped = true;
                    Ok(())
                } else {
                    Err(meta.error("the only option is `skip`"))
                }
            })?;
        }
    }
    Ok(skipped)
}

/// Builds an impl that serializes each named field in declaration order,
/// leaving out skipped ones.
fn struct_impl(name: &Ident, generics: &Generics, data: &DataStruct) -> syn::Result<TokenStream2> {
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new_spanned(
            name,
            "Serializable can only be derived for structs with named fields",
        ));
    };
    let mut field_names = Vec::new();
    for field in &fields.named {
        if !is_skipped(field)? {
            field_names.push(&field.ident);
        }
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics crate::serializable::Serializable for #name #type_generics #where_clause {
            fn serialize(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
                #(crate::serializable::Serializable::serialize(&self.#field_names, writer)?;)*
                Ok(())
            }

            fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> std::io::Result<()> {
                #(crate::serializable::Serializable::deserialize(&mut self.#field_names, reader)?;)*
                Ok(())
            }
        }
    })
}

/// Builds an impl that stores the variant's position as one byte and rejects
/// unknown bytes when reading.
fn enum_impl(name: &Ident, generics: &Generics, data: &DataEnum) -> syn::Result<TokenStream2> {
    let mut variant_names = Vec::new();
    let mut discriminants = Vec::new();
    for (index, variant) in data.variants.iter().enumerate() {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(Error::new_spanned(
                variant,
                "Serializable can only be derived for enums without data; implement it manually instead",
            ));
        }
        let discriminant = u8::try_from(index).map_err(|_| {
            Error::new_spanned(variant, "Serializable supports at most 256 enum variants")
        })?;
        variant_names.push(&variant.ident);
        discriminants.push(discriminant);
    }

    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics crate::serializable::Serializable for #name #type_generics #where_clause {
            fn serialize(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
                let discriminant: u8 = match self {
                    #(#name::#variant_names => #discriminants,)*
                };
                crate::serializable::Serializable::serialize(&discriminant, writer)
            }

            fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> std::io::Result<()> {
                let mut discriminant = 0u8;
                crate::serializable::Serializable::deserialize(&mut discriminant, reader)?;
                *self = match discriminant {
                    #(#discriminants => #name::#variant_names,)*
                    _ => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "invalid enum discriminant",
                        ));
                    }
                };
                Ok(())
            }
        }
    })
}
