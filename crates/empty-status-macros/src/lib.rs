use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[proc_macro_derive(RotateNext)]
pub fn derive_rotate_next(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = input.ident;

    let Data::Enum(data_enum) = input.data else {
        return syn::Error::new_spanned(ident, "RotateNext can only be derived for enums")
            .to_compile_error()
            .into();
    };

    if data_enum.variants.is_empty() {
        return syn::Error::new_spanned(ident, "RotateNext requires at least one variant")
            .to_compile_error()
            .into();
    }

    let mut variants = Vec::with_capacity(data_enum.variants.len());
    for v in data_enum.variants {
        match v.fields {
            Fields::Unit => {}
            _ => {
                return syn::Error::new_spanned(
                    v.ident,
                    "RotateNext only supports fieldless (unit) variants",
                )
                .to_compile_error()
                .into();
            }
        }
        variants.push(v.ident);
    }

    let n = variants.len();
    let arms = (0..n).map(|i| {
        let cur = &variants[i];
        let next = &variants[(i + 1) % n];
        quote! { Self::#cur => Self::#next }
    });

    quote! {
        impl #ident {
            pub const fn next(self) -> Self {
                match self {
                    #(#arms,)*
                }
            }
        }
    }
    .into()
}
