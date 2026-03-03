use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Fields, LitBool, LitInt, LitStr};

/// Derive the `Options` trait for a bot struct.
///
/// Annotate fields with `#[spin(...)]`, `#[check(...)]`, `#[combo(...)]`,
/// or `#[str_opt(...)]` to declare configurable options. Unannotated fields
/// are ignored.
#[proc_macro_derive(Options, attributes(spin, check, combo, str_opt))]
pub fn derive_options(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match impl_options(&input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn impl_options(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;

    let fields = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    input,
                    "Options derive only supports structs with named fields",
                ))
            },
        },
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "Options derive only supports structs",
            ))
        },
    };

    let mut option_def_exprs = Vec::new();
    let mut apply_arms = Vec::new();

    for field in fields {
        let field_ident = field.ident.as_ref().expect("named fields have idents");
        let field_name = field_ident.to_string();

        for attr in &field.attrs {
            let path_str = attr.path().get_ident().map(|i| i.to_string());

            match path_str.as_deref() {
                Some("spin") => {
                    let (default, min, max) = parse_spin_attr(attr)?;
                    option_def_exprs.push(quote! {
                        ::pyrat_sdk::SdkOptionDef {
                            name: #field_name.to_owned(),
                            option_type: ::pyrat_sdk::SdkOptionType::Spin.to_wire(),
                            default_value: #default.to_string(),
                            min: #min,
                            max: #max,
                            choices: vec![],
                        }
                    });
                    apply_arms.push(quote! {
                        #field_name => {
                            self.#field_ident = __value.parse::<i32>()
                                .map_err(|e| format!("invalid i32 for {}: {e}", #field_name))?;
                            Ok(())
                        }
                    });
                },
                Some("check") => {
                    let default = parse_check_attr(attr)?;
                    let default_str = if default { "true" } else { "false" };
                    option_def_exprs.push(quote! {
                        ::pyrat_sdk::SdkOptionDef {
                            name: #field_name.to_owned(),
                            option_type: ::pyrat_sdk::SdkOptionType::Check.to_wire(),
                            default_value: #default_str.to_owned(),
                            min: 0,
                            max: 0,
                            choices: vec![],
                        }
                    });
                    apply_arms.push(quote! {
                        #field_name => {
                            self.#field_ident = match __value {
                                "true" | "1" => true,
                                "false" | "0" => false,
                                _ => return Err(format!("invalid bool for {}: {}", #field_name, __value)),
                            };
                            Ok(())
                        }
                    });
                },
                Some("combo") => {
                    let (default, choices) = parse_combo_attr(attr)?;
                    let choices_expr: Vec<_> =
                        choices.iter().map(|c| quote! { #c.to_owned() }).collect();
                    option_def_exprs.push(quote! {
                        ::pyrat_sdk::SdkOptionDef {
                            name: #field_name.to_owned(),
                            option_type: ::pyrat_sdk::SdkOptionType::Combo.to_wire(),
                            default_value: #default.to_owned(),
                            min: 0,
                            max: 0,
                            choices: vec![#(#choices_expr),*],
                        }
                    });
                    apply_arms.push(quote! {
                        #field_name => {
                            self.#field_ident = __value.to_owned();
                            Ok(())
                        }
                    });
                },
                Some("str_opt") => {
                    let default = parse_str_opt_attr(attr)?;
                    option_def_exprs.push(quote! {
                        ::pyrat_sdk::SdkOptionDef {
                            name: #field_name.to_owned(),
                            option_type: ::pyrat_sdk::SdkOptionType::String.to_wire(),
                            default_value: #default.to_owned(),
                            min: 0,
                            max: 0,
                            choices: vec![],
                        }
                    });
                    apply_arms.push(quote! {
                        #field_name => {
                            self.#field_ident = __value.to_owned();
                            Ok(())
                        }
                    });
                },
                _ => {},
            }
        }
    }

    let option_defs_body = if option_def_exprs.is_empty() {
        quote! { vec![] }
    } else {
        quote! { vec![#(#option_def_exprs),*] }
    };

    let apply_body = if apply_arms.is_empty() {
        quote! {
            Err(format!("unknown option: {}", __name))
        }
    } else {
        quote! {
            match __name {
                #(#apply_arms),*,
                _ => Err(format!("unknown option: {}", __name)),
            }
        }
    };

    Ok(quote! {
        impl ::pyrat_sdk::Options for #name {
            fn option_defs(&self) -> Vec<::pyrat_sdk::SdkOptionDef> {
                #option_defs_body
            }

            fn apply_option(&mut self, __name: &str, __value: &str) -> Result<(), String> {
                #apply_body
            }
        }
    })
}

// ── Attribute parsers (syn 2.x API) ──────────────────

fn parse_spin_attr(attr: &syn::Attribute) -> syn::Result<(i32, i32, i32)> {
    let mut default = 0i32;
    let mut min = i32::MIN;
    let mut max = i32::MAX;

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("default") {
            let value = meta.value()?;
            let lit: LitInt = value.parse()?;
            default = lit.base10_parse()?;
        } else if meta.path.is_ident("min") {
            let value = meta.value()?;
            let lit: LitInt = value.parse()?;
            min = lit.base10_parse()?;
        } else if meta.path.is_ident("max") {
            let value = meta.value()?;
            let lit: LitInt = value.parse()?;
            max = lit.base10_parse()?;
        }
        Ok(())
    })?;

    Ok((default, min, max))
}

fn parse_check_attr(attr: &syn::Attribute) -> syn::Result<bool> {
    let mut default = false;

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("default") {
            let value = meta.value()?;
            let lit: LitBool = value.parse()?;
            default = lit.value;
        }
        Ok(())
    })?;

    Ok(default)
}

fn parse_combo_attr(attr: &syn::Attribute) -> syn::Result<(String, Vec<String>)> {
    let mut default = String::new();
    let mut choices = Vec::new();

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("default") {
            let value = meta.value()?;
            let lit: LitStr = value.parse()?;
            default = lit.value();
        } else if meta.path.is_ident("choices") {
            // Parse choices = ["a", "b", ...]
            let value = meta.value()?;
            let content;
            syn::bracketed!(content in value);
            while !content.is_empty() {
                let lit: LitStr = content.parse()?;
                choices.push(lit.value());
                if !content.is_empty() {
                    let _ = content.parse::<syn::Token![,]>();
                }
            }
        }
        Ok(())
    })?;

    Ok((default, choices))
}

fn parse_str_opt_attr(attr: &syn::Attribute) -> syn::Result<String> {
    let mut default = String::new();

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("default") {
            let value = meta.value()?;
            let lit: LitStr = value.parse()?;
            default = lit.value();
        }
        Ok(())
    })?;

    Ok(default)
}
