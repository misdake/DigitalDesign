//! Real RCC helper snapshots measured on the core's clocked one-cycle peer.
//! This is not a cache/MC, full-renderer, or board timing measurement.
use cpu_v3::rcc_backend::{
    try_compile_with_inline_report, CompilerOptions, CpuV3Program, InlineOutcome,
};
use cpu_v3::{CpuV3Core, CpuV3CoreInput, CpuV3CoreInputValue, CpuV3Sim};
use digital_design_circuit::{build_circuit, Circuit};
use digital_design_hardware::{Module, ModuleIo};
use rcc::frontend::compile_program_named;
use std::collections::HashMap;

fn drive(
    circuit: &mut Circuit,
    input: &CpuV3CoreInput,
    instruction: Option<u16>,
    data: Option<u16>,
) {
    input.drive(
        circuit,
        &CpuV3CoreInputValue {
            reset: false,
            hold: false,
            instruction_request_ready: true,
            instruction_response_valid: instruction.is_some(),
            instruction_data: u64::from(instruction.unwrap_or(0)),
            instruction_error: false,
            data_request_ready: true,
            data_response_valid: data.is_some(),
            data_read_data: u64::from(data.unwrap_or(0)),
            data_error: false,
            data_line_copy_ready: true,
            data_cache_command_error: false,
            device_read_data: 0,
        },
    );
}
fn core_cycles(program: &CpuV3Program, maximum_cycles: usize) -> (u16, usize, u32) {
    assert_eq!(program.code_base, 0);
    let mut memory: HashMap<u32, u16> = program
        .words
        .iter()
        .copied()
        .enumerate()
        .map(|(i, w)| (i as u32, w))
        .collect();
    let (mut circuit, (input, output)) = build_circuit(|| {
        let i = CpuV3CoreInput::allocate();
        let o = CpuV3Core::emu(&i);
        (i, o)
    });
    let mut instruction = None;
    let mut data = None;
    drive(&mut circuit, &input, None, None);
    for cycle in 0..maximum_cycles {
        circuit.execute_gates();
        let value = output.sample(&circuit);
        assert!(
            !value.fault,
            "core fault {} at {}",
            value.fault_code, value.fault_pc
        );
        if value.halted {
            return (value.halt_signal as u16, cycle, value.retired_words as u32);
        }
        let next_instruction = value.instruction_request_valid.then(|| {
            memory
                .get(&(value.instruction_address as u32))
                .copied()
                .unwrap_or(0)
        });
        let next_data = if value.data_request_valid {
            let address = value.data_address as u32;
            if value.data_write {
                memory.insert(address, value.data_write_data as u16);
                Some(0)
            } else {
                Some(memory.get(&address).copied().unwrap_or(0))
            }
        } else {
            None
        };
        drive(&mut circuit, &input, instruction.take(), data.take());
        circuit.clock_tick();
        instruction = next_instruction;
        data = next_data;
    }
    panic!("inline helper core exceeded {maximum_cycles} cycles")
}
fn architectural_steps(program: &CpuV3Program) -> (u16, usize, u16) {
    let mut sim = CpuV3Sim::default();
    sim.load_program(program.code_base, &program.words).unwrap();
    let mut lowest_sp = u16::MAX;
    for steps in 1..=250_000 {
        let outcome = sim.step().unwrap();
        let sp = sim.register(13).unwrap();
        // Ignore reset/partially materialized SP before the entry prologue.
        if sp > 0xe000 {
            lowest_sp = lowest_sp.min(sp);
        }
        if let cpu_v3::StepOutcome::Halted { signal } = outcome {
            return (signal, steps, lowest_sp);
        }
    }
    panic!("inline helper architectural model exceeded 250000 steps")
}

#[test]
fn scene_fix_helpers_reduce_clocked_core_cycles_with_guarded_memory() {
    // Exact annotated production helpers, without duplicating inline attributes.
    let math = include_str!("../../rcc/gpu_scene_math.rs");
    let helpers = &math[math.find("pub fn gpu_scene_fix_read(").unwrap()
        ..math.find("pub fn gpu_scene_zero(").unwrap()];
    let helpers = format!("#[inline]\n{helpers}");
    let source = r#"
        mod scene;
        static DATA:Buf<u16,4>=Buf::new([0,1,0,2]);
        static OUT:Buf<u16,6>=Buf::new([0,0,0xbeef,0xcafe,0x1357,0x2468]);
        fn main(){
            let mut i:u16=0;let mut sum=fix32::zero();
            while i<32 {
                let x=gpu_scene_fix_read(DATA.as_array(),(i&1)*2);
                sum=sum+x;gpu_scene_fix_write(OUT.as_array(),0,sum);i=i+1;
            }
            let guard=(OUT[2u16]^0xbeef)|(OUT[3u16]^0xcafe)|(OUT[4u16]^0x1357)|(OUT[5u16]^0x2468);
            halt(OUT[1u16]+guard);
        }
    "#;
    let mut opts = CompilerOptions {
        stack_init: 0xfff0,
        ..CompilerOptions::default()
    };
    opts.inline.enabled = false;
    let parse = |opts: &CompilerOptions| {
        compile_program_named("inline-helper.rs", source, opts, &mut |name| {
            if name == "scene" {
                Ok(helpers.clone())
            } else {
                Err(name.into())
            }
        })
        .unwrap()
    };
    // Share one frontend snapshot; time the whole backend API, including
    // graph analysis/cloning and both lowerings in the enabled compilation.
    let frontend = parse(&opts);
    let candidate_input = rcc::frontend::Program {
        funcs: frontend.funcs.clone(),
        debug: frontend.debug.clone(),
    };
    let start = std::time::Instant::now();
    let (baseline, _) = try_compile_with_inline_report(frontend, &opts, "main").unwrap();
    let baseline_host_backend = start.elapsed();
    opts.inline.enabled = true;
    let start = std::time::Instant::now();
    let (candidate, report) =
        try_compile_with_inline_report(candidate_input, &opts, "main").unwrap();
    let candidate_host_backend = start.elapsed();
    assert_eq!(report.outcome, InlineOutcome::Applied, "{report}");
    assert_eq!(report.retained_sites, 2, "{report}");
    assert!(!candidate
        .debug
        .functions
        .iter()
        .any(|f| f.name == "gpu_scene_fix_read" || f.name == "gpu_scene_fix_write"));
    let before_steps = architectural_steps(&baseline);
    let after_steps = architectural_steps(&candidate);
    assert_eq!(before_steps.0, 48);
    assert_eq!(after_steps.0, 48);
    let before = core_cycles(&baseline, 100_000);
    let after = core_cycles(&candidate, 100_000);
    assert_eq!(before.0, 48);
    assert_eq!(after.0, 48);
    assert!(after.1 < before.1, "{before:?} -> {after:?}");
    println!("real scene fix helpers, fixed 32 dynamic iterations, one-cycle core peer: baseline cycles={} retired={} words={} steps={} lowest_SP={:#06x}; inline cycles={} retired={} words={} steps={} lowest_SP={:#06x}",before.1,before.2,baseline.words.len(),before_steps.1,before_steps.2,after.1,after.2,candidate.words.len(),after_steps.1,after_steps.2);
    println!("host backend wall time from one frontend snapshot (one noisy observation, not a timing guarantee): inline off {:?}, inline on {:?}; on includes analysis and both lowerings", baseline_host_backend, candidate_host_backend);
    println!("{report}");
}

#[path = "scene_matrix.rs"]
mod scene_matrix;
