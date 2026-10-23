//! Standalone CPU and wall-clock checks, without production device wiring.
use cpu_v3::{rcc_backend, CpuV3Sim, PhysicalWordAddress, RunOutcome};

const PROGRAM: &str = r#"
mod frame_stats;
static STATE: Buf<u16,4> = Buf::new([0;4]);
fn main() {
    let input = unsafe { Ptr::from_addr(0x5000).as_u16_array() };
    frame_stats_reset(STATE.as_array());
    let mut i:u16 = 0;
    while i < input[0u16] {
        frame_stats_begin(STATE.as_array(),input[1+i*2],input[2+i*2] != 0);
        frame_stats_digits(STATE.as_array(),unsafe { Ptr::from_addr(0x6000+i*5).as_u16_array() });
        i += 1;
    }
    halt(0x600d);
}
"#;

fn validate_cpu() -> usize {
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed =
        rcc::frontend::compile_program_named("Frame statistics", PROGRAM, &options, &mut |name| {
            match name {
                "frame_stats" => Ok(include_str!("rcc/frame_stats.rs").to_owned()),
                _ => Err(format!("unexpected module {name}")),
            }
        })
        .expect("RCC frontend");
    let code = rcc_backend::try_compile(parsed, &options, "main")
        .expect("CPU V3 compile")
        .words;
    let mut cases = vec![(65530u16, true), (11, true), (100, false), (109, true)];
    let mut now = 109u16;
    for delta in [0, 1, 9, 10, 99, 100, 999, 1000, 9999, 10000, 59999, 65535] {
        now = now.wrapping_add(delta);
        cases.push((now, true));
    }
    let mut seed = 0x1234u16;
    for _ in 0..128 {
        seed = seed.wrapping_mul(25173).wrapping_add(13849);
        now = now.wrapping_add(seed);
        cases.push((now, true));
    }
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code);
    cpu.physical_memory_mut()[0x5000] = cases.len() as u16;
    for (i, (now, complete)) in cases.iter().enumerate() {
        cpu.physical_memory_mut()[0x5001 + i * 2] = *now;
        cpu.physical_memory_mut()[0x5002 + i * 2] = u16::from(*complete);
    }
    let result = cpu.run(5_000_000).expect("bounded CPU run");
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    for (i, (now, complete)) in cases.iter().enumerate() {
        let expected = if i != 0 && *complete {
            format!("{:05}", now.wrapping_sub(cases[i - 1].0))
        } else {
            "-----".to_owned()
        };
        for (j, glyph) in expected.bytes().enumerate() {
            assert_eq!(
                cpu.physical_memory(PhysicalWordAddress::new((0x6000 + i * 5 + j) as u32)),
                u16::from(glyph),
                "case {i}, glyph {j}"
            );
        }
    }
    cases.len()
}

fn main() {
    println!("PASS frame statistics: {} RCC cases", validate_cpu());
}

#[cfg(test)]
mod tests {
    #[test]
    fn compiled_cpu_formats_completed_intervals_and_rejects_invalid_ones() {
        assert_eq!(super::validate_cpu(), 144);
    }
}
