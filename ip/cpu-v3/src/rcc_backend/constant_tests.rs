//! Const source is checked by rustc, folded by RCC, and executed on CPU V3.
use super::{try_compile, CompilerOptions};
use crate::{CpuV3Sim, RunOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::path::PathBuf;

fn parse(label: &str, source: &str) -> Result<rcc::frontend::Program, String> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/sdk-constexpr/rustc")
        .join(label);
    compile_program_named_checked(
        label,
        source,
        &CompilerOptions::default(),
        &RustcCheck::new(directory),
        &mut |name| Err(format!("unexpected module {name}")),
    )
    .map_err(|e| e.to_string())
}

#[test]
fn constant_struct_functions_fields_updates_and_runtime_calls_execute() {
    let source = r#"
#[derive(Copy,Clone)] struct Point{x:u16,y:i16}
impl Point{const fn new(x:u16,y:i16)->Self{Self{x:x,y:y}}}
const fn shifted(p:Point,dx:u16)->Point{
 let x:u16=p.x+dx;
 let y:i16=if p.y<0{p.y*2}else{p.y};
 Point{x:x,y:y}
}
const ORIGIN:Point=Point::new(LATER,-3);
const LATER:u16=11;
const MOVED:Point=shifted(ORIGIN,7);
const UPDATED:Point=Point{x:29,..MOVED};
static STORED:Point=shifted(UPDATED,2);
fn main(){
 let p:Point=MOVED;
 if p.x!=18||p.y!= -6{halt(1);}
 if STORED.x!=31||STORED.y!= -12{halt(2);}
 let local:Point=Point{x:100,y:4};
 if local.x!=100{halt(3);}
 let runtime:Point=shifted(Point::new(8,-2),9);
 if runtime.x!=17||runtime.y!= -4{halt(4);}
 halt(0x600d);
}
"#;
    let ir = parse("struct-constants", source).unwrap();
    // Constants have no storage. Only the explicit two-word static is allocated.
    assert_eq!(
        ir.debug
            .globals
            .iter()
            .filter(|g| g.name == "STORED")
            .count(),
        1
    );
    assert!(!ir
        .debug
        .globals
        .iter()
        .any(|g| matches!(g.name.as_str(), "ORIGIN" | "MOVED" | "UPDATED")));
    let code = try_compile(ir, &CompilerOptions::default(), "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    assert!(matches!(
        cpu.run(100_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

#[test]
fn constant_bytes_length_index_cast_and_short_circuit_execute() {
    let source = r#"
const BYTES:&[u8]=b"abcde";
const TABLE:[u8;SIZE]=[7;SIZE];
const SIZE:usize=5;
const fn count(bytes:&[u8])->u16{bytes.len() as u16}
const COUNT:u16=count(BYTES);
const FIRST:u16=BYTES[0] as u16;
const SAFE:bool=true||BYTES[99]==0;
struct Info{length:u16,first:u16}
static INFO:Info=Info{length:COUNT,first:FIRST};
fn main(){if INFO.length!=5||INFO.first!=97||!SAFE||TABLE[4]!=7{halt(1);}halt(0x600d);}
"#;
    let code = try_compile(
        parse("byte-constants", source).unwrap(),
        &CompilerOptions::default(),
        "main",
    )
    .unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    assert!(matches!(
        cpu.run(100_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

#[test]
fn constant_signed_tuple_bool_and_shift_width_match_rust() {
    let source = r#"
const PAIR:(u16,i16)=(9,-7);
const HIGH:i16=1i16<<15;
const LOW:u16=0x8000u16<<1;
const MIN:i16=-32768i16;
#[repr(u16)]#[derive(Copy,Clone)]enum Mode{Off,On}
const MODE:Mode=Mode::On;
const MODE_BITS:u16=MODE as u16;
const fn n(x:i16)->i16{x/2}
static T:(u16,i16)=(PAIR.0,n(PAIR.1));
static FLAG:bool=true;
struct State{flag:bool,bits:i16,mode:Mode}
static S:State=State{flag:false,bits:HIGH>>2,mode:MODE};
fn main(){if T.0!=9||T.1!= -3||LOW!=0||S.bits!= -8192||S.flag||!FLAG||MODE_BITS!=1||S.mode as u16!=1||MIN/2!= -16384{halt(1);}halt(0x600d);}
"#;
    let code = try_compile(
        parse("signed-constants", source).unwrap(),
        &CompilerOptions::default(),
        "main",
    )
    .unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    assert!(matches!(
        cpu.run(100_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

#[test]
fn constant_type_ownership_and_const_legality_fail_in_rustc_first() {
    for (name, source, code) in [
        (
            "type",
            "struct P{x:u16} const P0:P=P{x:true};fn main(){}",
            "E0308",
        ),
        (
            "move",
            "#[rcc_resource]struct P{x:u16}fn bad(p:P){drop(p);drop(p);}fn main(){}",
            "E0382",
        ),
        (
            "nonconst",
            "fn n()->u16{1}const X:u16=n();fn main(){}",
            "E0015",
        ),
        ("overflow", "const X:u16=65535+1;fn main(){}", "E0080"),
    ] {
        let error = parse(name, source)
            .err()
            .expect("invalid const source accepted");
        assert!(error.contains(code), "{name}: {error}");
    }
}

#[test]
fn constant_external_asset_keeps_blob_outside_cpu_statics_and_loads_descriptor() {
    use rcc::frontend::{
        compile_program_named_with_libraries_checked, static_assets::AssetBank, ModuleSource,
    };
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/sdk-constexpr/external");
    std::fs::create_dir_all(&root).unwrap();
    let bytes: Vec<u8> = (0..70_001).map(|i| (i * 17) as u8).collect();
    std::fs::write(root.join("data.bin"), &bytes).unwrap();
    let source = r#"
mod static_assets;use static_assets::*;
#[asset(align=256)]
static DATA:AssetBytes=AssetBytes::new(include_bytes!("data.bin"));
struct Desc{sector:u16,words:u16,lo:u16,hi:u16,valid:bool}
const fn descriptor(data:AssetBytes)->Desc{
 assert!(data.sector_count==547);
 Desc{sector:data.first_sector,words:data.sector_count,lo:data.byte_len_lo,hi:data.byte_len_hi,valid:true}
}
static DESC:Desc=descriptor(DATA);
fn main(){if DESC.lo!=4465||DESC.hi!=1||DESC.words!=547||!DESC.valid{halt(1);}halt(DESC.sector);}
"#;
    let entry = root.join("main.rs");
    for (label, first, expected) in [
        ("first", 0x10_0083, 0x2002),
        ("relocated", 0x11_0083, 0x2202),
    ] {
        let source = if label == "relocated" {
            source.replace("#[asset(align=256)]", "#[asset]")
        } else {
            source.to_owned()
        };
        std::fs::write(&entry, &source).unwrap();
        let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
        options.asset_banks = vec![AssetBank {
            bytes: first..0x14_0000,
        }];
        let mut ir = compile_program_named_with_libraries_checked(
            &entry.to_string_lossy(),
            &source,
            &options,
            &[],
            &RustcCheck::new(root.join(label)),
            &mut |name| {
                let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../systems/cpu-v3-tang-nano-20k/rcc")
                    .join(format!("{name}.rs"));
                Ok(ModuleSource {
                    namespace: rcc::frontend::ModuleNamespace::Flat,
                    source: std::fs::read_to_string(&path).map_err(|e| e.to_string())?,
                    origin: Some(path),
                })
            },
        )
        .unwrap();
        assert!(
            ir.debug.static_data_end < 32,
            "blob leaked into CPU statics"
        );
        assert_eq!(ir.assets.placed.len(), 1);
        assert_eq!(ir.assets.placed[0].data, bytes);
        assert_eq!(ir.assets.placed[0].address, expected as usize * 128);
        assert_eq!(ir.assets.inputs[0].data, bytes);
        let data = ir.take_static_initializers();
        assert!(!ir
            .funcs
            .iter()
            .flat_map(|f| &f.blocks)
            .flat_map(|b| &b.insts)
            .any(|i| matches!(i, rcc::Instr::StoreStatic { .. })));
        let code = try_compile(ir, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        for (address, value) in data {
            cpu.physical_memory_mut()[address as usize] = value;
        }
        assert!(
            matches!(cpu.run(100_000).unwrap(),RunOutcome::Halted{signal,..} if signal==expected)
        );
    }
}

#[test]
fn decimal_fix_macro_is_rust_checked_constant_and_instruction_identical_to_raw_words() {
    const HOST: rcc::dsl_rt::fix32 = rcc::fix!(-1.25);
    assert_eq!(HOST.0, -81920);
    let source = r#"
const RATE:fix32=fix!(0.001);
struct Settings{scale:fix32}
static SETTINGS:Settings=Settings{scale:fix!(-1.25)};
const fn quarter()->fix32{fix!(1.25e-1)}
fn main(){
 let half=fix!(0.5,);
 assert!(half.lo_bits()==32768 && half.hi_bits()==0);
 assert!(fix!(0.1).lo_bits()==6554);
 if RATE.lo_bits()!=66 || SETTINGS.scale.hi_bits()!=65534 || SETTINGS.scale.lo_bits()!=49152 {halt(1);}
 if quarter().lo_bits()!=8192 {halt(2);}
 let min=fix!(-32768);if min.hi_bits()!=32768 || min.lo_bits()!=0 {halt(3);}
 halt(0x600d);
}
"#;
    let raw = source
        .replace("fix!(0.001)", "fix32::from_words(66,0)")
        .replace("fix!(-1.25)", "fix32::from_words(49152,65534)")
        .replace("fix!(1.25e-1)", "fix32::from_words(8192,0)")
        .replace("fix!(0.5,)", "fix32::from_words(32768,0)")
        .replace("fix!(0.1)", "fix32::from_words(6554,0)")
        .replace("fix!(-32768)", "fix32::from_words(0,32768)");
    let code = try_compile(
        parse("decimal-literals", source).unwrap(),
        &CompilerOptions::default(),
        "main",
    )
    .unwrap();
    let reference = try_compile(
        parse("decimal-raw-reference", &raw).unwrap(),
        &CompilerOptions::default(),
        "main",
    )
    .unwrap();
    assert_eq!(code.words, reference.words);
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    assert!(matches!(
        cpu.run(100_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

#[test]
fn decimal_fix_macro_rejects_ranges_suffixes_and_nonliteral_expressions() {
    for literal in [
        "32768", "-32769", "0.1f32", "0x10", "value", "1+2", "(0.5)", "\"0.5\"", "true", "0.1,0.2",
    ] {
        let source = format!("fn main(){{let value:u16=2;let number=fix!({literal});halt(0);}}");
        let error = parse("invalid-decimal-literal", &source)
            .err()
            .expect("invalid fixed literal accepted");
        assert!(error.contains("fix!"), "{literal}: {error}");
    }
}

#[test]
fn static_buffers_are_checked_placed_once_and_materialize_ordinary_descriptions() {
    use rcc::frontend::{
        compile_program_named_with_libraries_checked, static_assets::AssetBank, ModuleSource,
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/sdk-buffers/compiler");
    std::fs::create_dir_all(&root).unwrap();
    let options = CompilerOptions {
        asset_banks: vec![AssetBank {
            bytes: 0x100083..0x110000,
        }],
        ..CompilerOptions::for_separate_code_and_data_segments(0)
    };
    let source = r#"
mod static_buffer;use static_buffer::*;
mod wrapper;use wrapper::*;
#[static_buffer(align=4096)]const FIRST:Wrapper=Wrapper::allocate();
#[static_buffer]const ALIAS:Wrapper=FIRST;
static DESC:Wrapper=FIRST;
fn main(){halt(DESC.address());}
"#;
    let compile = |source: &str| {
        compile_program_named_with_libraries_checked(
            "static buffers",
            source,
            &options,
            &[],
            &RustcCheck::new(root.clone()),
            &mut |name| {
                if name == "wrapper" {
                    return Ok(ModuleSource {
                        namespace: rcc::frontend::ModuleNamespace::Scoped,
                        source: r#"
use crate::static_buffer::*;
#[derive(Copy,Clone)]pub struct Wrapper{storage:StaticBuffer}
impl Wrapper {
 pub const fn allocate()->Self{Self{storage:StaticBuffer::new(129,128)}}
 pub const fn address(self)->u16{self.storage.first_sector()}
 pub const fn from_buffer(storage:StaticBuffer)->Self{Self{storage}}
}
"#
                        .into(),
                        origin: None,
                    });
                }
                let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../systems/cpu-v3-tang-nano-20k/rcc")
                    .join(format!("{name}.rs"));
                Ok(ModuleSource {
                    namespace: rcc::frontend::ModuleNamespace::Flat,
                    source: std::fs::read_to_string(&path).map_err(|e| e.to_string())?,
                    origin: Some(path),
                })
            },
        )
    };
    let mut ir = compile(source).unwrap();
    assert_eq!(ir.assets.buffers.len(), 1);
    assert_eq!(ir.assets.buffers[0].address, 0x101000);
    assert_eq!(ir.assets.buffers[0].bytes, 256);
    let data = ir.take_static_initializers();
    let code = try_compile(ir, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    for (address, value) in data {
        cpu.physical_memory_mut()[address as usize] = value;
    }
    assert!(matches!(
        cpu.run(10000).unwrap(),
        RunOutcome::Halted { signal: 0x2020, .. }
    ));
    for bad in [
        source.replace(
            "Wrapper::allocate()",
            "Wrapper::from_buffer(StaticBuffer::from_sectors(1,2))",
        ),
        source.replace("#[static_buffer(align=4096)]", ""),
        source.replace(
            "Wrapper::allocate()",
            "unsafe { StaticBuffer::from_sectors(1,2) }",
        ),
        source.replace(
            "halt(DESC.address());",
            "let b=StaticBuffer::new(128,128);halt(b.first_sector());",
        ),
    ] {
        assert!(
            compile(&bad).is_err(),
            "invalid original source passed rustc/RCC: {bad}"
        );
    }
}

#[test]
fn aggregate_conditionals_keep_lazy_scopes_and_argument_snapshots() {
    let source = r#"
#[derive(Copy,Clone)]struct Pair{x:u16,y:u16}
static DATA:Pair=Pair{x:7,y:11};static LOG:u16=0;
fn event(value:u16)->u16{unsafe{addr_of(&LOG).write(0,LOG*10+value);}value}
fn choose(flag:bool)->Pair{if flag {let x=event(1);Pair{x,y:2}}else{let x=event(3);Pair{x,y:4}}}
fn overwrite()->u16{unsafe{addr_of(&DATA).write(0,77);}0}
fn observe(pair:Pair,ignored:u16)->u16{pair.x+pair.y}
fn main(){
 let yes:Pair=choose(true);let no:Pair=choose(false);
 if yes.x!=1||yes.y!=2||no.x!=3||no.y!=4||LOG!=13{halt(1);}
 let total=observe(if event(2)==2 {DATA}else{Pair{x:0,y:0}},overwrite());
 if total!=18||DATA.x!=77||LOG!=132{halt(2);}halt(0x600d);
}
"#;
    let options = CompilerOptions::default();
    let program = try_compile(parse("aggregate-if", source).unwrap(), &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    assert!(matches!(
        cpu.run(200000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

#[test]
fn fixed_const_math_and_dynamic_intrinsics_share_types_without_host_projection_exceptions() {
    const HOST: rcc::dsl_rt::fix32 = rcc::dsl_rt::frcp(rcc::fix!(2));
    assert_eq!(HOST.0, 32768);
    let source = r#"
const HALF:fix32=frcp(fix!(2));
const QUARTER_TURN:fix32=fix!(90).to_radians();
const SINE:fix32=fsin(QUARTER_TURN);
const ROOT:fix32=frsqrt(fix!(4));
const fn wave()->fix32{let pair=fsincos(fix!(0));pair.x().wrapping_add(pair.y())}
static PREFIX:u16=0x1234;static VALUE:fix32=wave();
fn scaled(value:fix32)->fix32{value.wrapping_mul(fix!(2)).wrapping_add(fix!(0.5))}
fn compare(value:fix32)->bool{value.less_than(fix!(2)) && value.greater_or_equal(fix!(1)) && !value.equals(fix!(0))}
fn main(){
 if PREFIX!=0x1234||HALF!=fix!(0.5)||SINE!=fix!(1)||ROOT!=fix!(0.5)||VALUE!=fix!(1){halt(1);}
 if scaled(fix!(1.25))!=fix!(3)||!compare(fix!(1.5)){halt(2);}
 let negative=fix!(1).wrapping_neg();if negative!=fix!(-1){halt(3);}
 halt(0x600d);
}
"#;
    let options = CompilerOptions::default();
    let program =
        try_compile(parse("fixed-const-math", source).unwrap(), &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    assert!(matches!(
        cpu.run(200000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

#[test]
fn runtime_radians_initializes_both_coefficient_halves() {
    let source = r#"
static DEGREES:fix32=fix!(60);
fn main(){
 let radians=DEGREES.to_radians();
 if radians.lo_bits()!=3104||radians.hi_bits()!=1{halt(1);}
 halt(0x600d);
}
"#;
    for opt in [rcc::Opts::default(), rcc::Opts::disabled()] {
        let options = CompilerOptions {
            opt,
            ..CompilerOptions::default()
        };
        let program =
            try_compile(parse("runtime-radians", source).unwrap(), &options, "main").unwrap();
        for high in [0xffff, 0x1234] {
            // Execute real instructions to dirty every possible F-register
            // allocation before entering the compiled program.
            let mut seed = crate::load_immediate16(1, high).to_vec();
            for fd in 0..64 {
                seed.extend(crate::fpu_aux(
                    crate::FpuAuxKind::IntegerRegister,
                    1,
                    0,
                    fd,
                    crate::FpuAuxSubop::I16tof,
                    0,
                ));
            }
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&seed);
            for _ in 0..seed.len() {
                if usize::from(cpu.pc()) == seed.len() {
                    break;
                }
                cpu.step().unwrap();
            }
            assert_eq!(usize::from(cpu.pc()), seed.len());
            assert!(cpu
                .fpu_registers()
                .iter()
                .all(|value| (*value as u32 >> 16) == u32::from(high)));
            cpu.set_boot_window(&program.words);
            cpu.set_pc(0);
            assert!(
                matches!(
                    cpu.run(10000).unwrap(),
                    RunOutcome::Halted { signal: 0x600d, .. }
                ),
                "coefficient inherited F-register high half {high:#06x}"
            );
        }
    }
}
