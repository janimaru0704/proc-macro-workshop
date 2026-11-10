use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Ident};

#[proc_macro_derive(Builder, attributes(builder))]
pub fn derive(input: TokenStream) -> TokenStream {
    let derive_input = parse_macro_input!(input as DeriveInput);
    generate_builder(&derive_input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn each_name(f: &syn::Field) -> Result<Option<(Ident, &syn::Type)>, syn::Error> {
    for attr in &f.attrs {
        if !attr.path().is_ident("builder") {
            continue;
        }
        let mut each = None;
        let res = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("each") {
                each = Some(meta.value()?.parse::<syn::LitStr>()?.parse::<Ident>()?);
                Ok(())
            } else {
                Err(meta.error("unsupported"))
            }
        });
        if res.is_err() || each.is_none() {
            return Err(syn::Error::new_spanned(
                &attr.meta,
                "expected `builder(each = \"...\")`",
            ));
        }
        let elem = inner_type(&f.ty, "Vec")
            .ok_or_else(|| syn::Error::new_spanned(&f.ty, "`each` requires a Vec<T> field"))?;
        return Ok(each.map(|e| (e, elem)));
    }
    Ok(None)
}

fn inner_type<'a>(ty: &'a syn::Type, wrapper: &str) -> Option<&'a syn::Type> {
    if let syn::Type::Path(syn::TypePath {
        qself: None, path, ..
    }) = ty
    {
        if path.segments.len() == 1 {
            let seg = &path.segments[0];
            if seg.ident == wrapper {
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    if args.args.len() == 1 {
                        if let syn::GenericArgument::Type(inner) = &args.args[0] {
                            return Some(inner);
                        }
                    }
                }
            }
        }
    }
    None
}

fn generate_build(
    name: &Ident,
    fields: &syn::punctuated::Punctuated<syn::Field, syn::token::Comma>,
    ty_generics: &syn::TypeGenerics,
    each_names: &[Option<(Ident, &syn::Type)>],
) -> proc_macro2::TokenStream {
    let checks = fields
        .iter()
        .zip(each_names)
        .filter(|(f, each)| each.is_none() && inner_type(&f.ty, "Option").is_none())
        .map(|(f, _)| {
            let ident = &f.ident;
            quote! {
                if self.#ident.is_none() {
                    return std::result::Result::Err(
                        std::format!("Field {} not set", std::stringify!(#ident)).into(),
                    );
                }
            }
        });

    let unwrapping = fields.iter().zip(each_names).map(|(f, each)| {
        let ident = &f.ident;
        if each.is_some() {
            quote! { let #ident = std::mem::take(&mut self.#ident); }
        } else if inner_type(&f.ty, "Option").is_some() {
            quote! { let #ident = self.#ident.take(); }
        } else {
            quote! { let #ident = self.#ident.take().unwrap(); }
        }
    });

    let init = fields.iter().map(|f| {
        let ident = &f.ident;
        quote! { #ident }
    });

    quote! {
        pub fn build(&mut self) -> std::result::Result<#name #ty_generics, std::boxed::Box<dyn std::error::Error>> {
            #(#checks)*
            #(#unwrapping)*
            std::result::Result::Ok(#name { #(#init),* })
        }
    }
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

    let each_names = fields
        .iter()
        .map(each_name)
        .collect::<Result<Vec<_>, syn::Error>>()?;

    let name = &derive_input.ident;
    let builder_ident = Ident::new(&format!("{}Builder", name), name.span());

    let declare = fields.iter().zip(&each_names).map(|(f, each)| {
        let ident = &f.ident;
        let ty = &f.ty;
        if let Some((_, elem)) = each {
            quote! { #ident: std::vec::Vec<#elem> }
        } else if inner_type(ty, "Option").is_some() {
            quote! { #ident: #ty }
        } else {
            quote! { #ident: std::option::Option<#ty> }
        }
    });

    let init = fields.iter().zip(&each_names).map(|(f, each)| {
        let ident = &f.ident;
        if each.is_some() {
            quote! { #ident: std::vec::Vec::new() }
        } else {
            quote! { #ident: std::option::Option::None }
        }
    });

    let setter = fields.iter().zip(&each_names).map(|(f, each)| {
        let ident = &f.ident;
        let ty = &f.ty;
        if let Some((each_ident, elem)) = each {
            let one = quote! {
                pub fn #each_ident(&mut self, #each_ident: #elem) -> &mut Self {
                    self.#ident.push(#each_ident);
                    self
                }
            };
            if f.ident.as_ref() == Some(each_ident) {
                one
            } else {
                quote! {
                    #one
                    pub fn #ident(&mut self, #ident: #ty) -> &mut Self {
                        self.#ident = #ident;
                        self
                    }
                }
            }
        } else if let Some(inner) = inner_type(ty, "Option") {
            quote! {
                fn #ident(&mut self, #ident: #inner) -> &mut Self {
                    self.#ident = std::option::Option::Some(#ident);
                    self
                }
            }
        } else {
            quote! {
                fn #ident(&mut self, #ident: #ty) -> &mut Self {
                    self.#ident = std::option::Option::Some(#ident);
                    self
                }
            }
        }
    });

    let (impl_generics, ty_generics, where_clause) = derive_input.generics.split_for_impl();

    let build = generate_build(name, fields, &ty_generics, &each_names);

    let expanded = quote! {
        pub struct #builder_ident #impl_generics #where_clause {
            #(#declare),*
        }

        impl #impl_generics #builder_ident #ty_generics #where_clause {
            #(#setter)*
            #build
        }

        impl #impl_generics #name #ty_generics #where_clause {
            pub fn builder() -> #builder_ident #ty_generics {
                #builder_ident {
                    #(#init),*
                }
            }
        }
    };
    Ok(expanded)
}
