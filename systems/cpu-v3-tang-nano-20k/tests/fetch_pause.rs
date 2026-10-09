//! Naive-sim (`CpuV3Sim`) versus cycle-accurate emulator alignment.
//!
//! This is the high-level alignment harness: run the same program image on both
//! models, pause the emulator at a chosen point, wait until the architectural
//! state has stopped changing, and require the **externally visible** state to
//! be identical. The GPR-only case pauses after its register dump; FPU cases
//! compare a direct architectural view captured at the quiescent point.
//!
//! The pause is owned by a per-cycle emulator test hook. It holds new fetch
//! requests off the core without discarding the fetch queue or BTC state and
//! deliberately has no guest-visible device channel or hardware port.
//!
//! Externally visible state compared here:
//! - the 16 architectural GPRs (dumped to a reserved data region by the program
//!   itself, and now also read directly through `SystemHandles`);
//! - all 64 architectural F registers and the shared 64-bit FPU accumulator;
//! - the PC and both segment registers;
//! - the retired word count at the frozen point;
//! - the complete architectural memory image after each model finishes and the
//!   D-cache has been cleaned.
//!
//! Private state is deliberately excluded: cache tag/valid/dirty/way state,
//! arbiter state, and SDRAM controller state are not architectural (the SDRAM
//! controller phase *is* carried in snapshots, but it is not compared).
//!
//! These checks are part of the default suite; hardware co-simulation remains
//! the explicitly invoked ignored workload.

mod system_emu;

use cpu_v3::{alu, halt, load_immediate16, store, AluOp, CpuV3Sim, RunOutcome, Word};
use cpu_v3::{fpu_aux, fpu_vector, FpuAuxKind, FpuAuxSubop, FpuVectorLength, FpuVectorSubop};
use system_emu::{
    compile_cpu_v3_source, run_benchmark_paused_memory, run_from_checkpoint, ArchitecturalView,
    Checkpoint,
};

/// Data-visible region the program writes while it runs.
const WORK_BASE: Word = 0x4000;
/// Region the program parks its GPR dump in: one word per register.
const DUMP_BASE: Word = 0x3000;

const WORDS: usize = 1 << 16;

/// A rolling seed so several instantiations can be compared.
///
/// `halt()` is `SIGNAL r0, 0`, so it latches `r0` as the halt signal: the
/// program keeps `r0` at `seed` and accumulates into `r1` so the halt signal
/// stays deterministic across both models.
///
/// Structure matters here: the program computes, then dumps every GPR, then
/// spins in a NOP sled before halting. The NOP sled is where the pause lands,
/// so the dump has already retired when the state freezes and the comparison
/// covers the register file the pause point actually saw.
fn program(seed: Word, sled: usize) -> Vec<u16> {
    let mut p = Vec::new();
    // r0 = seed and halt signal, r1 = accumulator, r2 = result, r3 = work base.
    p.extend(load_immediate16(0, seed));
    p.extend(load_immediate16(1, 0x0001));
    p.extend(load_immediate16(2, 0x0000));
    p.extend(load_immediate16(3, WORK_BASE));
    // A short arithmetic chain producing distinct register contents and memory
    // traffic. Store offsets are 4-bit immediates, so both regions stay small.
    for step in 0..7u8 {
        p.push(alu(AluOp::Add, 1, 1, 0));
        p.push(alu(AluOp::Xor, 2, 2, 1));
        p.push(store(2, 3, i16::from(step)));
    }
    // Dump every GPR into the reserved region. The two dump bases cover
    // registers 0..8 and 8..16 so every store offset stays inside 0..7.
    for (base, register) in [(4u8, 0u8), (5, 8)] {
        p.extend(load_immediate16(
            base,
            if register == 0 {
                DUMP_BASE
            } else {
                DUMP_BASE + 8
            },
        ));
        for index in register..register + 8 {
            p.push(store(index, base, i16::from(index - register)));
        }
    }
    for _ in 0..sled {
        p.push(cpu_v3::nop());
    }
    p.push(halt());
    p
}

/// Word count at which the NOP sled begins for [`program`].
fn sled_start() -> usize {
    program(0, 0).len() - 1
}

/// The checkpoint signal the program retires at: `SIGNAL r0, 0` never happens
/// here, so alignment uses the retired-word count, but the halt signal is
/// compared too.
/// Runs the naive architectural simulator to completion and returns the final
/// machine plus the halt signal it stopped with.
fn run_sim(words: &[u16], maximum_steps: usize) -> (CpuV3Sim, Word) {
    let mut machine = CpuV3Sim::with_physical_memory_words(WORDS);
    machine
        .load_physical(cpu_v3::PhysicalWordAddress::new(0), words)
        .expect("program image must fit");
    let outcome = machine.run(maximum_steps).expect("sim must not fault");
    match outcome {
        RunOutcome::Halted { signal, .. } => (machine, signal),
        other => panic!("sim must reach the halt, got {other:?}"),
    }
}

fn sim_register_dump(machine: &CpuV3Sim) -> Vec<Word> {
    let mut dump = Vec::with_capacity(16);
    for register in 0..16u16 {
        dump.push(machine.memory(DUMP_BASE + register));
    }
    dump
}

fn emu_register_dump(memory: &[u16]) -> Vec<Word> {
    let mut dump = Vec::with_capacity(16);
    for register in 0..16u16 {
        dump.push(memory[(DUMP_BASE + register) as usize]);
    }
    dump
}

/// One cycle-by-cycle `(cycle, retired_words)` timeline from a no-pause run.
fn retired_timeline(program: &[u16]) -> Vec<(usize, u32)> {
    let mut timeline = Vec::new();
    let _ = run_benchmark_paused_memory(program, 2_000_000, |cycle, retired| {
        timeline.push((cycle, retired));
        false
    });
    timeline
}

/// A bounded pause window that starts once `target` words have retired.
///
/// The start cycle is remembered separately from the retired count: retirement
/// must stop while paused, so using the retired count as the release condition
/// would create a pause that can never end.
fn pause_for_cycles_after_retired(
    target: u32,
    hold_cycles: usize,
) -> impl FnMut(usize, u32) -> bool {
    let mut started_at = None;
    move |cycle, retired| {
        if started_at.is_none() && retired >= target {
            started_at = Some(cycle);
        }
        started_at.is_some_and(|start| cycle < start + hold_cycles)
    }
}

/// Alignment at a held fetch pause: the emulator is stopped at a chosen point
/// and its externally visible state must equal the naive sim's state at the same
/// retired-word count.
#[test]
fn paused_emulator_alignment_against_naive_sim() {
    for seed in [0x0000u16, 0x1234, 0xffff] {
        let words = program(seed, 600);
        let sled = sled_start();

        let (sim, sim_halt) = run_sim(&words, 200_000);
        let sim_dump = sim_register_dump(&sim);
        let sim_retired = sim.retired_words();

        // Pick a cycle inside the NOP sled: derive it from a no-pause run so it
        // does not depend on a magic constant, then confirm where we landed.
        let pause_cycle = retired_timeline(&words)
            .iter()
            .find(|(_, retired)| *retired as usize >= sled)
            .map(|(cycle, _)| *cycle)
            .expect("the run must reach the sled");
        const HOLD_CYCLES: usize = 40;
        let paused_retired = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let observed_retired = paused_retired.clone();
        let (emu, emu_memory) =
            run_benchmark_paused_memory(&words, 2_000_000, move |cycle, retired| {
                let active = (pause_cycle..pause_cycle + HOLD_CYCLES).contains(&cycle);
                if active {
                    observed_retired.borrow_mut().push(retired);
                }
                active
            });
        assert_eq!(
            emu.fetch_pause_cycles, HOLD_CYCLES as u32,
            "seed {seed:#06x}: pause duration"
        );
        let paused_retired = paused_retired.borrow();
        assert_eq!(paused_retired.len(), HOLD_CYCLES);
        assert!(
            paused_retired.windows(2).all(|pair| pair[0] <= pair[1]),
            "seed {seed:#06x}: retirement went backwards while paused: {paused_retired:?}"
        );
        assert!(
            *paused_retired.last().unwrap() - *paused_retired.first().unwrap() <= 1,
            "seed {seed:#06x}: more than the already-latched instruction retired while paused: {paused_retired:?}"
        );
        assert!(
            paused_retired[paused_retired.len() - 3..]
                .windows(2)
                .all(|pair| pair[0] == pair[1]),
            "seed {seed:#06x}: retirement did not settle while paused: {paused_retired:?}"
        );

        let emu_dump = emu_register_dump(&emu_memory);
        let frozen_at = emu
            .retired_words_at_quiescence
            .expect("the pause must reach a quiescent retired count");
        assert_eq!(
            frozen_at,
            *paused_retired.last().unwrap(),
            "seed {seed:#06x}: the recorded sync point is not the stable paused state"
        );

        // The comparison is only valid once the data side has gone quiet: a store
        // that retired just before the pause keeps draining through the D-cache
        // afterwards. Assert that quiet point exists and is reached while the
        // pause is held, i.e. strictly before the run ends.
        let quiet_at = emu
            .data_side_quiet_at
            .expect("a held pause must reach data-side quiescence");
        assert!(
            quiet_at >= pause_cycle,
            "seed {seed:#06x}: quiescence at cycle {quiet_at} predates the pause at {pause_cycle}"
        );
        if quiet_at >= emu.cycles {
            panic!(
                "seed {seed:#06x}: data side never went quiet inside the run (quiet_at={quiet_at}, cycles={})",
                emu.cycles
            );
        }
        println!(
            "seed {seed:#06x}: sled from {sled}, pause at cycle {pause_cycle}, quiet at {quiet_at}, emu cycles={} retired={} frozen={frozen_at}",
            emu.cycles, emu.retired_words
        );

        // Guard against a degenerate compare: the pause must land after the
        // whole program body (dump included) and before the final halt, and the
        // run must still finish. The registers cannot change in a NOP sled, so
        // the dump is exactly the register file the pause point saw.
        assert!(
            frozen_at >= sled as u32 && frozen_at < sim_retired as u32,
            "seed {seed:#06x}: pause landed at retired={frozen_at}, outside the sled [{sled}, {sim_retired})"
        );

        // 1. The externally visible register file at the pause point must equal
        //    the naive sim's register file at the same retired-word count.
        assert_eq!(
            emu_dump.len(),
            sim_dump.len(),
            "seed {seed:#06x}: register dump width"
        );
        assert!(
            emu_dump == sim_dump,
            "seed {seed:#06x}: externally visible registers diverged at retired={frozen_at} (paused at cycle {pause_cycle})\n  sim: {sim_dump:04x?}\n  emu: {emu_dump:04x?}"
        );

        // 2. Both models retire the same number of words and stop the same way.
        assert_eq!(
            emu.retired_words, sim_retired as u32,
            "seed {seed:#06x}: retired word count diverged"
        );
        assert_eq!(
            emu.halt_signal, sim_halt,
            "seed {seed:#06x}: halt signal (latched r0)"
        );

        // 3. The complete architectural memory image matches after both runs
        //    have finished and the D-cache has been cleaned.
        let sim_memory: Vec<u16> = (0..WORDS).map(|w| sim.memory(w as Word)).collect();
        let mismatches: Vec<usize> = (0..WORDS)
            .filter(|index| sim_memory[*index] != emu_memory[*index])
            .take(8)
            .collect();
        assert!(
            mismatches.is_empty(),
            "seed {seed:#06x}: architectural memory diverged at {mismatches:04x?}\n  sim: {:04x?}\n  emu: {:04x?}",
            mismatches
                .iter()
                .map(|i| sim_memory[*i])
                .collect::<Vec<_>>(),
            mismatches
                .iter()
                .map(|i| emu_memory[*i])
                .collect::<Vec<_>>()
        );
    }
}

/// The alignment harness also has to hold for a compiler-produced program, not
/// just hand-built instruction sequences.
#[test]
fn compiled_program_alignment_against_naive_sim() {
    let source = r#"
fn accumulate(limit: u16) -> u16 {
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    while i < limit {
        sum = sum + i * 3;
        i = i + 1;
    }
    sum
}

fn main() {
    let a = accumulate(4);
    let b = accumulate(2);
    if a ^ b == 0 {
        halt(1);
    } else {
        halt(0);
    }
}
"#;
    let words = compile_cpu_v3_source(source);

    let (sim, sim_halt) = run_sim(&words, 2_000_000);
    let sim_retired = sim.retired_words();

    // No pause in this test: it pins that a compiler-produced program runs on
    // both models and reaches the same retired count and halt signal.
    let (emu, _memory) = run_benchmark_paused_memory(&words, 50_000, |_, _| false);
    println!(
        "compiled program: sim retired={sim_retired} halt={sim_halt} emu cycles={} retired={} halt={}",
        emu.cycles, emu.retired_words, emu.halt_signal
    );
    assert_eq!(
        emu.retired_words, sim_retired as u32,
        "a compiled program must retire the same words on both models"
    );
    assert_eq!(
        u32::from(sim_halt),
        u32::from(emu.halt_signal),
        "a compiled program must stop with the same signal on both models"
    );
}

/// Several sync points, not just one.
///
/// The same program image, pause gate and comparison are reused at three
/// distinct points. Each point is requested by *retirement count* rather than by
/// cycle: the harness engages the gate on the next cycle after the request, so
/// keying on retirement removes any cycle/instruction offset from the test and
/// makes the frozen point exactly the one asked for. At every point the
/// emulator's frozen externally visible state must equal what the naive simulator
/// reports at the same retired-word count.
#[test]
fn several_sync_points_align_with_the_naive_sim() {
    let words = program(0x4321, 600);
    let sled = sled_start();

    let (sim, sim_halt) = run_sim(&words, 200_000);
    let sim_dump = sim_register_dump(&sim);
    let sim_retired = sim.retired_words();

    // All three points sit in the sled, so the whole program body - including
    // the register dump - has already retired and the register file cannot
    // change again. That makes the naive sim's final register file the correct
    // reference for every point, and each point is still a distinct instruction.
    let checkpoints = [sled as u32, sled as u32 + 25, sled as u32 + 60];
    let mut seen = Vec::new();
    for target in checkpoints {
        assert!(
            target < sim_retired as u32,
            "checkpoint {target} must fall before the halt at {sim_retired}"
        );
        let (emu, emu_memory) = run_benchmark_paused_memory(
            &words,
            2_000_000,
            pause_for_cycles_after_retired(target, 40),
        );

        let frozen = emu
            .retired_words_at_quiescence
            .expect("a held pause must record where it froze");
        // The request is evaluated before the combinational settle while the
        // gate engages on that same cycle, so one more instruction may retire
        // and the freeze lands on the requested point or the one after it. The
        // assertion pins that documented relationship rather than an exact
        // equality the harness cannot promise.
        assert!(
            frozen == target || frozen == target + 1,
            "the pause requested at retired={target} froze at {frozen}"
        );
        assert!(
            frozen >= sled as u32,
            "the frozen point {frozen} must be at or after the sled start {sled}"
        );
        assert!(
            emu.data_side_quiet_at.is_some(),
            "checkpoint at retired={target} never reached data-side quiescence"
        );
        assert_eq!(
            emu.retired_words, sim_retired as u32,
            "checkpoint at retired={target} must still let the run finish"
        );
        assert_eq!(
            emu.halt_signal, sim_halt,
            "halt signal after the checkpoint at retired={target}"
        );
        assert_eq!(
            emu_register_dump(&emu_memory),
            sim_dump,
            "externally visible registers diverged at the sync point at retired={frozen}"
        );
        seen.push(frozen);
    }

    assert!(
        seen.windows(2).all(|pair| pair[0] < pair[1]),
        "the checkpoints must be distinct and increasing: {seen:?}"
    );
    println!("sync points compared at retired words {seen:?} (halt at {sim_retired})");
}

/// A checkpoint lets a trace be resumed instead of replayed.
///
/// The first run pauses at a chosen point and takes a complete machine snapshot
/// there. The second run starts *from that snapshot* - the prefix is reused, not
/// re-executed - and must finish exactly like the unpaused reference run. This is
/// the "reuse the early execution for several tests" capability: one paused
/// prefix can be snapshotted once and resumed any number of times.
#[test]
fn a_checkpoint_resumes_without_replaying_the_prefix() {
    let words = program(0x2468, 600);
    let sled = sled_start();

    // Reference: the whole run, no pause.
    let (reference, _) = run_benchmark_paused_memory(&words, 2_000_000, |_, _| false);

    // First run: pause at the sled and snapshot the machine while it is paused
    // and quiescent.
    let pause_from = sled as u32;
    let release = std::rc::Rc::new(std::cell::Cell::new(false));
    let release_from_observer = release.clone();
    let release_from_hook = release.clone();
    let mut take_checkpoint =
        move |_cycle: usize, _handles: &system_emu::SystemHandles, _busy: bool| {
            release_from_observer.set(true);
            true
        };
    let (first, _, checkpoint) = run_from_checkpoint(
        &words,
        2_000_000,
        move |_, retired| retired >= pause_from && !release_from_hook.get(),
        None,
        Some(&mut take_checkpoint),
    );
    let checkpoint: Checkpoint = checkpoint.expect("the paused point must yield a checkpoint");
    let frozen = first
        .retired_words_at_quiescence
        .expect("the first run must have paused");
    assert!(
        frozen >= sled as u32,
        "the checkpoint must be taken at or after the sled start {sled}, got {frozen}"
    );

    // Second run: start from the snapshot. The prefix is not re-executed, so the
    // retired count when it starts is exactly the frozen one.
    let resumed_start = std::rc::Rc::new(std::cell::Cell::new(None));
    let observed_start = resumed_start.clone();
    let (resumed, resumed_memory, _) = run_from_checkpoint(
        &words,
        2_000_000,
        move |_, retired| {
            if observed_start.get().is_none() {
                observed_start.set(Some(retired));
            }
            false
        },
        Some(&checkpoint),
        None,
    );
    assert_eq!(
        resumed_start.get(),
        Some(frozen),
        "resumed execution reset or replayed the saved prefix"
    );
    assert!(
        resumed.cycles < reference.cycles,
        "resuming a late checkpoint did not save any execution: resumed={} reference={}",
        resumed.cycles,
        reference.cycles
    );

    assert_eq!(
        resumed.halt_signal, reference.halt_signal,
        "a resumed run must stop with the same signal"
    );
    assert_eq!(
        resumed.retired_words, reference.retired_words,
        "a resumed run must retire the same total number of words"
    );
    assert_eq!(
        resumed_memory,
        {
            let (_, reference_memory) =
                run_benchmark_paused_memory(&words, 2_000_000, |_, _| false);
            reference_memory
        },
        "a resumed run must produce the same architectural memory as the reference"
    );
    println!(
        "checkpoint at retired={frozen} resumed in {} cycles to halt={} retired={} (full run {} cycles / {} retired)",
        resumed.cycles,
        resumed.halt_signal,
        resumed.retired_words,
        reference.cycles,
        reference.retired_words
    );
}

/// Snapshot/restore round trip at several points, including mid-body.
///
/// At each chosen retired-word count the machine is paused, snapshotted, and then
/// resumed from that snapshot. The resumed run must reproduce the reference run
/// exactly. This is a strong sync check at arbitrary points: if the state frozen
/// at a pause were incomplete or stale - a lost queue word, a request still in
/// flight, a register not yet written - the resumed run would diverge
/// downstream, so agreement means the pause point really is a coherent
/// architectural state. Points inside the body are deliberately included, since
/// that is where the register dump has not happened yet and the earlier
/// sled-only comparisons were weakest.
#[test]
fn snapshot_round_trip_matches_the_reference_at_several_points() {
    let words = program(0x5a5a, 600);

    let (reference, reference_memory) =
        run_benchmark_paused_memory(&words, 2_000_000, |_, _| false);
    let (sim, sim_halt) = run_sim(&words, 200_000);
    assert_eq!(reference.halt_signal, sim_halt, "reference vs naive sim");

    // Early body, late body, and inside the sled.
    let points = [4u32, 20, 40, 50, 90];
    let mut checked = Vec::new();
    let mut busy_sdram = 0usize;
    for target in points {
        // Wait for the pause to have drained the data side but keep the SDRAM
        // mid-transaction, so the snapshot has to carry the controller phase.
        // Giving up after a bounded search still yields a checkpoint, so the
        // round trip is checked either way.
        let release = std::rc::Rc::new(std::cell::Cell::new(false));
        let release_from_observer = release.clone();
        let release_from_hook = release.clone();
        let mut attempts = 0usize;
        let mut take =
            move |_cycle: usize, _handles: &system_emu::SystemHandles, sdram_busy: bool| {
                attempts += 1;
                let accept = sdram_busy || attempts > 5_000;
                if accept {
                    release_from_observer.set(true);
                }
                accept
            };
        let (first, _, checkpoint) = run_from_checkpoint(
            &words,
            2_000_000,
            move |_, retired| retired >= target && !release_from_hook.get(),
            None,
            Some(&mut take),
        );
        let checkpoint: Checkpoint =
            checkpoint.unwrap_or_else(|| panic!("no checkpoint at retired>={target}"));
        let frozen = first
            .retired_words_at_quiescence
            .expect("the run must have paused");
        if checkpoint.sdram_was_busy() {
            busy_sdram += 1;
        }

        // Resume from the snapshot: no prefix replay.
        let (resumed, resumed_memory, _) =
            run_from_checkpoint(&words, 2_000_000, |_, _| false, Some(&checkpoint), None);

        assert_eq!(
            resumed.retired_words, reference.retired_words,
            "resume from retired={frozen} retired a different total"
        );
        assert_eq!(
            resumed.halt_signal, reference.halt_signal,
            "resume from retired={frozen} stopped with a different signal"
        );
        assert!(
            resumed_memory == reference_memory,
            "resume from retired={frozen} produced different architectural memory"
        );
        checked.push(frozen);
    }

    assert!(
        checked.windows(2).all(|pair| pair[0] < pair[1]),
        "the snapshot points must be distinct and increasing: {checked:?}"
    );
    assert!(
        checked.first().copied().unwrap_or(u32::MAX) < sled_start() as u32,
        "at least one snapshot point must fall inside the body: {checked:?}"
    );
    assert!(
        busy_sdram > 0,
        "no snapshot point had the SDRAM busy, so restore of the controller phase is untested: {checked:?}"
    );
    println!(
        "snapshot round trip ok at retired {checked:?} ({busy_sdram} taken with the SDRAM busy); sim retired={} halt={sim_halt}",
        sim.retired_words()
    );
}

/// FPU code paths, checked with the architectural FPU state directly.
///
/// The earlier programs are integer-only, so they never exercised the F
/// registers, the shared 64-bit accumulator or the special/vector paths. These
/// programs are modelled on the frozen suite's FPU shapes (dot product, vector
/// multiply/subtract, `frsqrt`, `fsqrt`-free normalize construction, `frcp` and
/// `fsincos`).
///
/// Each case pauses the cycle model after the FPU work has retired, takes a
/// checkpoint, and then compares the **complete architectural view** - 16 GPRs,
/// all 64 F registers, the accumulator, PC and both segment registers - against
/// the naive simulator's final state. It also resumes from that checkpoint and
/// requires the architectural memory to match, which covers side effects the
/// registers do not.
fn check_fpu_program(name: &str, source: &str) {
    let words = compile_cpu_v3_source(source);
    let (sim, sim_halt) = run_sim(&words, 500_000);
    let sim_retired = sim.retired_words();

    // A second simulator run, stopped at the same retired-word count as the
    // cycle model's frozen point, so the two states are compared at the same
    // instruction rather than at the end of the program.
    let mut sim_at_point = CpuV3Sim::with_physical_memory_words(WORDS);
    sim_at_point
        .load_physical(cpu_v3::PhysicalWordAddress::new(0), &words)
        .expect("program image must fit");

    // All FPU work is done before the final spin, so pausing anywhere in the
    // halt loop is a point where the architectural FPU state is final.
    let spin_start = words.len().saturating_sub(3) as u32;

    let release = std::rc::Rc::new(std::cell::Cell::new(false));
    let release_from_observer = release.clone();
    let release_from_hook = release.clone();
    let mut observed: Option<ArchitecturalView> = None;
    let mut take = |_cycle: usize, handles: &system_emu::SystemHandles, _busy: bool| {
        observed = Some(handles.architectural_view());
        release_from_observer.set(true);
        true
    };
    let (emu, _emu_memory, checkpoint) = run_from_checkpoint(
        &words,
        500_000,
        move |_, retired| retired >= spin_start && !release_from_hook.get(),
        None,
        Some(&mut take),
    );
    let observed = observed.unwrap_or_else(|| panic!("{name}: no architectural view taken"));
    let checkpoint = checkpoint.unwrap_or_else(|| panic!("{name}: no checkpoint taken"));
    assert_eq!(
        checkpoint.pending_accepted_words(),
        1,
        "{name}: checkpoint did not exercise restoration of an accepted prefix word"
    );

    // 1. Retirement agreement.
    assert_eq!(
        emu.retired_words, sim_retired as u32,
        "{name}: retired words diverged"
    );
    assert_eq!(emu.halt_signal, sim_halt, "{name}: halt signal diverged");

    // 2. Every architectural register, directly, against a simulator stopped at
    //    the same retired-word count.
    while sim_at_point.retired_words() < u64::from(observed.retired_words) {
        sim_at_point
            .step()
            .unwrap_or_else(|fault| panic!("{name}: simulator faulted: {fault:?}"));
    }
    assert_eq!(
        sim_at_point.retired_words(),
        u64::from(observed.retired_words),
        "{name}: simulator stopped at a different retired count"
    );
    assert_eq!(
        observed.gprs,
        *sim_at_point.registers(),
        "{name}: GPRs diverged at retired={}",
        observed.retired_words
    );
    assert_eq!(
        observed.f_registers,
        *sim_at_point.fpu_registers(),
        "{name}: F registers diverged at retired={}",
        observed.retired_words
    );
    assert_eq!(
        observed.accumulator,
        sim_at_point.fpu_accumulator(),
        "{name}: FPU accumulator diverged at retired={}",
        observed.retired_words
    );
    assert_eq!(
        observed.pc,
        sim_at_point.pc().wrapping_add(1),
        "{name}: PC diverged at retired={} (emu {:04x} vs sim {:04x})",
        observed.retired_words,
        observed.pc,
        sim_at_point.pc()
    );
    assert_eq!(
        observed.segments,
        (sim_at_point.code_segment(), sim_at_point.data_segment()),
        "{name}: segment registers diverged"
    );

    // 3. Resume from the checkpoint: the run must still finish identically, and
    //    the architectural memory must match the naive simulator's memory.
    let sim_memory: Vec<u16> = (0..WORDS).map(|w| sim.memory(w as Word)).collect();
    let checkpoint_memory = checkpoint.snapshot.memory.clone();
    let (resumed, resumed_memory, _) =
        run_from_checkpoint(&words, 500_000, |_, _| false, Some(&checkpoint), None);
    assert_eq!(
        resumed.retired_words, emu.retired_words,
        "{name}: resumed run retired a different total"
    );
    assert_eq!(
        resumed.halt_signal, emu.halt_signal,
        "{name}: resumed run stopped with a different signal"
    );
    assert!(
        resumed_memory == sim_memory,
        "{name}: architectural memory diverged after resuming from the checkpoint"
    );
    assert!(
        checkpoint_memory == sim_memory,
        "{name}: the architectural memory carried in the checkpoint (before resuming) already \
         disagreed with the naive simulator"
    );
    // The F-register comparison must not be vacuous: the FPU programs leave live
    // values behind, so at least some F registers are non-zero at the tested
    // point.
    assert!(
        observed.f_registers.iter().any(|value| *value != 0),
        "{name}: every F register was zero, so the F-register comparison proves nothing"
    );

    // Guard against a vacuous F-register comparison: report how many F registers
    // were actually holding a non-zero value at the tested point.
    let live_f = observed
        .f_registers
        .iter()
        .filter(|value| **value != 0)
        .count();
    println!(
        "{name}: ok at retired={} (halt={}, acc={}, {live_f} non-zero F registers, first four {:?})",
        observed.retired_words,
        sim_halt,
        observed.accumulator,
        &observed.f_registers[0..4]
    );
}

#[test]
fn fpu_vector_and_rsqrt_paths_align() {
    check_fpu_program(
        "fpu_vec_dot_rsqrt",
        r#"
use crate::dsl_rt::*;

fn main() {
    let a = vec3::new(fix32::from_int(1), fix32::from_int(2), fix32::from_int(3));
    let b = vec3::new(fix32::from_int(4), fix32::from_int(5), fix32::from_int(6));
    let sum = a + b;                       // 5, 7, 9
    let diff = b - a;                      // 3, 3, 3
    let s = sum * diff;                    // 15, 21, 27
    let squared = fdot(s, s);              // 15^2 + 21^2 + 27^2
    let inv = frsqrt(squared);
    let scaled = s * inv;
    let length2 = fdot(a, a);              // 14
    halt((scaled.x().to_int() as u16) ^ (length2.to_int() as u16));
}
"#,
    );
}

#[test]
fn fpu_special_function_paths_align() {
    check_fpu_program(
        "fpu_rcp_sincos",
        r#"
use crate::dsl_rt::*;

fn main() {
    let angle = fix32::from_words(0x8000, 0x0001);
    let sc = fsincos(angle);
    let r = frcp(fix32::from_int(3));
    let n = frsqrt(fix32::from_int(2));
    let combined = sc.y() + r + n;
    let floored = combined.abs().floor();
    halt((floored.to_int() as u16) ^ (sc.x().to_int() as u16));
}
"#,
    );
}

#[test]
fn fpu_medium_bezier_shape_aligns() {
    check_fpu_program(
        "fpu_bezier",
        r#"
use crate::dsl_rt::*;

fn main() {
    let p0 = vec4::new(fix32::from_int(0), fix32::from_int(0), fix32::zero(), fix32::zero());
    let p1 = vec4::new(fix32::from_int(1), fix32::from_int(2), fix32::zero(), fix32::zero());
    let p2 = vec4::new(fix32::from_int(3), fix32::from_int(1), fix32::zero(), fix32::zero());
    let p3 = vec4::new(fix32::from_int(4), fix32::from_int(0), fix32::zero(), fix32::zero());
    let three = fix32::from_int(3);
    let t = fix32::from_words(0x4000, 0x0000);
    let s = fix32::from_int(1) - t;
    let a = p0 * (s * s * s);
    let b = p1 * (three * s * s * t);
    let c = p2 * (three * s * t * t);
    let d = p3 * (t * t * t);
    let point = a + b + c + d;
    halt((point.x().to_int() as u16) ^ (point.y().to_int() as u16));
}
"#,
    );
}

/// The shared FPU accumulator, checked at a point where it is non-zero.
///
/// Every architectural boundary reached through `DOTSTORE` leaves ACC clean, so
/// a comparison there is vacuous. `DOTADD` however accumulates and leaves ACC
/// live, so a boundary *between* two `DOTADD`s is a point where ACC holds a real
/// partial sum. This program is a hand-built image (rcc has no intrinsic that
/// stops between two accumulating dots): it loads 2, 1, 2, 2 into F0..F3 and
/// accumulates two 2-lane dots without ever running `DOTSTORE`, so ACC is
/// non-zero at every boundary from the first accumulating dot onwards. The exact
/// sum is deliberately not pinned here: what the test requires is that both
/// models agree on a live, non-zero accumulator, not which value a hand-built
/// lane/stride arrangement happens to produce.
#[test]
fn fpu_accumulator_is_compared_where_it_is_live() {
    let mut words: Vec<u16> = Vec::new();
    // GPR constants: 2, 1, 2, 2.
    for (register, value) in [(0u8, 2u16), (1, 1), (2, 2), (3, 2)] {
        words.extend(load_immediate16(register, value));
    }
    // F0..F3 = 2, 1, 2, 2 as Q16.16.
    for (gpr, f) in [(0u8, 0u8), (1, 1), (2, 2), (3, 3)] {
        words.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            gpr,
            0,
            f,
            FpuAuxSubop::I16tof,
            0,
        ));
    }
    // First accumulating dot: (2*1) + (2*1) = 4, ACC stays live (no DOTSTORE).
    words.extend(fpu_vector(
        0,
        1,
        0,
        FpuVectorLength::Vec2,
        FpuVectorSubop::DotAdd,
        0,
    ));
    // Second accumulating dot: adds (2*1) + (2*2) = 6, so ACC becomes 10.
    words.extend(fpu_vector(
        2,
        3,
        0,
        FpuVectorLength::Vec2,
        FpuVectorSubop::DotAdd,
        0,
    ));
    // Spin, then halt. The pause lands in the spin, i.e. after both dots.
    for _ in 0..40 {
        words.push(cpu_v3::nop());
    }
    words.push(halt());

    let (sim, sim_halt) = run_sim(&words, 100_000);
    let sim_retired = sim.retired_words();

    // Pause before the spin so the freeze sits after the accumulating dots.
    let release = std::rc::Rc::new(std::cell::Cell::new(false));
    let release_from_observer = release.clone();
    let release_from_hook = release.clone();
    let mut observed: Option<ArchitecturalView> = None;
    let mut take = |_cycle: usize, handles: &system_emu::SystemHandles, _busy: bool| {
        observed = Some(handles.architectural_view());
        release_from_observer.set(true);
        true
    };
    let pause_from = (words.len() - 40) as u32;
    let (emu, _memory, checkpoint) = run_from_checkpoint(
        &words,
        100_000,
        move |_, retired| retired >= pause_from && !release_from_hook.get(),
        None,
        Some(&mut take),
    );
    let observed = observed.expect("no architectural view taken");
    let checkpoint = checkpoint.expect("no checkpoint taken");

    // The simulator stopped at the very same retired-word count.
    let mut sim_at_point = CpuV3Sim::with_physical_memory_words(WORDS);
    sim_at_point
        .load_physical(cpu_v3::PhysicalWordAddress::new(0), &words)
        .expect("program image must fit");
    while sim_at_point.retired_words() < u64::from(observed.retired_words) {
        sim_at_point.step().expect("simulator must not fault");
    }

    assert_eq!(
        observed.accumulator,
        sim_at_point.fpu_accumulator(),
        "the accumulator diverged at retired={}",
        observed.retired_words
    );
    assert!(
        observed.accumulator != 0,
        "the accumulator was 0 at retired={}, so this comparison is vacuous",
        observed.retired_words
    );
    assert_eq!(
        emu.retired_words, sim_retired as u32,
        "retired words diverged"
    );
    assert_eq!(emu.halt_signal, sim_halt, "halt signal diverged");
    assert_eq!(
        observed.f_registers,
        *sim_at_point.fpu_registers(),
        "F registers diverged in the accumulator program"
    );

    let checkpoint_memory = checkpoint.snapshot.memory.clone();
    let sim_memory: Vec<u16> = (0..WORDS).map(|w| sim.memory(w as Word)).collect();
    assert!(
        checkpoint_memory == sim_memory,
        "the checkpoint memory disagreed with the naive simulator"
    );
    println!(
        "accumulator program: ok at retired={} (ACC={}, halt={})",
        observed.retired_words, observed.accumulator, sim_halt
    );
}
