use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data, Fields, Variant};

#[proc_macro_derive(Serializable)]
pub fn serializable_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    let name = &input.ident;
    
    match &input.data {
        Data::Struct(data_struct) => generate_struct_impl(name, data_struct),
        Data::Enum(data_enum) => generate_enum_impl(name, &data_enum.variants.iter().collect::<Vec<_>>()),
        _ => panic!("Serializable only supports structs and enums"),
    }
}

fn generate_struct_impl(name: &syn::Ident, data_struct: &syn::DataStruct) -> TokenStream {
    let fields = match &data_struct.fields {
        Fields::Named(fields_named) => &fields_named.named,
        _ => panic!("Serializable only supports structs with named fields"),
    };
    
    let field_names: Vec<_> = fields.iter()
        .map(|f| &f.ident)
        .collect();
    
    let serialize_fields = field_names.iter().map(|name| {
        quote! {
            self.#name.serialize(writer)?;
        }
    });
    
    let deserialize_fields = field_names.iter().map(|name| {
        quote! {
            self.#name.deserialize(reader)?;
        }
    });
    
    let expanded = quote! {
        impl Serializable for #name {
            fn serialize(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
                #(#serialize_fields)*
                Ok(())
            }
            
            fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> std::io::Result<()> {
                #(#deserialize_fields)*
                Ok(())
            }
        }
    };
    
    TokenStream::from(expanded)
}

fn generate_enum_impl(name: &syn::Ident, variants: &[&Variant]) -> TokenStream {
    let serialize_arms = variants.iter().enumerate().map(|(index, variant)| {
        let variant_name = &variant.ident;
        let discriminant = index as u8;
        
        match &variant.fields {
            Fields::Unit => {
                quote! {
                    #name::#variant_name => #discriminant.serialize(writer),
                }
            }
            Fields::Unnamed(_) => panic!("Serializable enum derive does not yet support tuple variants with data. Please implement Serializable manually for this enum."),
            Fields::Named(_) => panic!("Serializable enum derive does not yet support named field variants. Please implement Serializable manually for this enum."),
        }
    });
    
    let deserialize_arms = variants.iter().enumerate().map(|(index, variant)| {
        let variant_name = &variant.ident;
        let discriminant = index as u8;
        
        match &variant.fields {
            Fields::Unit => {
                quote! {
                    #discriminant => *self = #name::#variant_name,
                }
            }
            Fields::Unnamed(_) => panic!("Serializable enum derive does not yet support tuple variants with data. Please implement Serializable manually for this enum."),
            Fields::Named(_) => panic!("Serializable enum derive does not yet support named field variants. Please implement Serializable manually for this enum."),
        }
    });
    
    let expanded = quote! {
        impl Serializable for #name {
            fn serialize(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
                match self {
                    #(#serialize_arms)*
                }
            }
            
            fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> std::io::Result<()> {
                let mut discriminant = 0u8;
                discriminant.deserialize(reader)?;
                
                match discriminant {
                    #(#deserialize_arms)*
                    _ => return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Invalid enum discriminant"
                    ))
                }
                Ok(())
            }
        }
    };
    
    TokenStream::from(expanded)
}