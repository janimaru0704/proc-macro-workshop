use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Ident};

#[proc_macro_derive(Builder)]
pub fn derive(input: TokenStream) -> TokenStream {
    let derive_input = parse_macro_input!(input as DeriveInput);
    generate_builder(&derive_input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn generate_builder(derive_input: &DeriveInput) -> Result<proc_macro2::TokenStream, syn::Error> {
    let fields = match &derive_input.data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Named(named),
            ..
        }) => &named.named,
        _ => {
            return Err(syn::Error::new_spanned(
                &derive_input.ident,
                "Builder can only be derived for structures with named fields",
            ));
        }
    };

    let name = &derive_input.ident;
    let builder_ident = Ident::new(&format!("{}Builder", name), name.span());

    let declare = fields.iter().map(|f| {
        let ident = &f.ident;
        let ty = &f.ty;
        quote! { #ident: std::option::Option<#ty> }
    });

    let init = fields.iter().map(|f| {
        let ident = &f.ident;
        quote! { #ident: std::option::Option::None }
    });

    let setter = fields.iter().map(|f| {
        let ident = &f.ident;
        let ty = &f.ty;
        quote! {
            fn #ident(&mut self, #ident: #ty) -> &mut Self {
                self.#ident = std::option::Option::Some(#ident);
                self
            }
        }
    });

    let (impl_generics, ty_generics, where_clause) = derive_input.generics.split_for_impl();

    let expanded = quote! {
        pub struct #builder_ident {
            #(#declare),*
        }

        impl #builder_ident {
            #(#setter)*
        }

        impl #impl_generics #name #ty_generics #where_clause {
            pub fn builder() -> #builder_ident {
                #builder_ident {
                    #(#init),*
                }
            }
        }
    };
    Ok(expanded)
}
