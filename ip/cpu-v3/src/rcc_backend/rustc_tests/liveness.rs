//! Storage reachability retains full-source ownership checks and implicit effects.
use super::{execute, parse, parse_with_options};
use crate::rcc_backend::CompilerOptions;
use rcc::frontend::static_assets::AssetBank;

#[test]
fn rustc_unused_storage_and_literals_do_not_consume_the_static_arena() {
    let program = parse(
        "dead-static-storage",
        r#"
static UNUSED:Buf<u16,20000>=Buf::new([4;20000]);
fn dead(){let bytes=b"UNREFERENCED TEXT";UNUSED.as_array()[0u16]=bytes[0] as u16;}
fn main(){halt(0x600d);}
"#,
    )
    .unwrap();
    assert_eq!(
        program.debug.static_data_end,
        usize::from(CompilerOptions::default().data_base)
    );
    let target =
        crate::rcc_backend::try_compile(program, &CompilerOptions::default(), "main").unwrap();
    assert!(!target.debug.functions.iter().any(|f| f.name == "dead"));
    let error = parse(
        "dead-code-still-checked",
        r#"
fn dead(){let mut x=1;let a=&mut x;let b=&mut x;*a+=*b;}
fn main(){halt(0x600d);}
"#,
    )
    .err()
    .expect("unreachable source bypassed rustc")
    .to_string();
    assert!(error.contains("E0499"), "{error}");
}

#[test]
fn rustc_storage_dependencies_include_indirect_calls_and_drop_bodies() {
    execute(
        "storage-indirect-drop",
        r#"
static VALUE:u16=2;
#[rcc_resource]struct Guard{v:u16}
impl Drop for Guard{fn drop(&mut self){unsafe{addr_of(&VALUE).write(0,VALUE+self.v);}}}
fn target()->u16{VALUE}
fn invoke(f:fn()->u16)->u16{f()}
fn main(){ {let guard=Guard{v:5};} if invoke(target)!=7{halt(0xbad);}halt(0x600d);}
"#,
    );
}

#[test]
fn rustc_dead_assets_validate_without_reserving_space() {
    let opts = CompilerOptions {
        asset_banks: vec![AssetBank {
            bytes: 0x100000..0x100080,
        }],
        ..CompilerOptions::default()
    };
    let source = r#"
#[derive(Copy,Clone)]struct AssetBytes{first_sector:u16,sector_count:u16,byte_len_lo:u16,byte_len_hi:u16}
impl AssetBytes{const fn new(bytes:&[u8])->Self{Self{first_sector:0,sector_count:1,byte_len_lo:bytes.len() as u16,byte_len_hi:0}}}
#[asset]const USED:AssetBytes=AssetBytes::new(b"used");
#[asset(align=65536)]const UNUSED:AssetBytes=AssetBytes::new(b"unused");
fn main(){halt(USED.first_sector);}
"#;
    let program = parse_with_options("dead-asset-placement", source, &opts).unwrap();
    assert_eq!(program.assets.placed.len(), 1);
    assert_eq!(program.assets.placed[0].data, b"used");
    let macro_source = source.replace("fn main(){halt(USED.first_sector);}",
        "struct Reader{x:u16}impl Reader{fn sector()->u16{USED.first_sector}}fn main(){assert!(Reader::sector()!=0);halt(0x600d);}");
    assert_eq!(
        parse_with_options("asset-macro-method-dependency", &macro_source, &opts)
            .unwrap()
            .assets
            .placed
            .len(),
        1
    );
    let mut linked = opts.clone();
    linked.asset_banks[0].bytes.end += 65536;
    linked
        .asset_dependencies
        .push(("USED".into(), "UNUSED".into()));
    assert_eq!(
        parse_with_options("asset-pointer-dependency", source, &linked)
            .unwrap()
            .assets
            .placed
            .len(),
        2
    );
    let invalid = source.replace("align=65536", "align=3");
    let error = parse_with_options("dead-asset-invalid", &invalid, &opts)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("alignment"), "{error}");
}

#[test]
fn rustc_dead_working_buffers_do_not_exhaust_live_capacity() {
    let opts = CompilerOptions {
        asset_banks: vec![AssetBank {
            bytes: 0x100000..0x100080,
        }],
        ..CompilerOptions::default()
    };
    let source = r#"
#[derive(Copy,Clone)]struct StaticBuffer{first_sector:u16,sector_count:u16}
impl StaticBuffer{const fn new(bytes:usize,align:usize)->Self{Self{first_sector:0,sector_count:((bytes+127)/128) as u16}}}
#[static_buffer]const USED:StaticBuffer=StaticBuffer::new(128,128);
#[static_buffer]const UNUSED:StaticBuffer=StaticBuffer::new(1048576,65536);
fn main(){halt(USED.first_sector);}
"#;
    let program = parse_with_options("dead-working-buffer", source, &opts).unwrap();
    assert_eq!(program.assets.buffers.len(), 1);
    assert_eq!(program.assets.buffers[0].bytes, 128);
}
