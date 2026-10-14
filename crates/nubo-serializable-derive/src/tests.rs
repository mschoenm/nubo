use super::*;
use syn::parse_quote;

/// The error deriving `Serializable` for `input` gives, if any.
fn derive_error(input: &DeriveInput) -> Option<String> {
    let result = match &input.data {
        Data::Struct(data) => struct_impl(&input.ident, &input.generics, data),
        Data::Enum(data) => enum_impl(&input.ident, &input.generics, data),
        Data::Union(_) => unreachable!("the tests derive structs and enums"),
    };
    result.err().map(|error| error.to_string())
}

#[test]
fn fields_with_different_stored_names_derive() {
    let input: DeriveInput = parse_quote! {
        struct Registers {
            #[serializable(name = "old_name")]
            renamed: u8,
            other: u8,
            #[serializable(skip)]
            cache: u8,
        }
    };
    assert_eq!(derive_error(&input), None);
}

#[test]
fn a_field_renamed_to_another_fields_name_is_refused() {
    let input: DeriveInput = parse_quote! {
        struct Registers {
            #[serializable(name = "other")]
            renamed: u8,
            other: u8,
        }
    };
    assert_eq!(
        derive_error(&input).as_deref(),
        Some("the stored name `other` is already used")
    );
}

#[test]
fn two_fields_renamed_alike_are_refused() {
    let input: DeriveInput = parse_quote! {
        struct Registers {
            #[serializable(name = "x")]
            first: u8,
            #[serializable(name = "x")]
            second: u8,
        }
    };
    assert_eq!(
        derive_error(&input).as_deref(),
        Some("the stored name `x` is already used")
    );
}

#[test]
fn a_skipped_field_may_share_a_name() {
    let input: DeriveInput = parse_quote! {
        struct Registers {
            #[serializable(skip, name = "other")]
            cache: u8,
            other: u8,
        }
    };
    assert_eq!(derive_error(&input), None);
}

#[test]
fn a_variant_renamed_to_another_variants_name_is_refused() {
    let input: DeriveInput = parse_quote! {
        enum Mode {
            #[serializable(name = "Off")]
            Disabled,
            Off,
        }
    };
    assert_eq!(
        derive_error(&input).as_deref(),
        Some("the stored name `Off` is already used")
    );
}
