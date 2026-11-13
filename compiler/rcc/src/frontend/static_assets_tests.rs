use super::{prepare, AssetBank, Assets};
use syn::parse_quote;

fn place(source: &str, ranges: &[std::ops::Range<usize>]) -> Result<Assets, String> {
    let mut files = [syn::parse_file(source).unwrap()];
    let banks: Vec<_> = ranges
        .iter()
        .cloned()
        .map(|bytes| AssetBank { bytes })
        .collect();
    prepare(&mut files, &["synthetic.rs".into()], &[None], &banks).map_err(|(_, e)| e.to_string())
}

#[test]
fn external_asset_padding_fallback_and_physical_end_are_exact() {
    let source = r#"
#[asset]static A:AssetBytes=AssetBytes::new(b"abc");
#[asset]static B:AssetBytes=AssetBytes::new(b"de");
"#;
    let assets = place(source, &[0x100000..0x100080, 0x7fff80..0x800000]).unwrap();
    assert_eq!(
        assets.placed.iter().map(|a| a.address).collect::<Vec<_>>(),
        [0x100000, 0x7fff80]
    );
    assert_eq!(assets.placed[0].data, b"abc");
    assert_eq!(assets.placed[0].load_data().len(), 128);
    assert!(assets.placed[0].load_data()[3..].iter().all(|b| *b == 0));
    assert_eq!(assets.placed[1].data, b"de");
    assert_eq!(assets.placed[1].alignment, 128);
    assert!(place(source, std::slice::from_ref(&(0x100000..0x100080))).is_err());
    assert!(
        place(source, &[0x100000..0x100040, 0x100080..0x1000c0]).is_err(),
        "cannot join separated banks"
    );
}

#[test]
fn external_asset_invalid_options_alignment_size_and_extent_are_rejected() {
    for attribute in [
        "kind=\"gpu\"",
        "kind=\"other\"",
        "align=64",
        "align=384",
        "align=128,align=256",
    ] {
        let source = format!("#[asset({attribute})]static A:AssetBytes=AssetBytes::new(b\"x\");");
        assert!(
            place(&source, std::slice::from_ref(&(0x100000..0x110000))).is_err(),
            "{attribute}"
        );
    }
    assert!(place(
        "#[asset]static A:AssetBytes=AssetBytes::new(b\"\");",
        std::slice::from_ref(&(0x100000..0x110000))
    )
    .is_err());
    assert!(place(
        "#[asset]static A:AssetBytes=AssetBytes::new(b\"x\");",
        std::slice::from_ref(&(0x800000..0x800080))
    )
    .is_err());
}

#[test]
fn include_bytes_uses_module_origin_and_reads_changed_input() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/sdk-static-assets/includes");
    std::fs::create_dir_all(root.join("nested")).unwrap();
    let origin = root.join("nested/module.rs");
    let source = "const BYTES:&[u8]=include_bytes!(\"../bytes.bin\");fn main(){}";
    for data in [&b"first"[..], &b"changed input"[..]] {
        std::fs::write(root.join("bytes.bin"), data).unwrap();
        let mut files = [syn::parse_file(source).unwrap()];
        let assets = prepare(
            &mut files,
            &["module.rs".into()],
            &[Some(origin.clone())],
            &[],
        )
        .unwrap();
        assert_eq!(assets.inputs.len(), 1);
        assert_eq!(assets.inputs[0].data, data);
        assert_eq!(
            assets.inputs[0].path,
            root.join("bytes.bin").canonicalize().unwrap()
        );
        let syn::Item::Const(c) = &files[0].items[0] else {
            panic!()
        };
        let literal = syn::LitByteStr::new(data, proc_macro2::Span::call_site());
        let expected: syn::Expr = parse_quote!(#literal);
        use quote::ToTokens;
        assert_eq!(
            c.expr.to_token_stream().to_string(),
            expected.to_token_stream().to_string()
        );
    }
    assert!(place(source, &[]).unwrap_err().contains("origin"));
    assert!(place("const X:&[u8]=include_bytes!(\"missing.bin\");", &[]).is_err());
}

#[test]
fn const_asset_and_named_alignment_share_the_static_placement_rules() {
    let source = r#"
const SECTOR:usize=128;
const ATLAS_ALIGNMENT:usize=SECTOR*512;
#[asset(align=ATLAS_ALIGNMENT)]const A:AssetBytes=AssetBytes::new(b"abc");
#[asset(align=SECTOR*2)]static B:AssetBytes=AssetBytes::new(b"de");
"#;
    let assets = place(source, std::slice::from_ref(&(0x100080..0x120000))).unwrap();
    assert_eq!(assets.placed[0].address, 0x110000);
    assert_eq!(assets.placed[0].alignment, 65536);
    assert_eq!(assets.placed[1].address, 0x100100);
    assert!(place(
        &source.replace("SECTOR*512", "SECTOR*3"),
        std::slice::from_ref(&(0x100080..0x120000))
    )
    .unwrap_err()
    .contains("power of two"));
}
