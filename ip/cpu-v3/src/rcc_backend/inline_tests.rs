use super::*;
use crate::{CpuV3Sim, RunOutcome};
use rcc::frontend::{compile_program_named, parse_source_with, parse_source_with_config};
fn build(source: &str, options: &CompilerOptions) -> (CpuV3Program, InlineCompilationReport) {
    try_compile_with_inline_report(
        parse_source_with_config(source, options).unwrap(),
        options,
        "main",
    )
    .unwrap()
}
fn run(program: &CpuV3Program) -> (u16, usize) {
    let mut sim = CpuV3Sim::default();
    sim.load_program(program.code_base, &program.words).unwrap();
    sim.set_pc(program.code_base);
    match sim.run(250_000).unwrap() {
        RunOutcome::Halted { signal, steps } => (signal, steps),
        other => panic!("bounded inline test failed: {other:?}"),
    }
}
fn compare(source: &str, expected: u16) -> InlineCompilationReport {
    let options = CompilerOptions::default();
    let (on, report) = build(source, &options);
    let mut off = options.clone();
    off.inline.enabled = false;
    let (off, _) = build(source, &off);
    assert_eq!(run(&off).0, expected);
    assert_eq!(run(&on).0, expected);
    assert_eq!(report.outcome, InlineOutcome::Applied, "{report}");
    assert!(report.retained_sites > 0);
    report
}
#[test]
fn cfg_loops_multi_return_and_caller_phi_execute_equally() {
    compare("#[inline] fn choose(input:u16)->u16{let mut x=input;while x<4{x=x+1;}if x>9{return x+2;}x+3} fn main(){let a:u16=if choose(1)>6{choose(10)}else{choose(3)};halt(a);}",12);
}
#[test]
fn evaluated_arguments_and_void_memory_effects_are_preserved() {
    compare("static COUNT:u16=0; #[inline(never)] fn bump()->u16{unsafe{addr_of(&COUNT).write(0,COUNT+1);} COUNT} #[inline] fn take(a:u16,b:u16)->u16{a*10+b} fn main(){let a=take(bump(),bump());halt(a*10+COUNT);}",122);
    compare("static COUNT:u16=0; #[inline] fn write(v:u16){unsafe{addr_of(&COUNT).write(0,v);}} fn main(){write(7);let old=COUNT;write(9);halt(old*10+COUNT);}",79);
}
#[test]
fn fpu_scalar_and_vector_returns_execute_equally() {
    let report=compare("#[inline] fn twice(x:fix32)->fix32{x+x} #[inline] fn offset(v:vec3)->vec3{v+vec3::new(fix32::from_int(1),fix32::from_int(2),fix32::from_int(3))} fn main(){let a=twice(fix32::from_int(4));let b=offset(vec3::new(a,fix32::from_int(2),fix32::from_int(3)));halt((b.x()+b.y()+b.z()).to_int() as u16);}",19);
    assert!(report
        .frames
        .iter()
        .any(|f| f.candidate_fpu_spill <= f.baseline_fpu_spill));
}
#[test]
fn cross_module_debug_uses_caller_callsite_and_reports_origins() {
    let source = "mod helpers;\nfn main(){\nlet value=remote(7);\nhalt(value);\n}";
    let module = format!(
        "{}#[inline] pub fn remote(x:u16)->u16{{x+1}}",
        "\n".repeat(90)
    );
    let options = CompilerOptions::default();
    let frontend = compile_program_named("caller-inline.rs", source, &options, &mut |n| {
        if n == "helpers" {
            Ok(module.clone())
        } else {
            Err(n.into())
        }
    })
    .unwrap();
    let (program, report) = try_compile_with_inline_report(frontend, &options, "main").unwrap();
    assert_eq!(run(&program).0, 8);
    assert_eq!(report.outcome, InlineOutcome::Applied);
    assert!(!program.debug.functions.iter().any(|f| f.name == "remote"));
    let main = program
        .debug
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap();
    assert!(program
        .debug
        .lines
        .iter()
        .filter(|(pc, _, _)| *pc >= main.addr.0 && *pc < main.addr.1)
        .all(|(_, file, line)| *file == main.file && *line < 90));
    let index = report
        .ir
        .sites
        .iter()
        .position(|s| s.callee == "remote")
        .unwrap();
    assert_eq!(report.ir.sites[index].line, Some(3));
    assert_eq!(
        report.origins[index].caller_file.as_deref(),
        Some("caller-inline.rs")
    );
    assert_ne!(
        report.origins[index].callee_file,
        report.origins[index].caller_file
    );
}
#[test]
fn function_addresses_keep_the_original_body_and_indirect_call() {
    let source="#[inline] fn add(a:u16,b:u16)->u16{a+b} fn main(){let f:fn(u16,u16)->u16=add;let a=add(2,3);halt(a+f(4,5));}";
    let (program, report) = build(source, &CompilerOptions::default());
    assert_eq!(run(&program).0, 14);
    assert_eq!(report.retained_sites, 1);
    assert!(program.debug.functions.iter().any(|f| f.name == "add"));
}
#[test]
fn candidate_code_capacity_falls_back_to_identical_baseline() {
    let source="static INPUT:u16=3; #[inline] fn grow(x:u16)->u16{let a=x+1;let b=a^37;let c=b+x;let d=c^19;let e=d+x;let f=e^43;f+x} fn main(){let f:fn(u16)->u16=grow;let a=grow(INPUT);let b=grow(INPUT+1);halt(a+b+f(INPUT));}";
    let mut opts = CompilerOptions::for_separate_code_and_data_segments(0);
    opts.inline.enabled = false;
    let (sized, _) = build(source, &opts);
    opts.code_base = (65536 - sized.words.len()) as u16;
    let (baseline, _) = build(source, &opts);
    opts.inline.enabled = true;
    let (actual, report) = build(source, &opts);
    assert_eq!(report.fallback_count, 1, "{report}");
    assert_eq!(report.retained_sites, 0);
    assert!(
        matches!(
            report.outcome,
            InlineOutcome::Baseline(InlineRejection::Resource(_))
        ),
        "{report}"
    );
    assert_eq!(actual.words, baseline.words);
    assert_eq!(actual.listing, baseline.listing);
    assert_eq!(actual.debug.render(), baseline.debug.render());
}
#[test]
fn code_growth_soft_budget_selects_baseline_without_masking_errors() {
    let mut source = String::from("static INPUT:u16=3; #[inline] fn grow(x:u16)->u16{let mut y=x;");
    for i in 0..8 {
        source.push_str(&format!("y=(y^{})+x;", 100 + i));
    }
    source.push_str("y}");
    for i in 0..20 {
        source.push_str(&format!("fn wrapper{i}(x:u16)->u16{{grow(x+{i})}}"));
    }
    source.push_str("fn main(){let f:fn(u16)->u16=grow;let mut value=INPUT;");
    for i in 0..20 {
        source.push_str(&format!("value=wrapper{i}(value);"));
    }
    source.push_str("halt(value+f(INPUT));}");
    let mut opts = CompilerOptions::default();
    opts.inline.enabled = false;
    let (baseline, _) = build(&source, &opts);
    opts.inline.enabled = true;
    let (actual, report) = build(&source, &opts);
    assert!(
        matches!(
            report.outcome,
            InlineOutcome::Baseline(InlineRejection::CodeGrowth { .. })
        ),
        "{report}"
    );
    assert_eq!(report.retained_sites, 0);
    assert_eq!(report.fallback_count, 1);
    assert_eq!(actual.words, baseline.words);
    assert_eq!(actual.debug.render(), baseline.debug.render());
}

#[test]
fn automatic_small_wrappers_execute_equally_and_can_be_disabled_without_losing_hints() {
    let source="static INPUT:u16=4; fn choose(x:u16)->u16{if x>7{x*2+1}else{x+3}} fn wrapper(x:u16)->u16{choose(x)+2} #[inline] fn hinted(x:u16)->u16{x+1} #[inline(never)] fn barrier(x:u16)->u16{x} fn main(){halt(barrier(hinted(wrapper(INPUT))));}";
    let options = CompilerOptions::default();
    let (on, report) = build(source, &options);
    assert_eq!(run(&on).0, 10);
    assert_eq!(report.outcome, InlineOutcome::Applied, "{report}");
    assert!(report.ir.automatic_expanded_sites() > 0);
    assert!(!on.debug.functions.iter().any(|f| f.name == "wrapper"));
    assert!(on.debug.functions.iter().any(|f| f.name == "barrier"));
    let mut hints_only = options;
    hints_only.inline.automatic = false;
    let (off, report) = build(source, &hints_only);
    assert_eq!(run(&off).0, 10);
    assert_eq!(report.ir.automatic_expanded_sites(), 0);
    assert!(report.retained_sites > 0);
    assert!(off.debug.functions.iter().any(|f| f.name == "wrapper"));
}

#[test]
fn hard_frame_capacity_is_typed_but_nonconvergence_is_not_recoverable() {
    let program =
        parse_source_with("fn call(x:u16)->u16{x} fn main(){halt(call(3));}", 0x1000).unwrap();
    let main = program.funcs.iter().find(|f| f.name == "main").unwrap();
    let convention = RegisterConvention {
        maximum_frame_words: 0,
        ..CPU_V3_REGISTER_CONVENTION
    };
    let error = try_allocate_with_convention(main, true, convention)
        .err()
        .unwrap();
    assert!(matches!(error, AllocationError::FrameCapacity { .. }));
    assert!(BackendError::Allocation(error).is_resource_limit());
    assert!(
        !BackendError::Allocation(AllocationError::NonConvergence { function: "main" })
            .is_resource_limit()
    );
    assert!(!BackendError::Inline(InlineError("bad phi".into())).is_resource_limit());
}

fn checked(source: &str, label: &str, options: &CompilerOptions) -> rcc::frontend::Program {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/rcc-inline-rustc")
        .join(label);
    let gate = rcc::frontend::rustc_check::RustcCheck::new(directory);
    rcc::frontend::compile_program_named_checked(
        "checked-inline.rs",
        source,
        options,
        &gate,
        &mut |name| Err(name.into()),
    )
    .unwrap()
}
#[test]
fn aggregate_arguments_are_snapshotted_before_later_effects() {
    let source="struct Pair{x:u16,y:u16} static DATA:Pair=Pair{x:7,y:11}; fn overwrite()->u16{unsafe{addr_of(&DATA).write(0,77);}0} #[inline] fn observed(value:Pair,ignored:u16)->u16{value.x+value.y} fn main(){let result=observed(DATA,overwrite());halt(result*100+DATA.x);}";
    let mut opts = CompilerOptions::default();
    let (on, report) =
        try_compile_with_inline_report(checked(source, "snapshot-on", &opts), &opts, "main")
            .unwrap();
    opts.inline.enabled = false;
    let (off, _) =
        try_compile_with_inline_report(checked(source, "snapshot-off", &opts), &opts, "main")
            .unwrap();
    assert_eq!(run(&on).0, 1877);
    assert_eq!(run(&off).0, 1877);
    assert!(
        report
            .ir
            .sites
            .iter()
            .any(|s| s.callee == "observed" && s.decision == InlineDecision::Expanded),
        "{report}"
    );
}
#[test]
fn checked_drop_order_and_return_value_stay_exact() {
    let source="#[rcc_resource] struct Owner{word:u16} static LOG:u16=0; #[inline] fn event(code:u16){unsafe{addr_of(&LOG).write(0,LOG*10+code);}} impl Drop for Owner{fn drop(&mut self){event(self.word);}} fn make(word:u16)->Owner{Owner{word:word}} #[inline] fn choose(owner:&Owner)->u16{owner.word+3} fn early(owner:Owner)->u16{choose(&owner)+LOG} fn main(){let a=make(2);let before=early(a);if before!=5||LOG!=2{halt(0xbad);}let b=make(3);drop(b);halt(LOG*10+before);}";
    let mut opts = CompilerOptions::default();
    let (on, report) =
        try_compile_with_inline_report(checked(source, "drop-on", &opts), &opts, "main").unwrap();
    opts.inline.enabled = false;
    let (off, _) =
        try_compile_with_inline_report(checked(source, "drop-off", &opts), &opts, "main").unwrap();
    assert_eq!(run(&on).0, 235);
    assert_eq!(run(&off).0, 235);
    assert!(report.retained_sites > 0, "{report}");
}
#[test]
fn signal_barriers_keep_dynamic_order_and_count() {
    let source="#[inline] fn emit(x:u16)->u16{signal(1,x);x+1} fn main(){let a=emit(4);signal(2,a);halt(emit(a));}";
    let mut opts = CompilerOptions::default();
    let (on, report) = build(source, &opts);
    opts.inline.enabled = false;
    let (off, _) = build(source, &opts);
    let events = |p: &CpuV3Program| {
        let mut sim = CpuV3Sim::default();
        sim.load_program(0, &p.words).unwrap();
        let mut events = Vec::new();
        for _ in 0..10000 {
            match sim.step().unwrap() {
                crate::StepOutcome::Signaled(event) => {
                    events.push((event.signal_type, event.value))
                }
                crate::StepOutcome::Halted { signal } => return (signal, events),
                _ => {}
            }
        }
        panic!("signal inline bound exceeded")
    };
    assert_eq!(events(&on), (6, vec![(1, 4), (2, 5), (1, 5)]));
    assert_eq!(events(&on), events(&off));
    assert_eq!(report.outcome, InlineOutcome::Applied);
}
#[test]
fn allocation_spills_after_inlining_are_not_source_stack_objects() {
    let report=compare("#[inline] fn plus(x:fix32)->fix32{x+fix32::from_int(1)} #[inline(never)] fn external(x:u16)->u16{x+1} fn main(){let a=plus(fix32::from_int(3));let k=external(1);halt((a.to_int() as u16)+k);}",6);
    assert!(
        report
            .frames
            .iter()
            .any(|f| f.function == "main" && f.candidate_fpu_spill > 0),
        "{report}"
    );
}

#[test]
fn two_result_call_and_multi_return_shape_use_all_definitions() {
    let source = "#[inline] fn pair(x:u16)->u16{if x<3{return x;}x+1} fn main(){halt(pair(2));}";
    let opts = CompilerOptions::default();
    let prepare = || {
        let mut p = parse_source_with_config(source, &opts).unwrap();
        let callee = p.funcs.iter_mut().find(|f| f.name == "pair").unwrap();
        callee.n_rets = 2;
        for block in &mut callee.blocks {
            if let Some(Terminator::Ret { values }) = &mut block.term {
                values.push(values[0]);
            }
        }
        let main = p.funcs.iter_mut().find(|f| f.name == "main").unwrap();
        let second = main.fresh_vreg(RegClass::Gpr);
        let sum = main.fresh_vreg(RegClass::Gpr);
        let block = &mut main.blocks[0];
        let first = block
            .insts
            .iter_mut()
            .find_map(|i| {
                if let Instr::Call { rets, .. } = i {
                    rets.push(second);
                    Some(rets[0])
                } else {
                    None
                }
            })
            .unwrap();
        block.insts.push(Instr::Bin {
            dst: sum,
            op: BinOp::Add,
            lhs: first,
            rhs: IntOperand::Reg(second),
        });
        block.lines.push(None);
        block.term = Some(Terminator::Halt { signal: sum });
        p
    };
    let (a, report) = try_compile_with_inline_report(prepare(), &opts, "main").unwrap();
    let mut disabled = opts.clone();
    disabled.inline.enabled = false;
    let (b, _) = try_compile_with_inline_report(prepare(), &disabled, "main").unwrap();
    assert_eq!(run(&a).0, 4);
    assert_eq!(run(&b).0, 4);
    assert_eq!(report.outcome, InlineOutcome::Applied);
}
#[test]
fn inline_hints_do_not_bypass_ownership_or_drop_attribute_checks() {
    let gate = rcc::frontend::rustc_check::RustcCheck::new(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/rcc-inline-rustc/negative"),
    );
    for (source,expected) in [
            ("#[rcc_resource] struct O{word:u16} impl Drop for O{fn drop(&mut self){}} #[inline] fn take(o:O){} fn main(){let a=O{word:2};take(a);take(a);}","E0382"),
            ("#[rcc_resource] struct O{word:u16} impl Drop for O{#[inline] fn drop(&mut self){}} fn main(){}","unsupported resource Drop attribute"),
        ] {
            let error=rcc::frontend::compile_program_named_checked("negative-inline.rs",source,&CompilerOptions::default(),&gate,&mut |name|Err(name.into())).err().unwrap().to_string();
            assert!(error.contains(expected),"{error}");
        }
}

#[test]
fn source_check_profile_and_no_opt_are_independent_of_hints() {
    let source="#[cfg(gpu_debug)] #[inline] fn selected(x:u16)->u16{x+2} #[cfg(not(gpu_debug))] #[inline] fn selected(x:u16)->u16{x+1} fn main(){halt(selected(3));}";
    for gpu_debug in [false, true] {
        for optimized in [false, true] {
            let options = CompilerOptions {
                gpu_debug,
                opt: if optimized {
                    Opts::default()
                } else {
                    Opts::disabled()
                },
                ..CompilerOptions::default()
            };
            let frontend =
                compile_program_named(
                    "profile-inline.rs",
                    source,
                    &options,
                    &mut |n| Err(n.into()),
                )
                .unwrap();
            let (program, report) =
                try_compile_with_inline_report(frontend, &options, "main").unwrap();
            assert_eq!(run(&program).0, if gpu_debug { 5 } else { 4 });
            assert_eq!(program.gpu_debug, gpu_debug);
            assert_eq!(
                report.outcome,
                if optimized {
                    InlineOutcome::Applied
                } else {
                    InlineOutcome::Disabled
                }
            );
        }
    }
}

#[test]
fn automatic_heap_initialization_survives_wrapper_expansion() {
    let source="#[inline] fn allocate()->Ptr{malloc(4)} fn main(){let p=allocate();unsafe{p.write(0,7);}let value=unsafe{p.read(0)};free(p);halt(value);}";
    let mut options = CompilerOptions::default();
    let parse = |options: &CompilerOptions| {
        compile_program_named("heap-inline.rs", source, options, &mut |n| Err(n.into())).unwrap()
    };
    let original = parse(&options);
    let (on, report) = try_compile_with_inline_report(original, &options, "main").unwrap();
    options.inline.enabled = false;
    let (off, _) = try_compile_with_inline_report(parse(&options), &options, "main").unwrap();
    assert_eq!(run(&on).0, 7);
    assert_eq!(run(&off).0, 7);
    assert_eq!(report.outcome, InlineOutcome::Applied, "{report}");
    assert_eq!(
        on.debug
            .functions
            .iter()
            .filter(|f| f.name == "init_heap")
            .count(),
        1
    );
    assert!(!on.debug.functions.iter().any(|f| f.name == "allocate"));
}

#[test]
fn hints_only_unannotated_paths_retain_exact_code_and_debug() {
    let source = "fn plus(x:u16)->u16{x+1} fn main(){halt(plus(4));}";
    let mut opts = CompilerOptions::default();
    opts.inline.automatic = false;
    let (a, r) = build(source, &opts);
    assert_eq!(r.outcome, InlineOutcome::NoExpansions);
    opts.inline.enabled = false;
    let (b, r) = build(source, &opts);
    assert_eq!(r.outcome, InlineOutcome::Disabled);
    assert_eq!(a.words, b.words);
    assert_eq!(a.listing, b.listing);
    assert_eq!(a.debug.render(), b.debug.render());
    opts.inline.enabled = true;
    opts.inline.automatic = true;
    opts.opt = Opts::disabled();
    let (c, r) = build(&source.replace("fn plus", "#[inline] fn plus"), &opts);
    assert_eq!(r.outcome, InlineOutcome::Disabled);
    assert_eq!(run(&c).0, 5);
}
#[test]
fn bounded_nested_calls_execute_equally_without_mutating_baseline() {
    let source="#[inline] fn leaf(x:u16)->u16{x+1} #[inline] fn wrapper(x:u16)->u16{leaf(x)+2} fn main(){halt(wrapper(4));}";
    let mut opts = CompilerOptions::default();
    opts.inline.maximum_depth = 2;
    let (a, ra) = build(source, &opts);
    let (b, rb) = build(source, &opts);
    assert_eq!(run(&a).0, 7);
    assert_eq!(a.words, b.words);
    assert_eq!(a.debug.render(), b.debug.render());
    assert_eq!(ra.retained_sites, rb.retained_sites);
    assert!(ra
        .ir
        .sites
        .iter()
        .any(|s| s.depth == 2 && s.decision == InlineDecision::Expanded));
}

#[test]
fn caller_spill_rejection_keeps_other_bodies_and_rebuilds_selected_reachability() {
    let options = CompilerOptions::default();
    let shared = "static INPUT:u16=1; fn removed(x:u16)->u16{x+1} fn leaf(x:u16)->u16{x+3} fn external(x:u16)->u16{x}";
    let original = format!("{shared} fn bad(x:u16)->u16{{removed(x)}} fn good(x:u16)->u16{{leaf(x)}} fn main(){{halt(bad(1)+good(1));}}");
    // Model an expanded body with six FPU values live across a call. The
    // selected original bad body instead needs the previously removed callee.
    let expanded = format!(
        "{shared} fn bad(x:u16)->u16{{
        let a=fix32::from_int(INPUT as i16);
        let b=a+fix32::from_int(1);let c=a+fix32::from_int(2);
        let d=a+fix32::from_int(3);let e=a+fix32::from_int(4);let f=a+fix32::from_int(5);
        let marker=external(x);(a+b+c+d+e+f).to_int() as u16+marker-20
    }} fn good(x:u16)->u16{{x+3}} fn main(){{halt(bad(1)+good(1));}}"
    );
    let before = parse_source_with_config(&original, &options).unwrap();
    let originals: HashMap<_, _> = before.funcs.into_iter().map(|f| (f.name, f)).collect();
    let (baseline, costs) =
        compile_ir_with_allocations(&originals, &options, "main", before.debug).unwrap();
    let after = parse_source_with_config(&expanded, &options).unwrap();
    let candidates: HashMap<_, _> = after.funcs.into_iter().map(|f| (f.name, f)).collect();
    let (unselected, _) =
        compile_ir_with_allocations(&candidates, &options, "main", after.debug.clone()).unwrap();
    let mut selection = InlineSelection {
        originals: &originals,
        baseline_costs: &costs,
        frames: Vec::new(),
    };
    let (selected, actual_costs) = compile_ir_selected(
        &candidates,
        &options,
        "main",
        after.debug,
        Some(&mut selection),
    )
    .unwrap();
    assert_eq!(run(&baseline).0, 6);
    assert_eq!(run(&unselected).0, 6);
    assert_eq!(run(&selected).0, 6);
    let rejected = selection
        .frames
        .iter()
        .find(|f| f.function == "bad")
        .unwrap();
    assert!(
        matches!(
            rejected.rejection(),
            Some(InlineRejection::FpuSpillGrowth { .. })
                | Some(InlineRejection::FrameGrowth { .. })
        ),
        "{rejected:?}"
    );
    assert!(selection
        .frames
        .iter()
        .find(|f| f.function == "good")
        .unwrap()
        .rejection()
        .is_none());
    assert_eq!(actual_costs["bad"].frame, costs["bad"].frame);
    let linked = |name| selected.debug.functions.iter().any(|f| f.name == name);
    assert!(linked("removed"), "restored body must revive its callee");
    assert!(
        !linked("external"),
        "discarded candidate must not retain its callee"
    );
    assert!(
        !linked("leaf"),
        "unrelated expanded body must remain selected"
    );
}
