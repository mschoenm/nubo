//! Derive macro for the `Serializable` trait used by Nubo's save states.
//!
//! Structs with named fields store each field with its name (through
//! `FieldWriter` and `read_fields` in `nubo-core`), except fields marked
//! `#[serializable(skip)]`, which reading leaves as they are. Reading matches
//! fields by name and skips ones the struct doesn't have, so fields can be
//! added, removed and reordered without breaking save states of other
//! versions. Enums without data store the variant's name, and tell the stored
//! layout (`stored_layout`) the names of all their variants. The stored name is
//! the field's or variant's name in the code, or the one given with
//! `#[serializable(name = "stored_name")]`, which keeps it when the code is
//! renamed. The generated code names the trait by its full path in
//! `nubo-core` (`crate::serializable::Serializable`), so it only works inside
//! that crate, and the trait doesn't need to be imported.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Attribute, Data, DataEnum, DataStruct, DeriveInput, Error, Fields, Generics, Ident, LitStr,
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

/// What a field's or variant's `#[serializable(...)]` attributes ask for.
#[derive(Default)]
struct Options {
    /// `skip`: leave the field out of save states.
    skip: bool,
    /// `name = "..."`: the name save states store it under, when it isn't the
    /// name in the code.
    stored_name: Option<String>,
}

/// Reads the `#[serializable(...)]` attributes of a field or variant.
fn options(attributes: &[Attribute]) -> syn::Result<Options> {
    let mut options = Options::default();
    for attribute in attributes {
        if attribute.path().is_ident("serializable") {
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    options.skip = true;
                    Ok(())
                } else if meta.path.is_ident("name") {
                    let name: LitStr = meta.value()?.parse()?;
                    if name.value().is_empty() || name.value().len() > 255 {
                        return Err(meta.error("a stored name has 1 to 255 bytes"));
                    }
                    options.stored_name = Some(name.value());
                    Ok(())
                } else {
                    Err(meta.error("the options are `skip` and `name = \"stored name\"`"))
                }
            })?;
        }
    }
    Ok(options)
}

/// Builds an impl that stores each named field with its stored name, leaving
/// out skipped ones, and reads them back by that name.
fn struct_impl(name: &Ident, generics: &Generics, data: &DataStruct) -> syn::Result<TokenStream2> {
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new_spanned(
            name,
            "Serializable can only be derived for structs with named fields",
        ));
    };
    let mut field_idents = Vec::new();
    let mut field_names = Vec::new();
    for field in &fields.named {
        let options = options(&field.attrs)?;
        if options.skip {
            continue;
        }
        let ident = field.ident.as_ref().expect("named fields have names");
        field_idents.push(ident);
        field_names.push(options.stored_name.unwrap_or_else(|| ident.to_string()));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics crate::serializable::Serializable for #name #type_generics #where_clause {
            fn serialize(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
                let mut fields = crate::serializable::FieldWriter::new(writer);
                #(fields.field(#field_names, &self.#field_idents)?;)*
                fields.finish()
            }

            fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> std::io::Result<()> {
                crate::serializable::read_fields(reader, |name, bytes| {
                    match name {
                        #(#field_names => crate::serializable::Serializable::deserialize(
                            &mut self.#field_idents,
                            bytes,
                        )?,)*
                        _ => return Ok(false),
                    }
                    Ok(true)
                })
            }
        }
    })
}

/// Builds an impl that stores the variant's stored name, telling the layout
/// every variant's, and reports an unknown one as an unreadable value, which
/// leaves the field as it was.
fn enum_impl(name: &Ident, generics: &Generics, data: &DataEnum) -> syn::Result<TokenStream2> {
    let mut variant_idents = Vec::new();
    let mut variant_names = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(Error::new_spanned(
                variant,
                "Serializable can only be derived for enums without data; implement it manually instead",
            ));
        }
        let options = options(&variant.attrs)?;
        if options.skip {
            return Err(Error::new_spanned(variant, "variants can't be skipped"));
        }
        variant_idents.push(&variant.ident);
        variant_names.push(
            options
                .stored_name
                .unwrap_or_else(|| variant.ident.to_string()),
        );
    }

    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics crate::serializable::Serializable for #name #type_generics #where_clause {
            fn serialize(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
                let variant = match self {
                    #(#name::#variant_idents => #variant_names,)*
                };
                crate::serializable::write_variant(writer, variant, &[#(#variant_names),*])
            }

            fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> std::io::Result<()> {
                let variant = crate::serializable::read_name(reader)?;
                *self = match variant.as_str() {
                    #(#variant_names => #name::#variant_idents,)*
                    _ => {
                        return Err(crate::serializable::unreadable_value(format!(
                            "unknown {} {variant}",
                            stringify!(#name)
                        )));
                    }
                };
                Ok(())
            }

            fn encoding() -> String {
                String::from("variant")
            }
        }
    })
}
