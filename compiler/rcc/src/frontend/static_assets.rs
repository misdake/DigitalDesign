//! Source includes and statically placed external bytes. The host owns bank policy.
use super::err;
use std::path::{Path, PathBuf};
use syn::{parse_quote, spanned::Spanned, visit_mut::VisitMut, Expr, Item, Lit, Token};

struct AssetOption {
    name: syn::Ident,
    value: Expr,
}
impl syn::parse::Parse for AssetOption {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![=]>()?;
        Ok(Self {
            name,
            value: input.parse()?,
        })
    }
}

const SECTOR_BYTES: usize = 128;

pub use super::storage::StorageBank as AssetBank;
#[derive(Clone, Debug)]
pub struct StaticAsset {
    pub name: String,
    pub address: usize,
    pub alignment: usize,
    pub data: Vec<u8>,
}
impl StaticAsset {
    /// Initialize the reserved final sector deterministically; length stays exact.
    pub fn load_data(&self) -> Vec<u8> {
        let mut data = self.data.clone();
        data.resize(data.len().div_ceil(SECTOR_BYTES) * SECTOR_BYTES, 0);
        data
    }
}
#[derive(Clone, Debug)]
pub struct BinaryInput {
    pub path: PathBuf,
    pub data: Vec<u8>,
}
#[derive(Clone, Debug, Default)]
pub struct Assets {
    pub placed: Vec<StaticAsset>,
    /// Validated source payloads omitted from runtime storage; producer metadata
    /// can still be checked against them without consuming physical capacity.
    pub discarded: Vec<StaticAsset>,
    pub inputs: Vec<BinaryInput>,
    pub buffers: Vec<super::static_buffers::StaticBuffer>,
}

#[cfg(test)]
#[path = "static_assets_tests.rs"]
mod tests;

#[cfg(test)]
pub(super) fn prepare(
    files: &mut [syn::File],
    names: &[String],
    origins: &[Option<PathBuf>],
    banks: &[AssetBank],
) -> Result<Assets, (usize, syn::Error)> {
    prepare_live(files, names, origins, banks, &Default::default())
}

pub(super) fn prepare_live(
    files: &mut [syn::File],
    names: &[String],
    origins: &[Option<PathBuf>],
    banks: &[AssetBank],
    live: &super::liveness::Live,
) -> Result<Assets, (usize, syn::Error)> {
    let mut result = Assets::default();
    let mut allocator = super::storage::Allocator::new(banks)
        .map_err(|message| (0, syn::Error::new(proc_macro2::Span::call_site(), message)))?;
    for (fi, file) in files.iter_mut().enumerate() {
        struct Includes<'a> {
            origin: Option<&'a Path>,
            inputs: &'a mut Vec<BinaryInput>,
            error: Option<syn::Error>,
        }
        impl VisitMut for Includes<'_> {
            fn visit_expr_mut(&mut self, expression: &mut Expr) {
                let Expr::Macro(m) = expression else {
                    syn::visit_mut::visit_expr_mut(self, expression);
                    return;
                };
                if !m.mac.path.is_ident("include_bytes") || self.error.is_some() {
                    return;
                }
                let load = (|| {
                    let path: syn::LitStr = syn::parse2(m.mac.tokens.clone())?;
                    let source = PathBuf::from(path.value());
                    let source = if source.is_absolute() {
                        source
                    } else {
                        self.origin
                            .and_then(Path::parent)
                            .ok_or_else(|| {
                                err(
                                    m,
                                    "relative include_bytes requires a real source-file origin",
                                )
                            })?
                            .join(source)
                    };
                    let source = source
                        .canonicalize()
                        .map_err(|e| err(m, format!("include_bytes {}: {e}", source.display())))?;
                    let data = std::fs::read(&source)
                        .map_err(|e| err(m, format!("include_bytes {}: {e}", source.display())))?;
                    if !self.inputs.iter().any(|i| i.path == source) {
                        self.inputs.push(BinaryInput {
                            path: source,
                            data: data.clone(),
                        });
                    }
                    let literal = syn::LitByteStr::new(&data, m.span());
                    Ok(syn::parse_quote_spanned!(m.span()=> #literal))
                })();
                match load {
                    Ok(value) => *expression = value,
                    Err(e) => self.error = Some(e),
                }
            }
        }
        let fallback = Path::new(&names[fi]);
        let origin = origins
            .get(fi)
            .and_then(Option::as_deref)
            .or_else(|| fallback.is_absolute().then_some(fallback));
        let mut includes = Includes {
            origin,
            inputs: &mut result.inputs,
            error: None,
        };
        includes.visit_file_mut(file);
        if let Some(e) = includes.error {
            return Err((fi, e));
        }
    }
    let has_assets = files.iter().flat_map(|f| &f.items).any(|item| {
        let attrs = match item {
            Item::Static(s) => &s.attrs,
            Item::Const(c) => &c.attrs,
            _ => return false,
        };
        attrs.iter().any(|a| a.path.is_ident("asset"))
    });
    let mut sizes = if has_assets {
        Some(super::constants::Sizes::new(files)?)
    } else {
        None
    };
    for (fi, file) in files.iter_mut().enumerate() {
        for item in &mut file.items {
            let (attributes, ident, ty, expression, mutable) = match item {
                Item::Static(s) => (
                    &mut s.attrs,
                    &s.ident,
                    &s.ty,
                    &mut s.expr,
                    s.mutability.is_some(),
                ),
                Item::Const(c) => (&mut c.attrs, &c.ident, &c.ty, &mut c.expr, false),
                _ => continue,
            };
            let attrs: Vec<_> = attributes
                .iter()
                .filter(|a| a.path.is_ident("asset"))
                .cloned()
                .collect();
            if attrs.is_empty() {
                continue;
            }
            let fail = |message: &str| (fi, err(ident, message));
            if attrs.len() != 1 || mutable {
                return Err(fail(
                    "asset requires one attribute and an immutable declaration",
                ));
            }
            let options = if attrs[0].tokens.is_empty() {
                syn::punctuated::Punctuated::new()
            } else {
                attrs[0]
                    .parse_args_with(
                        syn::punctuated::Punctuated::<AssetOption, Token![,]>::parse_terminated,
                    )
                    .map_err(|error| (fi, error))?
            };
            let mut align = None;
            for option in options {
                if option.name == "align" && align.is_none() {
                    align = Some(
                        sizes
                            .as_mut()
                            .unwrap()
                            .bytes(&option.value)
                            .map_err(|error| (fi, error))?,
                    );
                } else {
                    return Err(fail(
                        "duplicate or unknown asset option; only align is supported",
                    ));
                }
            }
            let align = align.unwrap_or(SECTOR_BYTES);
            if align < SECTOR_BYTES || !align.is_power_of_two() {
                return Err(fail(
                    "external asset alignment must be a power of two and at least 128 bytes",
                ));
            }
            if !matches!(ty.as_ref(),syn::Type::Path(p) if p.path.is_ident("AssetBytes")) {
                return Err(fail("external asset declarations require AssetBytes"));
            }
            let Expr::Call(call) = expression.as_ref() else {
                return Err(fail(
                    "asset initializer must be AssetBytes::new(include_bytes!(...))",
                ));
            };
            if !matches!(call.func.as_ref(),Expr::Path(p) if p.qself.is_none() && p.path.segments.len()==2 && p.path.segments[0].ident=="AssetBytes" && p.path.segments[1].ident=="new" && p.path.segments.iter().all(|s|s.arguments.is_empty()))
                || call.args.len() != 1
            {
                return Err(fail(
                    "asset initializer must be AssetBytes::new(include_bytes!(...))",
                ));
            }
            let Expr::Lit(literal) = &call.args[0] else {
                return Err(fail("asset initializer requires whole immutable bytes"));
            };
            let Lit::ByteStr(literal) = &literal.lit else {
                return Err(fail("asset initializer requires whole immutable bytes"));
            };
            let data = literal.value();
            if data.is_empty() {
                return Err(fail("external asset must not be empty"));
            }
            let length = data
                .len()
                .checked_add(127)
                .map(|n| n & !127)
                .ok_or_else(|| fail("asset size overflow"))?;
            let used = live.contains(&ident.to_string());
            let address = if used {
                allocator.allocate(data.len(), align).map_err(fail)?.start
            } else {
                0
            };
            let first_sector =
                u16::try_from(address / 128).map_err(|_| fail("asset sector address overflow"))?;
            let sector_count =
                u16::try_from(length / 128).map_err(|_| fail("asset sector count overflow"))?;
            let byte_len_lo = data.len() as u16;
            let byte_len_hi = (data.len() >> 16) as u16;
            **expression = parse_quote!(AssetBytes { first_sector: #first_sector, sector_count: #sector_count, byte_len_lo: #byte_len_lo, byte_len_hi: #byte_len_hi });
            attributes.retain(|a| !a.path.is_ident("asset"));
            let asset = StaticAsset {
                name: ident.to_string(),
                address,
                alignment: align,
                data,
            };
            if used {
                result.placed.push(asset);
            } else {
                result.discarded.push(asset);
            }
        }
    }
    // The constructor denotes a placed blob, not a runtime descriptor factory.
    struct Unplaced(Option<syn::Error>);
    impl<'ast> syn::visit::Visit<'ast> for Unplaced {
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if matches!(call.func.as_ref(),Expr::Path(p) if p.path.segments.len()==2 && p.path.segments[0].ident=="AssetBytes" && p.path.segments[1].ident=="new")
            {
                self.0 = Some(err(
                    call,
                    "AssetBytes::new requires an #[asset] declaration",
                ));
            }
            syn::visit::visit_expr_call(self, call);
        }
    }
    for (fi, file) in files.iter().enumerate() {
        let mut check = Unplaced(None);
        syn::visit::Visit::visit_file(&mut check, file);
        if let Some(error) = check.0 {
            return Err((fi, error));
        }
    }
    Ok(result)
}
