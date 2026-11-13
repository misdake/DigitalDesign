//! Compile-time working-storage requests through ordinary const descriptions.
use super::{constants, err, static_assets::AssetBank, storage::Allocator};
use proc_macro2::Span;
use quote::ToTokens;
use std::collections::{HashMap, HashSet};
use syn::{parse_quote, visit_mut::VisitMut, Expr, Item};

#[derive(Clone, Debug)]
pub struct StaticBuffer {
    pub name: String,
    pub address: usize,
    pub bytes: usize,
    pub alignment: usize,
}
struct Alignment(Expr);
impl syn::parse::Parse for Alignment {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let name: syn::Ident = input.parse()?;
        if name != "align" {
            return Err(err(&name, "static_buffer only accepts align"));
        }
        input.parse::<syn::Token![=]>()?;
        let value = input.parse()?;
        if input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
        }
        Ok(Self(value))
    }
}
pub(super) struct Planner {
    pub(super) allocator: Allocator,
    pub(super) declarations: HashMap<String, usize>,
    pub(super) context: Option<(String, usize)>,
    pub(super) buffers: Vec<StaticBuffer>,
    pub(super) live: super::liveness::Live,
}
impl Planner {
    pub(super) fn allocate(
        &mut self,
        bytes: usize,
        alignment: usize,
        at: &Expr,
    ) -> Result<Expr, syn::Error> {
        let (name, minimum) = self
            .context
            .as_ref()
            .ok_or_else(|| err(at, "StaticBuffer::new requires #[static_buffer]"))?;
        if alignment < 128 || !alignment.is_power_of_two() {
            return Err(err(
                at,
                "buffer alignment must be a power of two and at least 128 bytes",
            ));
        }
        let alignment = alignment.max(*minimum);
        if bytes == 0 || bytes > 65535 * 128 {
            return Err(err(at, "buffer size must be in 1..8388480 bytes"));
        }
        let used = self.live.contains(name);
        let range = if used {
            self.allocator
                .allocate(bytes, alignment)
                .map_err(|message| err(at, message))?
        } else {
            0..bytes.div_ceil(128) * 128
        };
        let first_sector = (range.start / 128) as u16;
        let sector_count = (range.len() / 128) as u16;
        if used {
            self.buffers.push(StaticBuffer {
                name: name.clone(),
                address: range.start,
                bytes: range.len(),
                alignment,
            });
        }
        Ok(parse_quote!(StaticBuffer { first_sector: #first_sector, sector_count: #sector_count }))
    }
}

pub(super) struct Prepared {
    pub(super) buffers: Vec<StaticBuffer>,
    pub(super) statics: HashSet<String>,
}

pub(super) fn prepare(
    files: &mut [syn::File],
    banks: &[AssetBank],
    assets: &[super::static_assets::StaticAsset],
    domains: &HashMap<usize, usize>,
    live: &super::liveness::Live,
) -> Result<Prepared, (usize, syn::Error)> {
    let mut declarations = HashMap::new();
    let mut statics = HashSet::new();
    let mut sizes = None;
    for (fi, file) in files.iter().enumerate() {
        if let Some(name) = super::value_methods::reserved(file.to_token_stream(), "__rcc_buffer_")
        {
            return Err((fi, err(&name, "reserved buffer-check name")));
        }
        for item in &file.items {
            let (attrs, name, mutable) = match item {
                Item::Const(c) => (&c.attrs, &c.ident, false),
                Item::Static(s) => (&s.attrs, &s.ident, s.mutability.is_some()),
                _ => continue,
            };
            let attrs: Vec<_> = attrs
                .iter()
                .filter(|a| a.path.is_ident("static_buffer"))
                .collect();
            if attrs.is_empty() {
                continue;
            }
            if attrs.len() != 1 || mutable {
                return Err((
                    fi,
                    err(
                        name,
                        "static_buffer requires one attribute and an immutable declaration",
                    ),
                ));
            }
            let alignment = if attrs[0].tokens.is_empty() {
                128
            } else {
                let option: Alignment = attrs[0].parse_args().map_err(|e| (fi, e))?;
                if sizes.is_none() {
                    sizes = Some(constants::Sizes::new(files)?);
                }
                sizes
                    .as_mut()
                    .unwrap()
                    .bytes(&option.0)
                    .map_err(|e| (fi, e))?
            };
            if alignment < 128 || !alignment.is_power_of_two() {
                return Err((
                    fi,
                    err(
                        name,
                        "buffer alignment must be a power of two and at least 128 bytes",
                    ),
                ));
            }
            if matches!(item, Item::Static(_)) {
                statics.insert(name.to_string());
            }
            declarations.insert(name.to_string(), alignment);
        }
    }
    if declarations.is_empty() {
        return Ok(Prepared {
            buffers: vec![],
            statics,
        });
    }
    let mut allocator =
        Allocator::new(banks).map_err(|e| (0, syn::Error::new(Span::call_site(), e)))?;
    for asset in assets {
        allocator.reserve(asset.address..asset.address + asset.data.len().div_ceil(128) * 128);
    }
    let mut lowered = files.to_vec();
    super::fixed_literals::prepare(&mut lowered)?;
    let methods = super::value_methods::prepare(&mut lowered, true)?;
    let mut evaluator = constants::StorageEvaluation::new(
        &lowered,
        &methods,
        Planner {
            allocator,
            declarations,
            context: None,
            buffers: vec![],
            live: live.clone(),
        },
    )?;
    let owners: HashMap<_, _> = files
        .iter()
        .enumerate()
        .flat_map(|(fi, file)| {
            file.items.iter().filter_map(move |item| {
                let Item::Struct(s) = item else { return None };
                s.fields
                    .iter()
                    .any(|f| !matches!(f.vis, syn::Visibility::Public(_)))
                    .then(|| (s.ident.to_string(), domains.get(&fi).copied().unwrap_or(fi)))
            })
        })
        .collect();
    let mut bindings = Bindings {
        owners,
        generated: vec![],
        counter: 0,
    };
    let mut counter = 0usize;
    for (fi, file) in files.iter_mut().enumerate() {
        let mut checks = vec![];
        for item in &mut file.items {
            let (attrs, name, ty, expression) = match item {
                Item::Const(c) => (&mut c.attrs, &c.ident, &c.ty, &mut c.expr),
                Item::Static(s) => (&mut s.attrs, &s.ident, &s.ty, &mut s.expr),
                _ => continue,
            };
            if !attrs.iter().any(|a| a.path.is_ident("static_buffer")) {
                continue;
            }
            let original = expression.clone();
            let mut value = evaluator
                .description(&name.to_string(), expression)
                .map_err(|e| (fi, e))?;
            bindings.visit_expr_mut(&mut value);
            // Preserve the initializer for ordinary Rust type/privacy/const/unsafe
            // checking. It is never called or emitted into target code.
            let check = syn::Ident::new(&format!("__rcc_buffer_check_{counter}"), name.span());
            checks.push(parse_quote!(const fn #check() -> #ty { #original }));
            counter += 1;
            **expression = value;
            attrs.retain(|a| !a.path.is_ident("static_buffer"));
        }
        file.items.extend(checks);
    }
    for (fi, item) in bindings.generated {
        files[fi].items.push(item);
    }
    Ok(Prepared {
        buffers: evaluator.finish(),
        statics,
    })
}

// Emit folded private descriptions at their owning type, rather than leaking
// private fields into the caller's module. Original initializers are checked
// separately, so these generated bindings cannot hide illegal source access.
struct Bindings {
    owners: HashMap<String, usize>,
    generated: Vec<(usize, Item)>,
    counter: usize,
}
impl VisitMut for Bindings {
    fn visit_expr_mut(&mut self, expression: &mut Expr) {
        syn::visit_mut::visit_expr_mut(self, expression);
        let Expr::Struct(s) = expression else { return };
        let Some(owner) = s.path.get_ident() else {
            return;
        };
        let Some(&fi) = self.owners.get(&owner.to_string()) else {
            return;
        };
        let name = syn::Ident::new(
            &format!("__rcc_buffer_binding_{}", self.counter),
            Span::call_site(),
        );
        self.counter += 1;
        self.generated.push((
            fi,
            parse_quote!(impl #owner { pub const fn #name() -> Self { #s } }),
        ));
        *expression = parse_quote!(#owner::#name());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn place(source: &str, banks: &[AssetBank]) -> Result<Vec<StaticBuffer>, String> {
        let mut files = [syn::parse_file(&format!(r#"
struct StaticBuffer {{ first_sector:u16,sector_count:u16 }}
impl StaticBuffer {{ const fn new(bytes:usize,align:usize)->Self {{ Self{{first_sector:0,sector_count:1}} }} }}
{source}
"#)).unwrap()];
        prepare(&mut files, banks, &[], &HashMap::new(), &Default::default())
            .map(|p| p.buffers)
            .map_err(|(_, e)| e.to_string())
    }
    #[test]
    fn nested_requests_share_dependency_placement_and_raise_alignment() {
        let source = r#"
struct Pair{a:StaticBuffer,b:StaticBuffer}
const fn pair()->Pair{Pair{a:StaticBuffer::new(129,128),b:StaticBuffer::new(128,8192)}}
const ALIGN:usize=4096;
#[static_buffer(align=ALIGN)]const PAIR:Pair=pair();
#[static_buffer]const ALIAS:StaticBuffer=PAIR.a;
#[static_buffer]static LAST:StaticBuffer=StaticBuffer::new(1,128);
"#;
        let buffers = place(
            source,
            &[AssetBank {
                bytes: 0x100083..0x110000,
            }],
        )
        .unwrap();
        assert_eq!(buffers.len(), 3);
        assert_eq!(
            (buffers[0].address, buffers[0].bytes, buffers[0].alignment),
            (0x101000, 256, 4096)
        );
        assert_eq!(buffers[1].address, 0x102000);
        assert_eq!(buffers[1].alignment, 8192);
        assert_eq!(buffers[2].address, 0x100100);
    }
    #[test]
    fn invalid_requests_cannot_allocate_or_escape_banks() {
        let banks = [AssetBank {
            bytes: 0x100000..0x101000,
        }];
        for source in [
            "#[static_buffer]static mut B:StaticBuffer=StaticBuffer::new(128,128);",
            "#[static_buffer(align=64)]const B:StaticBuffer=StaticBuffer::new(128,128);",
            "#[static_buffer(align=384)]const B:StaticBuffer=StaticBuffer::new(128,128);",
            "#[static_buffer]const B:StaticBuffer=StaticBuffer::new(0,128);",
            "#[static_buffer]const B:StaticBuffer=StaticBuffer::new(1,64);",
            "#[static_buffer]const B:StaticBuffer=StaticBuffer::new(4097,128);",
            "const BAD:StaticBuffer=StaticBuffer::new(128,128);#[static_buffer]const B:StaticBuffer=BAD;",
        ] {
            assert!(place(source,&banks).is_err(),"{source}");
        }
    }
}
