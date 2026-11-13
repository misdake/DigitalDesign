//! The SDK's literal macro shares its exact host quantizer with checked lowering.
use super::err;
use quote::ToTokens;
use syn::{parse_quote_spanned, spanned::Spanned, visit_mut::VisitMut, Expr, Lit, Token};

pub(super) fn value(mac: &syn::Macro) -> Result<i32, syn::Error> {
    use syn::parse::Parser;
    let arguments = syn::punctuated::Punctuated::<Expr, Token![,]>::parse_terminated
        .parse2(mac.tokens.clone())?;
    if arguments.len() != 1 {
        return Err(err(mac, "fix! requires one decimal literal"));
    }
    let mut expression = &arguments[0];
    let negative = matches!(expression,Expr::Unary(u) if matches!(u.op,syn::UnOp::Neg(_)));
    if let Expr::Unary(unary) = expression {
        if negative {
            expression = &unary.expr;
        }
    }
    let Expr::Lit(literal) = expression else {
        return Err(err(
            expression,
            "fix! requires a decimal literal, not an expression",
        ));
    };
    let text = match &literal.lit {
        Lit::Int(value) if value.suffix().is_empty() => value.to_string(),
        Lit::Float(value) if value.suffix().is_empty() => value.to_string(),
        _ => return Err(err(literal, "fix! requires an unsuffixed decimal literal")),
    };
    let text = if negative { format!("-{text}") } else { text };
    crate::fixed_constants::decimal(&text).map_err(|message| err(mac, message))
}

pub(super) fn prepare(files: &mut [syn::File]) -> Result<(), (usize, syn::Error)> {
    struct Expand(Option<syn::Error>);
    impl VisitMut for Expand {
        fn visit_expr_mut(&mut self, expression: &mut Expr) {
            if let Expr::Macro(mac) = expression {
                if mac.mac.path.is_ident("fix") {
                    match value(&mac.mac) {
                        Ok(raw) => {
                            let lo = raw as u16;
                            let hi = (raw >> 16) as u16;
                            *expression =
                                parse_quote_spanned!(mac.span()=> fix32::from_words(#lo,#hi));
                        }
                        Err(error) => self.0 = Some(error),
                    }
                    return;
                }
                if mac.mac.path.is_ident("assert") || mac.mac.path.is_ident("debug_assert") {
                    if let Ok(mut condition) = syn::parse2::<Expr>(mac.mac.tokens.clone()) {
                        self.visit_expr_mut(&mut condition);
                        mac.mac.tokens = condition.to_token_stream();
                    }
                    return;
                }
            }
            syn::visit_mut::visit_expr_mut(self, expression);
        }
    }
    for (index, file) in files.iter_mut().enumerate() {
        let mut expand = Expand(None);
        expand.visit_file_mut(file);
        if let Some(error) = expand.0 {
            return Err((index, error));
        }
    }
    Ok(())
}
