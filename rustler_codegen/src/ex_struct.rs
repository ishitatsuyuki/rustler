use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};

use syn::{self, spanned::Spanned, Field, Ident};

use super::context::Context;
use super::RustlerAttr;

pub fn transcoder_decorator(ast: &syn::DeriveInput, add_exception: bool) -> TokenStream {
    let ctx = Context::from_ast(ast);

    let elixir_module = get_module(&ctx, add_exception);
    let expect_message = if add_exception {
        "NifException can only be used with structs"
    } else {
        "NifStruct can only be used with structs"
    };

    let struct_fields = ctx.struct_fields.as_ref().expect(expect_message);

    // Unwrap is ok here, as we already determined that struct_fields is not None
    let field_atoms = ctx.field_atoms().unwrap();

    let optional_exception_field = if add_exception {
        quote! {
            atom_exception = "__exception__",
        }
    } else {
        quote! {}
    };

    let atom_defs = quote! {
        rustler::atoms! {
            atom_struct = "__struct__",
            atom_module = #elixir_module,
            #optional_exception_field
            #(#field_atoms)*
        }
    };

    let atoms_module_name = ctx.atoms_module_name(Span::call_site());

    let decoder = if ctx.decode() {
        gen_decoder(&ctx, struct_fields, &atoms_module_name)
    } else {
        quote! {}
    };

    let encoder = if ctx.encode() {
        gen_encoder(&ctx, struct_fields, &atoms_module_name, add_exception)
    } else {
        quote! {}
    };

    let gen = quote! {
        mod #atoms_module_name {
            #atom_defs
        }

        #decoder
        #encoder
    };

    gen
}

fn gen_decoder(ctx: &Context, fields: &[&Field], atoms_module_name: &Ident) -> TokenStream {
    let struct_name = ctx.ident;
    let struct_name_str = struct_name.to_string();

    let idents: Vec<_> = fields
        .iter()
        .map(|field| field.ident.as_ref().unwrap())
        .collect();

    let (assignments, field_defs): (Vec<TokenStream>, Vec<TokenStream>) = fields
        .iter()
        .zip(idents.iter())
        .enumerate()
        .map(|(index, (field, ident))| {
            let atom_fun = Context::field_to_atom_fun(field);
            let variable = Context::escape_ident_with_index(&ident.to_string(), index, "struct");

            let assignment = quote_spanned! { field.span() =>
                let #variable = try_decode_field(term, #atom_fun())?;
            };

            let field_def = quote! {
                #ident: #variable
            };

            (assignment, field_def)
        })
        .unzip();

    super::encode_decode_templates::decoder(
        ctx,
        quote! {
            use #atoms_module_name::*;
            use ::rustler::Encoder;

            fn try_decode_field<'a, T>(
                term: rustler::Term<'a>,
                field: rustler::Atom,
                ) -> ::rustler::NifResult<T>
                where
                    T: rustler::Decoder<'a>,
                {
                    use rustler::Encoder;
                    match ::rustler::Decoder::decode(term.map_get(&field)?) {
                        Err(_) => Err(::rustler::Error::RaiseTerm(Box::new(format!(
                                        "Could not decode field :{:?} on %{}{{}}",
                                        field, #struct_name_str
                        )))),
                        Ok(value) => Ok(value),
                    }
                }

            let module: ::rustler::types::atom::Atom = term.map_get(atom_struct())?.decode()?;
            if module != atom_module() {
                return Err(::rustler::Error::RaiseAtom("invalid_struct"));
            }

            #(#assignments);*

            Ok(#struct_name { #(#field_defs),* })
        },
    )
}

fn gen_encoder(
    ctx: &Context,
    fields: &[&Field],
    atoms_module_name: &Ident,
    add_exception: bool,
) -> TokenStream {
    let field_defs: Vec<TokenStream> = fields
        .iter()
        .map(|field| {
            let field_ident = field.ident.as_ref().unwrap();
            let atom_fun = Context::field_to_atom_fun(field);
            quote_spanned! { field.span() =>
                map = map.map_put(#atom_fun(), &self.#field_ident).unwrap();
            }
        })
        .collect();

    let exception_field = if add_exception {
        quote! {
            map = map.map_put(atom_exception(), true).unwrap();
        }
    } else {
        quote! {}
    };

    super::encode_decode_templates::encoder(
        ctx,
        quote! {
            use #atoms_module_name::*;
            let mut map = ::rustler::types::map::map_new(env);
            map = map.map_put(atom_struct(), atom_module()).unwrap();
            #exception_field
            #(#field_defs)*
            map
        },
    )
}

fn get_module(ctx: &Context, add_exception: bool) -> String {
    let expect_message = if add_exception {
        "NifException requires a 'module' attribute"
    } else {
        "NifStruct requires a 'module' attribute"
    };

    ctx.attrs
        .iter()
        .find_map(|attr| match attr {
            RustlerAttr::Module(ref module) => Some(module.clone()),
            _ => None,
        })
        .expect(expect_message)
}
