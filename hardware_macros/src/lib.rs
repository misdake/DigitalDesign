use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Data, DeriveInput, Expr, Fields, GenericArgument, PathArguments, Type,
};

enum FieldKind {
    Scalar,
    Bus(proc_macro2::TokenStream),
}

fn field_kind(field_type: &Type) -> syn::Result<FieldKind> {
    let Type::Path(type_path) = field_type else {
        return Err(syn::Error::new_spanned(
            field_type,
            "ModuleIo fields must be Wire or Wires<N>",
        ));
    };
    let Some(segment) = type_path.path.segments.last() else {
        return Err(syn::Error::new_spanned(field_type, "missing field type"));
    };
    if segment.ident == "Wire" {
        return Ok(FieldKind::Scalar);
    }
    if segment.ident != "Wires" {
        return Err(syn::Error::new_spanned(
            field_type,
            "ModuleIo fields must be Wire or Wires<N>",
        ));
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            field_type,
            "Wires requires a width",
        ));
    };
    let Some(width) = arguments.args.first() else {
        return Err(syn::Error::new_spanned(
            field_type,
            "Wires width must be a const expression",
        ));
    };
    if let GenericArgument::Const(Expr::Lit(expression)) = width {
        let syn::Lit::Int(literal) = &expression.lit else {
            return Err(syn::Error::new_spanned(
                field_type,
                "Wires width must be an integer",
            ));
        };
        let literal = literal.base10_parse::<usize>()?;
        if !(1..=64).contains(&literal) {
            return Err(syn::Error::new_spanned(
                field_type,
                "ModuleIo supports bus widths from 1 through 64",
            ));
        }
    }
    let width = match width {
        GenericArgument::Const(width) => quote! { #width },
        // syn 1 parses an unbraced const identifier as a type argument. Rust
        // resolves it correctly when the generated impl is compiled.
        GenericArgument::Type(Type::Path(width)) => quote! { #width },
        _ => {
            return Err(syn::Error::new_spanned(
                field_type,
                "Wires width must be a const expression",
            ))
        }
    };
    Ok(FieldKind::Bus(width))
}

#[proc_macro_derive(ModuleIo)]
pub fn derive_module_io(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let generics = input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let value_name = format_ident!("{}Value", name);
    let Data::Struct(struct_data) = input.data else {
        return syn::Error::new_spanned(name, "ModuleIo can only be derived for structs")
            .to_compile_error()
            .into();
    };
    let Fields::Named(fields) = struct_data.fields else {
        return syn::Error::new_spanned(name, "ModuleIo requires named fields")
            .to_compile_error()
            .into();
    };

    let mut value_fields = Vec::new();
    let mut allocations = Vec::new();
    let mut bindings = Vec::new();
    let mut setters = Vec::new();
    let mut getters = Vec::new();

    for field in fields.named {
        let Some(field_name) = field.ident else {
            unreachable!();
        };
        let field_string = field_name.to_string();
        let kind = match field_kind(&field.ty) {
            Ok(kind) => kind,
            Err(error) => return error.to_compile_error().into(),
        };
        match kind {
            FieldKind::Scalar => {
                value_fields.push(quote! { pub #field_name: bool });
                allocations.push(quote! { #field_name: ::digital_design_code::input() });
                bindings.push(quote! {
                    ::digital_design_hardware::IoBinding {
                        name: #field_string,
                        wires: vec![self.#field_name],
                    }
                });
                setters.push(quote! {
                    self.#field_name.set(circuit, u8::from(value.#field_name));
                });
                getters.push(quote! {
                    #field_name: self.#field_name.is_one(circuit)
                });
            }
            FieldKind::Bus(width) => {
                value_fields.push(quote! { pub #field_name: u64 });
                allocations.push(quote! {
                    #field_name: {
                        assert!(
                            (1..=64).contains(&(#width)),
                            "ModuleIo supports bus widths from 1 through 64"
                        );
                        ::digital_design_code::input_w::<#width>()
                    }
                });
                bindings.push(quote! {
                    ::digital_design_hardware::IoBinding {
                        name: #field_string,
                        wires: self.#field_name.wires.to_vec(),
                    }
                });
                setters.push(quote! {
                    for (bit, wire) in self.#field_name.wires.iter().enumerate() {
                        wire.set(circuit, ((value.#field_name >> bit) & 1) as u8);
                    }
                });
                getters.push(quote! {
                    #field_name: self.#field_name.wires.iter().enumerate().fold(
                        0u64,
                        |result, (bit, wire)| result | ((wire.get(circuit) as u64) << bit),
                    )
                });
            }
        }
    }

    quote! {
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct #value_name {
            #(#value_fields,)*
        }

        impl #impl_generics ::digital_design_hardware::ModuleIo for #name #ty_generics #where_clause {
            type Value = #value_name;

            fn allocate() -> Self {
                Self {
                    #(#allocations,)*
                }
            }

            fn bindings(&self) -> Vec<::digital_design_hardware::IoBinding> {
                vec![#(#bindings),*]
            }

            fn drive(
                &self,
                circuit: &mut ::digital_design_code::CircuitWires,
                value: &Self::Value,
            ) {
                #(#setters)*
            }

            fn sample(
                &self,
                circuit: &::digital_design_code::CircuitWires,
            ) -> Self::Value {
                Self::Value {
                    #(#getters,)*
                }
            }
        }
    }
    .into()
}
