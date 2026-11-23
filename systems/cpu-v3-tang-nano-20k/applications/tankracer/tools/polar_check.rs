//! Exact kernel fixtures: host arithmetic models the preserved wide MAC/round contract.
use super::*;
fn profile(code: &CpuV3Program, input: &[u16], count: u16) -> (u64, u64, u64) {
    let fast = polar_symbol(code, "polar_fpu");
    let wide = polar_symbol(code, "polar_wide");
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    cpu.physical_memory_mut()[INPUT..INPUT + input.len()].copy_from_slice(input);
    cpu.physical_memory_mut()[COUNT] = count;
    let mut attempts = 0;
    let mut fallbacks = 0;
    for steps in 1..=10_000_000u64 {
        attempts += u64::from(Some(cpu.pc()) == fast);
        fallbacks += u64::from(Some(cpu.pc()) == wide);
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, PASS);
            return (steps, attempts, fallbacks);
        }
    }
    panic!("kernel check instruction budget exceeded")
}
pub(super) fn run(output: &Path) -> Result<Value, Box<dyn Error>> {
    let matrices = [
        [4096, 0, 0, 0, 4096, 0, 0, 0, 4096],
        [4096, 120, 0, 0, 4096, 96, 0, 0, 4096],
        [4096, -120, 0, 0, 4096, -96, 0, 0, 4096],
        [-4096, -120, 0, 0, -4096, -96, 0, 0, 4096],
        [4096, 4096, 0, 0, 4096, 0, 0, 0, 4096],
        [8193, 0, 0, 0, 4096, 0, 0, 0, 4096],
        [8192, 0, 0, 0, 4096, 0, 0, 0, 4096],
        [i32::MAX, 17, -31, 7, 4096, 8, -9, 10, 4096],
        [i32::MIN, 1, 2, 3, 4096, 4, 5, 6, 4096],
        [3840, 0, 0, 0, 4096, 0, 0, 0, 4096],
        [-3840, 0, 0, 0, -4096, 0, 0, 0, 4096],
        [0; 9],
    ];
    let mut input = Vec::new();
    let mut expected_fallbacks = 0;
    for m in matrices {
        let mut gram = [0i32; 9];
        for row in 0..3 {
            for col in 0..3 {
                for k in 0..3 {
                    gram[row * 3 + col] = gram[row * 3 + col]
                        .wrapping_add(m[row * 3 + k].wrapping_mul(m[col * 3 + k]));
                }
            }
        }
        let admissible = m.iter().all(|v| (-8192..=8192).contains(v))
            && (0..3).all(|row| {
                (0..3)
                    .map(|col| {
                        (i64::from(gram[row * 3 + col]) - if row == col { 1 << 24 } else { 0 })
                            .abs()
                    })
                    .sum::<i64>()
                    <= 1 << 21
            });
        expected_fallbacks += u64::from(!admissible);
        for v in m {
            input.extend(words(v));
        }
        for i in 0..9 {
            let mut numerator = 0i64;
            for k in 0..3 {
                let coefficient =
                    (if i / 3 == k { 3i32 << 24 } else { 0 }).wrapping_sub(gram[(i / 3) * 3 + k]);
                numerator = numerator
                    .wrapping_add(i64::from(coefficient).wrapping_mul(i64::from(m[k * 3 + i % 3])));
            }
            let magnitude = if numerator < 0 {
                numerator.wrapping_neg() as u64
            } else {
                numerator as u64
            };
            let rounded = (magnitude.wrapping_add(1 << 24) >> 25) as u32;
            let value = if numerator < 0 {
                rounded.wrapping_neg()
            } else {
                rounded
            };
            input.extend(words(value as i32));
        }
    }
    let code = compile(
        "polar-matrices",
        r#"
fn main(){let input=unsafe{Ptr::from_addr(0x2000).as_u16_array()};let a=unsafe{Ptr::from_addr(0x1100).as_u16_array()};let b=unsafe{Ptr::from_addr(0x1140).as_u16_array()};let g=unsafe{Ptr::from_addr(0x1180).as_u16_array()};let w=unsafe{Ptr::from_addr(0x11c0).as_u16_array()};let count=unsafe{Ptr::from_addr(0x1ffe).read(0)};let mut i=0u16;while i<count{let mut j=0u16;while j<9{attitude::put(a,j,fix32::load_words(input,i*36+j*2));j+=1;}attitude::polar_iteration(a,b,g,w);j=0;while j<9{if attitude::get(b,j)!=fix32::load_words(input,i*36+18+j*2){halt(1);}j+=1;}i+=1;}halt(0x600d);}
"#,
        output,
    )?;
    let (steps, attempts, fallbacks) = profile(&code, &input, matrices.len() as u16);
    assert_eq!(attempts, matrices.len() as u64);
    assert_eq!(fallbacks, expected_fallbacks);
    let mut input = Vec::new();
    for m in [-8192, -2, -1, 0, 1, 8192] {
        for r in [
            -(1 << 30),
            -(1 << 21),
            -(1 << 20),
            -1,
            0,
            1,
            (1 << 20) - 1,
            1 << 20,
            (1 << 20) + 1,
            (1 << 30) - 1,
        ] {
            for residue in [0, 1, 15] {
                let d = (i64::from(r) << 4) + i64::from(residue);
                let numerator = (i64::from(m) << 25) + d;
                let expected = ((numerator + numerator.signum() * (1 << 24)) / (1 << 25)) as i32;
                input.extend(words(m));
                input.extend(words(r));
                input.push(residue as u16);
                input.extend(words(expected));
            }
        }
    }
    let round = compile(
        "polar-round",
        r#"
fn main(){let input=unsafe{Ptr::from_addr(0x2000).as_u16_array()};let count=unsafe{Ptr::from_addr(0x1ffe).read(0)};let mut i=0u16;while i<count{let at=i*7;let result=attitude::polar_round(fix32::load_words(input,at),fix32::load_words(input,at+2),input[at+4]);if result!=fix32::load_words(input,at+5){halt(2);}i+=1;}halt(0x600d);}
"#,
        output,
    )?;
    let cases = input.len() / 7;
    let round_steps = sim(&round, &input, cases as u16, PASS);
    Ok(
        json!({"scope":"one exact polar iteration including out-of-range/wrapping wide fallback plus signed halfway/residue fixtures; matrix outputs checked against preserved wide 64-bit MAC/round semantics","matrices":{"cases":matrices.len(),"sim_instructions":steps,"fpu_attempts":attempts,"wide_fallbacks":fallbacks,"expected_wide_fallbacks":expected_fallbacks,"maximum_integer_error":0},"rounding":{"cases":cases,"sim_instructions":round_steps,"maximum_integer_error":0}}),
    )
}
