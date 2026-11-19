//! Bounded source-level examples for placement, far ABI and near compatibility.
use super::*;
use crate::{CpuV3Sim, RunOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::path::Path;
const ROOT: &str = r#"#![link_csegs(default="app", app=7, math=6, nested=5, same=7)]
mod math;use math::*;
static TOTAL:u16=0;
struct Pair{a:u16,b:u16}
#[inline(never)]fn back(n:u16)->u16{n+3}
#[link_cseg="same"]fn same(n:u16)->u16{n+1}
fn main(){
 let before=read_dseg_page(3);
 let n=six(1,2,3,4,5,6);let p=pair(1,2,3,4,5);let f=fixed(fix32::from_int(3));
 if n!=21 || p.a!=15 || p.b!=30 || f.to_int()!=12 || recurse(4)!=11 || same(8)!=9 || TOTAL!=10 || read_dseg_page(3)!=before {halt(0xbad);}
 halt(0x600d);
}
"#;
const MATH: &str = r#"#![link_cseg="math"]
use crate::{Pair,back,TOTAL};
# [inline(never)]pub fn six(a:u16,b:u16,c:u16,d:u16,e:u16,f:u16)->u16{a+b+c+d+e+f}
#[inline(never)]pub fn pair(a:u16,b:u16,c:u16,d:u16,e:u16)->Pair{Pair{a:a+b+c+d+e,b:(a+b+c+d+e)*2}}
#[inline(never)]pub fn fixed(a:fix32)->fix32{a*fix32::from_int(4)}
#[link_cseg="nested"]pub fn recurse(n:u16)->u16{if n==0{return back(0);}unsafe{addr_of(&TOTAL).write(0,TOTAL+n);}recurse(n-1)+2}
"#;
fn checked(
    source: &str,
    label: &str,
    options: &CompilerOptions,
) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/sdk-cseg/checks")
        .join(label);
    compile_program_named_checked(label, source, options, &RustcCheck::new(out), &mut |name| {
        if name == "math" {
            Ok(MATH.into())
        } else {
            Err(format!("unknown module {name}"))
        }
    })
}
#[test]
fn cseg_far_calls_preserve_arguments_returns_recursion_and_mapping() {
    // code_base 10/11 puts the first math entry at 15/16; base15 also
    // widens the return offset. High banks exercise wide CSEG constants.
    for (automatic, base, math, nested) in [
        (false, 0, 6, 5),
        (true, 0, 6, 5),
        (true, 10, 6, 5),
        (true, 11, 6, 5),
        (true, 15, 16, 17),
    ] {
        let source = ROOT
            .replace("math=6", &format!("math={math}"))
            .replace("nested=5", &format!("nested={nested}"));
        let mut options = CompilerOptions::for_separate_code_and_data_segments(base);
        options.inline.automatic = automatic;
        let p = try_compile(
            checked(&source, &format!("abi-{automatic}-{base}-{math}"), &options).unwrap(),
            &options,
            "main",
        )
        .unwrap();
        assert_eq!(p.cseg, Some(7));
        assert_eq!(p.code_segments.len(), 2);
        let mut cpu = CpuV3Sim::with_initial_code_segment(1 << 22, 7);
        cpu.load_segment(7, p.code_base, &p.words).unwrap();
        cpu.set_pc(base);
        for s in &p.code_segments {
            cpu.load_segment(s.cseg, s.code_base, &s.words).unwrap();
        }
        assert!(
            matches!(
                cpu.run(100_000).unwrap(),
                RunOutcome::Halted { signal: 0x600d, .. }
            ),
            "{}",
            p.listing
        );
        assert_eq!(cpu.code_segment(), 7);
        let frame = p
            .debug
            .functions
            .iter()
            .find(|f| f.name == "main")
            .unwrap()
            .frame_size;
        assert_eq!(cpu.registers()[13], options.stack_init - frame as u16);
        assert!(p
            .code_segments
            .iter()
            .find(|s| s.cseg == math)
            .unwrap()
            .debug
            .functions
            .iter()
            .any(|f| f.name == "six"));
    }
}
#[test]
fn cseg_validation_and_unannotated_bytes() {
    let options = CompilerOptions::for_separate_code_and_data_segments(0);
    for (source, message) in [
        ("#![link_csegs(app=7)] fn main(){}", "requires a default"),
        (
            "#![link_csegs(default=\"app\", app=7)] #[link_cseg=\"missing\"]fn main(){}",
            "undeclared CSEG",
        ),
        ("#[link_cseg=\"math\"]fn main(){}", "undeclared CSEG"),
        (
            "#![link_csegs(default=\"app\", app=7)] #[link_cseg=\"app\"]static X:u16=0;fn main(){}",
            "metadata belongs",
        ),
    ] {
        let error = checked(source, "invalid", &options)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(message), "{error}");
    }
    let source = "fn add(a:u16)->u16{a+1}fn main(){halt(add(41));}";
    let a = try_compile(
        checked(source, "plain-a", &options).unwrap(),
        &options,
        "main",
    )
    .unwrap();
    let b = try_compile(
        checked(source, "plain-b", &options).unwrap(),
        &options,
        "main",
    )
    .unwrap();
    assert_eq!(a.words, b.words);
    assert!(a.code_segments.is_empty());
    assert_eq!(a.cseg, None);
    let ptr = ROOT[..ROOT.find("fn main()").unwrap()].to_owned()
        + "fn main(){let p:fn(u16)->u16=back;halt(recurse(p(0)));}";
    let ir = checked(&ptr, "pointer", &options).unwrap();
    assert!(try_compile(ir, &options, "main")
        .unwrap_err()
        .to_string()
        .contains("near function pointer"));
}
#[test]
fn cseg_capacity_is_per_window() {
    use super::lowering::{Line, LoweredFunction};
    let function = |name, cseg, count| LoweredFunction {
        trailing_guard: true,
        name,
        cseg: Some(cseg),
        lines: vec![
            Line::Word {
                word: crate::halt(),
                line: None
            };
            count
        ],
        static_addresses: vec![],
        frame_size: 0,
        local_base: 0,
    };
    let options = CompilerOptions::for_separate_code_and_data_segments(0);
    let debug = rcc::frontend::FrontendDebug {
        default_cseg: Some(7),
        ..Default::default()
    };
    let p = super::cseg::link(
        vec![function("main", 7, 40000), function("other", 6, 40000)],
        &options,
        debug.clone(),
    )
    .unwrap();
    assert!(p.total_code_words() > 65536);
    assert!(p.words.len() < 65536);
    assert!(p.code_segments[0].words.len() < 65536);
    let error = super::cseg::link(vec![function("main", 7, 65536)], &options, debug).unwrap_err();
    assert!(matches!(
        error,
        BackendError::Resource(BackendResourceError {
            kind: BackendResourceKind::CodeWindow,
            ..
        })
    ));
}

#[test]
fn cseg_inline_copies_follow_callers_and_preserve_shared_state() {
    use rcc::{inline_calls, InlineDecision};
    let source = r#"#![link_csegs(default="app",app=7,helpers=6,work=5)]
static COUNT:Buf<u16,1>=Buf::new([0;1]);
#[link_cseg="helpers"]#[inline(never)]fn read_count()->u16{COUNT[0u16]}
#[link_cseg="helpers"]#[inline]fn bump(n:u16)->u16{COUNT[0u16]+=1;n+read_count()}
#[link_cseg="work"]#[inline(never)]fn worker()->u16{bump(20)}
#[link_cseg="helpers"]#[inline(never)]fn retained()->u16{bump(30)}
fn main(){let a=bump(10);let b=worker();let c=retained();if a!=11||b!=22||c!=33||COUNT[0u16]!=3{halt(0xbad);}halt(0x600d);}
"#;
    let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
    options.inline.automatic = false;
    options.inline.maximum_depth = 2;
    for keep_body in [true, false] {
        let ir = checked(source, &format!("inline-placement-{keep_body}"), &options).unwrap();
        let program = if keep_body {
            // Select two callers to exercise partial inlining. The original body
            // remains needed by `retained`, whose call is deliberately untouched.
            let mut functions: HashMap<_, _> =
                ir.funcs.iter().cloned().map(|f| (f.name, f)).collect();
            let report = inline_calls(
                &mut functions,
                &HashSet::from(["main", "worker"]),
                &HashSet::from(["main"]),
                &options.inline,
            )
            .unwrap();
            assert_eq!(
                report
                    .sites
                    .iter()
                    .filter(|s| s.callee == "bump" && s.decision == InlineDecision::Expanded)
                    .count(),
                2
            );
            for (name, bank) in [("main", 7), ("worker", 5), ("bump", 6), ("retained", 6)] {
                assert_eq!(functions[name].cseg, Some(bank));
            }
            assert!(function_callees(&functions["retained"]).any(|name| name == "bump"));
            let mut ir = ir;
            for f in &mut ir.funcs {
                *f = functions.remove(f.name).unwrap();
            }
            let mut no_more_inline = options.clone();
            no_more_inline.inline.enabled = false;
            try_compile(ir, &no_more_inline, "main").unwrap()
        } else {
            let (program, report) = try_compile_with_inline_report(ir, &options, "main").unwrap();
            assert_eq!(report.retained_sites, 3);
            program
        };
        assert_eq!(program.cseg, Some(7));
        let helper = program.code_segments.iter().find(|s| s.cseg == 6).unwrap();
        assert_eq!(
            helper.debug.functions.iter().any(|f| f.name == "bump"),
            keep_body
        );
        assert!(helper.debug.functions.iter().any(|f| f.name == "retained"));
        assert!(helper
            .debug
            .functions
            .iter()
            .any(|f| f.name == "read_count"));
        assert!(program
            .code_segments
            .iter()
            .find(|s| s.cseg == 5)
            .unwrap()
            .debug
            .functions
            .iter()
            .any(|f| f.name == "worker"));
        let mut cpu = CpuV3Sim::with_initial_code_segment(1 << 22, 7);
        cpu.load_segment(7, program.code_base, &program.words)
            .unwrap();
        for s in &program.code_segments {
            cpu.load_segment(s.cseg, s.code_base, &s.words).unwrap();
        }
        assert!(matches!(
            cpu.run(100_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ));
        assert_eq!(cpu.code_segment(), 7);
    }
}
