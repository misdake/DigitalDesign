//! Reproducible arithmetic qualification through rustc, RCC, sim and core emu.
use cpu_v3::{
    rcc_backend::{self, CompilerOptions, CpuV3Program},
    CpuV3Core, CpuV3CoreInput, CpuV3CoreInputValue, CpuV3Sim, StepOutcome,
};
use digital_design_circuit::build_circuit;
use digital_design_hardware::{Module, ModuleIo};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use serde_json::{json, Value};
use std::{error::Error, fs, path::Path};

#[path = "collision_check.rs"]
mod collision_check;
#[path = "polar_check.rs"]
mod polar_check;
#[path = "trig_check.rs"]
mod trig_check;
#[path = "vehicle_core_check.rs"]
mod vehicle_core_check;

const INPUT: usize = 0x2000;
const COUNT: usize = 0x1ffe;
const RESULT: usize = 0x1000;
const PASS: u16 = 0x600d;
const INTEGER: &str = include_str!("../rcc/integer.rs");
const ATTITUDE: &str = include_str!("../rcc/attitude.rs");
const SUSPENSION: &str = include_str!("../rcc/suspension.rs");
const DRIVE: &str = include_str!("../rcc/drive.rs");
const VEHICLE: &str = include_str!("../rcc/vehicle.rs");
const TRIG_LOOKUP: &str = include_str!("../rcc/trig_lookup.rs");
const COLLISION: &str = include_str!("../rcc/collision.rs");
const BODY: &str = include_str!("../rcc/body.rs");
const RANDOM: &str = include_str!("../rcc/random.rs");
const DEFORMATION: &str = include_str!("../rcc/deformation.rs");
const DAMAGE: &str = include_str!("../rcc/damage.rs");
const EFFECT_POOL: &str = include_str!("../rcc/effect_pool.rs");
const IMPACT: &str = include_str!("../rcc/impact.rs");
const GROUND_RESPONSE: &str = include_str!("../rcc/ground_response.rs");
const TANK_RESPONSE: &str = include_str!("../rcc/tank_response.rs");
const ORDERED: &str = include_str!("../rcc/ordered.rs");
const LIFECYCLE: &str = include_str!("../rcc/lifecycle.rs");
const CRUSH: &str = include_str!("../rcc/crush.rs");
const SPATIAL: &str = include_str!("../rcc/spatial.rs");
const FALLING: &str = include_str!("../rcc/falling.rs");
const BLAST: &str = include_str!("../rcc/blast.rs");
const SCENE_WORLD: &str = include_str!("../rcc/scene_world.rs");
const SCENE_CONTACT: &str = include_str!("../rcc/scene_contact.rs");
const BOUNCE: &str = include_str!("../rcc/bounce.rs");
const GROUND: &str = include_str!("../rcc/ground.rs");
const SAMPLING: &str = include_str!("../rcc/sampling.rs");
const TERRAIN: &str = include_str!("../rcc/terrain.rs");
const STATIC_ASSETS: &str = include_str!("../../../rcc/static_assets.rs");
const GPU_LAYOUT: &str = include_str!("../../../rcc/gpu_layout.rs");
const OBJECT_QUERY: &str = include_str!("../rcc/object_query.rs");
const TRIG_ASSET_LOOKUP: &str = include_str!("../rcc/trig_asset_lookup.rs");
const PREAMBLE: &str = "use crate::dsl_rt::*; mod integer; mod attitude; mod suspension; mod drive; mod vehicle; mod trig_lookup; mod collision; mod body; mod object_query; mod random; mod deformation; mod damage; mod effect_pool; mod impact; mod impact_capture; mod ground_response; mod tank_response; mod ordered; mod lifecycle; mod crush; mod spatial; mod falling; mod blast; mod scene_world; mod scene_contact; mod bounce; mod ground; mod sampling; mod terrain; mod static_assets; mod deform_contact; mod scene_query; mod gpu_window;\n";

fn compile(label: &str, body: &str, output: &Path) -> Result<CpuV3Program, Box<dyn Error>> {
    let folder = output.join(label);
    fs::create_dir_all(&folder)?;
    let source = format!("{PREAMBLE}{body}");
    fs::write(folder.join("input.rs"), &source)?;
    let options = CompilerOptions::for_separate_code_and_data_segments(0);
    let ir = compile_program_named_checked(
        label,
        &source,
        &options,
        &RustcCheck::new(folder.join("rustc")),
        &mut |name| match name {
            "integer" => Ok(INTEGER.to_owned()),
            "attitude" => Ok(ATTITUDE.to_owned()),
            "suspension" => Ok(SUSPENSION.to_owned()),
            "drive" => Ok(DRIVE.to_owned()),
            "vehicle" => Ok(VEHICLE.to_owned()),
            "trig_lookup" => Ok(TRIG_LOOKUP.to_owned()),
            "collision" => Ok(COLLISION.to_owned()),
            "body" => Ok(BODY.to_owned()),
            "object_query" => Ok(OBJECT_QUERY.to_owned()),
            "random" => Ok(RANDOM.to_owned()),
            "deformation" => Ok(DEFORMATION.to_owned()),
            "deform_contact" => Ok(include_str!("../rcc/deform_contact.rs").to_owned()),
            "scene_query" => Ok(include_str!("../rcc/scene_query.rs").to_owned()),
            "gpu_window" => Ok(include_str!("../../../rcc/gpu_window.rs").to_owned()),
            "damage" => Ok(DAMAGE.to_owned()),
            "effect_pool" => Ok(EFFECT_POOL.to_owned()),
            "impact" => Ok(IMPACT.to_owned()),
            "impact_capture" => Ok(include_str!("impact_capture.rs").to_owned()),
            "ground_response" => Ok(GROUND_RESPONSE.to_owned()),
            "tank_response" => Ok(TANK_RESPONSE.to_owned()),
            "ordered" => Ok(ORDERED.to_owned()),
            "lifecycle" => Ok(LIFECYCLE.to_owned()),
            "crush" => Ok(CRUSH.to_owned()),
            "spatial" => Ok(SPATIAL.to_owned()),
            "falling" => Ok(FALLING.to_owned()),
            "blast" => Ok(BLAST.to_owned()),
            "scene_world" => Ok(SCENE_WORLD.to_owned()),
            "scene_contact" => Ok(SCENE_CONTACT.to_owned()),
            "bounce" => Ok(BOUNCE.to_owned()),
            "ground" => Ok(GROUND.to_owned()),
            "sampling" => Ok(SAMPLING.to_owned()),
            "terrain" => Ok(TERRAIN.to_owned()),
            "static_assets" => Ok(STATIC_ASSETS.to_owned()),
            "gpu_layout" | "static_assets/gpu_layout" | "gpu_window/gpu_layout" => {
                Ok(GPU_LAYOUT.to_owned())
            }
            _ => Err(format!("unexpected module {name}")),
        },
    )?;
    let code = rcc_backend::try_compile(ir, &options, "main")?;
    fs::write(folder.join("program.asm"), &code.listing)?;
    Ok(code)
}

fn sim(code: &CpuV3Program, input: &[u16], count: u16, signal: u16) -> u64 {
    assert!(INPUT + input.len() < 0xa000, "input overlaps stack reserve");
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    cpu.physical_memory_mut()[INPUT..INPUT + input.len()].copy_from_slice(input);
    cpu.physical_memory_mut()[COUNT] = count;
    for steps in 1..=30_000_000 {
        if let StepOutcome::Halted { signal: actual } = cpu.step().expect("CPU instruction") {
            assert_eq!(actual, signal, "failed at pc {:04x}", cpu.pc());
            return steps;
        }
    }
    panic!("CPU sim exceeded 30,000,000 steps");
}

fn words(value: i32) -> [u16; 2] {
    [value as u16, (value >> 16) as u16]
}

fn random(seed: &mut u32) -> i32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed as i32
}

fn primitive_cases(output: &Path) -> Result<Value, Box<dyn Error>> {
    let edges = [
        i32::MIN,
        i32::MIN + 1,
        -65537,
        -65536,
        -4096,
        -1,
        0,
        1,
        4096,
        65535,
        65536,
        i32::MAX,
    ];
    let shifts = [0, 1, 12, 15, 16, 17, 24, 31, 32, 63, 255];
    let mut cases = Vec::new();
    for (i, &a) in edges.iter().enumerate() {
        for &shift in &shifts {
            cases.push((a, edges[(i + usize::from(shift)) % edges.len()], shift));
        }
    }
    let mut seed = 0x6bcf_0281;
    for i in 0..256 {
        cases.push((random(&mut seed), random(&mut seed), i));
    }
    let mut input = Vec::new();
    for &(a, b, shift) in &cases {
        input.extend(words(a));
        input.extend(words(b));
        input.push(shift);
        let trunc = if a < 0 {
            a.wrapping_neg().wrapping_shr(shift as u32).wrapping_neg()
        } else {
            a.wrapping_shr(shift as u32)
        };
        for value in [
            a.wrapping_add(b),
            a.wrapping_sub(b),
            a.wrapping_mul(b),
            a.wrapping_shl(shift as u32),
            a.wrapping_shr(shift as u32),
            trunc,
            if b == 0 { 0 } else { a.wrapping_div(b) },
        ] {
            input.extend(words(value));
        }
        input.push(u16::from(a < b));
        let low_product = u32::from(a as u16) * u32::from(b as u16);
        input.extend([(low_product >> 8) as u16, (low_product >> 16) as u16]);
    }
    let code = compile(
        "integer",
        r#"fn check(data: Array<u16>, at: u16, value: fix32, signal: u16) {
    if value != fix32::load_words(data, at) {
        halt(signal);
    }
}
fn main() {
    let data = unsafe { Ptr::from_addr(0x2000).as_u16_array() };
    let count = unsafe { Ptr::from_addr(0x1ffe).read(0) };
    let mut i = 0u16;
    while i < count {
        let at = i * 22;
        let a = fix32::load_words(data, at);
        let b = fix32::load_words(data, at + 2);
        let shift = data[at + 4];
        check(data, at + 5, a + b, 1);
        check(data, at + 7, a - b, 2);
        check(data, at + 9, integer::mul(a, b), 3);
        check(data, at + 11, integer::shl(a, shift), 4);
        check(data, at + 13, integer::sar(a, shift), 5);
        check(data, at + 15, integer::trunc_shift(a, shift), 6);
        if b != fix32::zero() {
            check(data, at + 17, integer::div(a, b), 7);
        }
        if (a < b) as u16 != data[at + 19] {
            halt(8);
        }
        if mul8(a.lo_bits(), b.lo_bits()) != data[at + 20] {
            halt(12);
        }
        if mul16(a.lo_bits(), b.lo_bits()) != data[at + 21] {
            halt(13);
        }
        i += 1;
    }
    halt(0x600d);
}
"#,
        output,
    )?;
    let steps = sim(&code, &input, cases.len() as u16, PASS);
    let zero = compile(
        "division-zero",
        "fn main(){ integer::div(integer::from_i16(1), fix32::zero()); halt(0x600d); }",
        output,
    )?;
    sim(&zero, &[], 0, 0x7b01);
    Ok(
        json!({"cases":cases.len(), "steps":steps, "code_bytes":code.total_code_words()*2,
              "zero_divisor_signal":"0x7b01"}),
    )
}

fn scaled_cases(output: &Path) -> Result<Value, Box<dyn Error>> {
    let code = compile(
        "scale-q12",
        r#"fn main() {
    let data = unsafe { Ptr::from_addr(0x2000).as_u16_array() };
    let count = unsafe { Ptr::from_addr(0x1ffe).read(0) };
    let mut i = 0u16;
    while i < count {
        let at = i * 7;
        let row = unsafe { Ptr::from_addr(0x2000 + at).as_i16_array() };
        integer::scale3_q12(row, data[at + 3]);
        let mut j = 0u16;
        while j < 3 {
            if row[j] != data[at + 4 + j] as i16 {
                halt(9);
            }
            j += 1;
        }
        i += 1;
    }
    halt(0x600d);
}
"#,
        output,
    )?;
    let mut input = Vec::new();
    let mut seed = 0x7e28_43d1;
    for d in 2048..=32768u16 {
        let x = (random(&mut seed) % 4097) as i16;
        for value in [4096i16, -4096, x] {
            input.push(value as u16);
        }
        input.push(d);
        for value in [4096i16, -4096, x] {
            input.push(((i32::from(value) * 4096) / i32::from(d)) as u16);
        }
    }
    for (x, y, z, d) in [
        (i16::MIN, i16::MAX, -1, 2048u16),
        (4096, -4096, 1, 1),
        (4096, -4096, 1, 65535),
        (32767, -32767, 4097, 8193),
        (1, -1, 0, 2047),
    ] {
        input.extend([x as u16, y as u16, z as u16, d]);
        input.extend([x, y, z].map(|v| ((i32::from(v) * 4096) / i32::from(d)) as u16));
    }
    let mut steps = 0;
    for rows in input.chunks(2048 * 7) {
        steps += sim(&code, rows, (rows.len() / 7) as u16, PASS);
    }
    Ok(
        json!({"triples": input.len()/7, "quotients":input.len()/7*3,
              "steps":steps, "code_bytes":code.total_code_words()*2,
              "maximum_integer_error":0}),
    )
}

fn integration_case(output: &Path) -> Result<Value, Box<dyn Error>> {
    let mut input = Vec::new();
    let mut position = i32::MIN + 4096;
    let mut velocity = -165_329i32;
    for i in 0..600i32 {
        let dt = if i & 1 == 0 { 16 } else { 33 };
        let acceleration = (i & 15) - 8;
        velocity = velocity.wrapping_add(acceleration * dt);
        velocity = velocity.wrapping_mul(4091) / 4096;
        position = position.wrapping_add(velocity.wrapping_mul(dt) / 1024);
        input.extend(words(position));
        input.extend(words(velocity));
    }
    let code = compile(
        "stateful-600",
        r#"fn main() {
    let expected = unsafe { Ptr::from_addr(0x2000).as_u16_array() };
    let mut position = fix32::from_words(0x1000, 0x8000);
    let mut velocity = fix32::from_words(0x7a2f, 0xfffd);
    let mut i = 0u16;
    while i < 600 {
        let mut dt = integer::from_i16(16);
        if i & 1 != 0 {
            dt = integer::from_i16(33);
        }
        let acceleration = integer::from_i16((i & 15) as i16 - 8i16);
        velocity = velocity + integer::mul(acceleration, dt);
        velocity = integer::trunc_shift(integer::mul(velocity, integer::from_i16(4091)), 12);
        position = position + integer::trunc_shift(integer::mul(velocity, dt), 10);
        if position != fix32::load_words(expected, i * 4) {
            halt(10);
        }
        if velocity != fix32::load_words(expected, i * 4 + 2) {
            halt(11);
        }
        i += 1;
    }
    halt(0x600d);
}
"#,
        output,
    )?;
    let steps = sim(&code, &input, 600, PASS);
    Ok(
        json!({"steps":600, "sim_steps":steps, "code_bytes":code.total_code_words()*2,
              "maximum_integer_error":0, "scope":"arithmetic integration, not vehicle attitude"}),
    )
}

fn core(code: &CpuV3Program, input_data: &[u16]) -> Value {
    let mut memory = vec![0u16; 65536];
    memory[INPUT..INPUT + input_data.len()].copy_from_slice(input_data);
    core_memory(code, memory)
}

fn core_memory(code: &CpuV3Program, mut memory: Vec<u16>) -> Value {
    let (mut circuit, (input, output)) = build_circuit(|| {
        let input = CpuV3CoreInput::allocate();
        let output = CpuV3Core::emu(&input);
        (input, output)
    });
    let mut instruction = None;
    let mut data = None;
    let mut start = None;
    let mut call_start = None;
    let mut call_end = None;
    for tick in 0..1_000_000u32 {
        input.drive(
            &mut circuit,
            &CpuV3CoreInputValue {
                reset: false,
                hold: false,
                instruction_request_ready: true,
                instruction_response_valid: instruction.is_some(),
                instruction_data: instruction.unwrap_or(0),
                instruction_error: false,
                data_request_ready: true,
                data_response_valid: data.is_some(),
                data_read_data: data.unwrap_or(0),
                data_error: false,
                data_line_copy_ready: true,
                data_cache_command_error: false,
                device_read_data: 0,
            },
        );
        circuit.execute_gates();
        let value = output.sample(&circuit);
        if value.instruction_request_valid && start.is_none() {
            start = Some(tick);
        }
        assert!(
            !value.fault,
            "core fault {} at {}",
            value.fault_code, value.fault_pc
        );
        if value.halted {
            assert_eq!(value.halt_signal as u16, PASS);
            return json!({"cycles":tick-start.unwrap()+1, "retired_words":value.retired_words,
                          "code_bytes":code.total_code_words()*2,
                          "result":[memory[RESULT],memory[RESULT+1],memory[RESULT+2]],
                          "step_call_cycles":call_start.zip(call_end).map(|(a,b)|b-a),
                          "state":memory[0x1100..0x113a].chunks_exact(2).map(|p| (u32::from(p[0]) | (u32::from(p[1])<<16)) as i32).collect::<Vec<_>>(),
                          "events":memory[0x1000],"ok":memory[0x1001] != 0});
        }
        instruction = value.instruction_request_valid.then(|| {
            u64::from(
                *code
                    .words
                    .get(value.instruction_address as u16 as usize)
                    .unwrap_or(&0),
            )
        });
        data = value.data_request_valid.then(|| {
            let at = value.data_address as u16 as usize;
            if value.data_write {
                if at == 0x1003 && value.data_write_data == 1 {
                    call_start = Some(tick);
                }
                if at == 0x1001 {
                    call_end = Some(tick);
                }
                memory[at] = value.data_write_data as u16;
                0
            } else {
                u64::from(memory[at])
            }
        });
        circuit.clock_tick();
    }
    panic!("core emu exceeded 1,000,000 cycles");
}

fn measurements(output: &Path) -> Result<Value, Box<dyn Error>> {
    let helpers = r#"fn exact3(mut values: Array<i16>, denominator: u16) {
    let mut i = 0u16;
    while i < 3 {
        let numerator = integer::shl(integer::from_i16(values[i]), 12);
        values[i] = integer::div(numerator, fix32::from_words(denominator, 0)).lo_bits() as i16;
        i += 1;
    }
}
"#;
    let mut results = serde_json::Map::new();
    let source = format!(
        r#"{helpers}
fn main() {{
    let data = unsafe {{ Ptr::from_addr(0x2000).as_u16_array() }};
    let mut a = fix32::load_words(data, 0);
    let b = fix32::load_words(data, 2);
    let mut i = 0u16;
    while i < 64 {{
        a = integer::mul(a, b);
        i += 1;
    }}
    let out = unsafe {{ Ptr::from_addr(0x1000).as_u16_array() }};
    fix32::store_words(out, 0, a);
    halt(0x600d);
}}
"#
    );
    let code = compile("mul-chain", &source, output)?;
    let result = core(&code, &[0x3211, 0xfffd, 0x0003, 0x0001]);
    let mut expected = 0xfffd3211u32;
    for _ in 0..64 {
        expected = expected.wrapping_mul(0x00010003);
    }
    assert_eq!(
        result["result"],
        json!([expected as u16, (expected >> 16) as u16, 0])
    );
    results.insert("mul-chain".to_owned(), result);
    let mut scaled = [4096i32, -100, 255];
    for _ in 0..8 {
        scaled = scaled.map(|v| v * 4096 / 4097);
    }
    for (label, operation) in [
        ("scale-exact", "exact3"),
        ("scale-q12", "integer::scale3_q12"),
    ] {
        let source = format!(
            r#"{helpers}
fn main() {{
    let values = unsafe {{ Ptr::from_addr(0x2000).as_i16_array() }};
    let mut i = 0u16;
    while i < 8 {{
        {operation}(values, 4097);
        i += 1;
    }}
    let out = Ptr::from_addr(0x1000);
    unsafe {{
        out.write(0, values[0u16] as u16);
        out.write(1, values[1u16] as u16);
        out.write(2, values[2u16] as u16);
    }}
    halt(0x600d);
}}
"#
        );
        let code = compile(label, &source, output)?;
        let result = core(&code, &[4096, (-100i16) as u16, 255]);
        assert_eq!(result["result"], json!(scaled.map(|v| v as u16)));
        results.insert(label.to_owned(), result);
    }
    assert_eq!(
        results["scale-exact"]["result"],
        results["scale-q12"]["result"]
    );
    Ok(
        json!({"scope":"CPU core, one-cycle instruction/data responses; excludes system cache, SDRAM and GPU",
              "multiply_iterations":64,"scale3_iterations":8,"cases":results}),
    )
}

fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .ok_or("usage: tankracer-physics-check OUTPUT_DIRECTORY")?;
    fs::create_dir_all(&output)?;
    fs::write(output.join("integer.rs"), INTEGER)?;
    fs::write(output.join("attitude.rs"), ATTITUDE)?;
    if let Some(fixture) = std::env::args_os().nth(2) {
        let limit = std::env::args()
            .nth(3)
            .map(|v| v.parse::<usize>())
            .transpose()?
            .unwrap_or(600);
        let mode = std::env::args().nth(4);
        let report = match mode.as_deref() {
            Some("collision") => collision_check::run(&output, Path::new(&fixture))?,
            Some("suspension") => suspension_cases(&output, Path::new(&fixture), limit)?,
            Some("drive") => drive_cases(&output, Path::new(&fixture), limit)?,
            Some("vehicle") => vehicle_cases(&output, Path::new(&fixture), limit)?,
            Some("trig-assets") => trig_check::run(&output, Path::new(&fixture))?,
            Some("polar-kernel") => polar_check::run(&output)?,
            Some("vehicle-core-candidate") => {
                let qualified = std::env::args_os()
                    .nth(5)
                    .ok_or("candidate needs baseline qualified directory")?;
                vehicle_core_check::run_candidate(
                    &output,
                    Path::new(&fixture),
                    Path::new(&qualified),
                )?
            }
            Some("vehicle-core") => {
                let qualified = std::env::args_os()
                    .nth(5)
                    .ok_or("vehicle-core needs qualified run directory as argument 5")?;
                vehicle_core_check::run(&output, Path::new(&fixture), Path::new(&qualified))?
            }
            _ => attitude_cases(&output, Path::new(&fixture), limit)?,
        };
        let report_name = format!("{}-report.json", mode.as_deref().unwrap_or("attitude"));
        fs::write(
            output.join(report_name),
            serde_json::to_string_pretty(&report)? + "\n",
        )?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let report = json!({
        "integer":primitive_cases(&output)?,
        "scale_q12":scaled_cases(&output)?,
        "integration":integration_case(&output)?,
        "measurements":measurements(&output)?,
    });
    fs::write(
        output.join("report.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn attitude_cases(output: &Path, fixture: &Path, limit: usize) -> Result<Value, Box<dyn Error>> {
    let fixture_data: Value = serde_json::from_slice(&fs::read(fixture.join("attitude.json"))?)?;
    let code = compile(
        "attitude",
        r#"fn main() {
    let state = unsafe { Ptr::from_addr(0x1100).as_u16_array() };
    let config = unsafe { Ptr::from_addr(0x1200).as_u16_array() };
    let input = unsafe { Ptr::from_addr(0x2000).as_u16_array() };
    let sine = unsafe { Ptr::from_addr(0x3000).as_i16_array() };
    let angle = unsafe { Ptr::from_addr(0x8000).as_i16_array() };
    let mut event = unsafe { Ptr::from_addr(0x1000).as_u16_array() };
    let count = unsafe { Ptr::from_addr(0x1ffe).read(0) };
    let mut tick = 0u16;
    while tick < count {
        let dt = attitude::get(input, 0);
        attitude::put(state, 24, fix32::load_words(input, 3));
        attitude::put(state, 25, fix32::load_words(input, 5));
        let mut j = 0u16;
        while j < 18 {
            let mut s = state;
            s[j] = input[7 + j];
            j += 1;
        }
        event[3u16] = 1;
        let ok = attitude::step(state, config, input, sine, angle, event);
        event[1u16] = ok as u16;
        tick += 1;
        event[2u16] = tick;
    }
    halt(0x600d);
}
"#,
        output,
    )?;
    let tables = [
        fs::read(fixture.join("sine.bin"))?,
        fs::read(fixture.join("angle.bin"))?,
    ];
    let mut reports = Vec::new();
    let mut core_reports = Vec::new();
    for case in fixture_data["cases"].as_array().ok_or("missing cases")? {
        let frames = case["frames"].as_array().ok_or("missing frames")?;
        let count = frames.len().min(limit);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (bytes, address) in tables.iter().zip([0x3000, 0x8000]) {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                memory[address + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        for (i, value) in case["initial"].as_array().unwrap().iter().enumerate() {
            memory[0x1100 + i * 2..0x1102 + i * 2]
                .copy_from_slice(&words(value.as_i64().unwrap() as i32));
        }
        for (i, value) in case["config"].as_array().unwrap().iter().enumerate() {
            memory[0x1200 + i] = value.as_u64().unwrap() as u16;
        }
        memory[COUNT] = count as u16;
        feed_attitude(memory, &frames[0]["input"]);
        let mut completed = 0usize;
        let mut last = 0u64;
        let mut call_steps = Vec::new();
        let maximum = count as u64 * 1_000_000 + 1_000;
        let fast_entry = polar_symbol(&code, "polar_fpu");
        let wide_entry = polar_symbol(&code, "polar_wide");
        let mut fpu_attempts = 0u64;
        let mut wide_fallbacks = 0u64;
        let mut halted = false;
        for steps in 1..=maximum {
            fpu_attempts += u64::from(Some(cpu.pc()) == fast_entry);
            wide_fallbacks += u64::from(Some(cpu.pc()) == wide_entry);
            let outcome = cpu.step().expect("attitude instruction");
            let pc = cpu.pc();
            let memory = cpu.physical_memory_mut();
            if memory[0x1002] as usize > completed {
                let expected = &frames[completed];
                if limit <= 10 {
                    let actual: Vec<i32> = memory[0x1100..0x113a]
                        .chunks_exact(2)
                        .map(|p| (u32::from(p[0]) | (u32::from(p[1]) << 16)) as i32)
                        .collect();
                    eprintln!(
                        "{} frame {} actual {:?} expected {}",
                        case["name"],
                        completed + 1,
                        actual,
                        expected["state"]
                    );
                }
                assert_eq!(memory[0x1002] as usize, completed + 1);
                assert_eq!(
                    memory[0x1001],
                    u16::from(expected["ok"].as_bool().unwrap()),
                    "{} frame {} success decision",
                    case["name"],
                    completed + 1
                );
                assert_eq!(
                    memory[0x1000],
                    expected["events"].as_u64().unwrap() as u16,
                    "{} frame {} branch decisions",
                    case["name"],
                    completed + 1
                );
                for (i, value) in expected["state"].as_array().unwrap().iter().enumerate() {
                    let actual = (u32::from(memory[0x1100 + i * 2])
                        | (u32::from(memory[0x1101 + i * 2]) << 16))
                        as i32;
                    assert_eq!(
                        actual,
                        value.as_i64().unwrap() as i32,
                        "{} frame {} state[{}], pc {:04x}",
                        case["name"],
                        completed + 1,
                        i,
                        pc
                    );
                }
                call_steps.push(steps - last);
                last = steps;
                completed += 1;
                if completed < count {
                    feed_attitude(memory, &frames[completed]["input"]);
                }
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, PASS);
                assert_eq!(completed, count);
                halted = true;
                break;
            }
        }
        assert!(
            halted,
            "{} exceeded bounded instruction count",
            case["name"]
        );
        if case["name"] == "orthogonal-16ms" && limit >= 10 {
            for index in [0usize, 9] {
                let mut memory = vec![0u16; 65536];
                for (bytes, address) in tables.iter().zip([0x3000, 0x8000]) {
                    for (i, pair) in bytes.chunks_exact(2).enumerate() {
                        memory[address + i] = u16::from_le_bytes([pair[0], pair[1]]);
                    }
                }
                let initial = if index == 0 {
                    &case["initial"]
                } else {
                    &frames[index - 1]["state"]
                };
                for (i, value) in initial.as_array().unwrap().iter().enumerate() {
                    memory[0x1100 + i * 2..0x1102 + i * 2]
                        .copy_from_slice(&words(value.as_i64().unwrap() as i32));
                }
                for (i, value) in case["config"].as_array().unwrap().iter().enumerate() {
                    memory[0x1200 + i] = value.as_u64().unwrap() as u16;
                }
                memory[COUNT] = 1;
                feed_attitude(&mut memory, &frames[index]["input"]);
                let mut result = core_memory(&code, memory);
                assert_eq!(
                    result["state"],
                    frames[index]["state"],
                    "core frame {} state",
                    index + 1
                );
                assert_eq!(result["ok"], frames[index]["ok"]);
                assert_eq!(result["events"], frames[index]["events"]);
                result["case"] = case["name"].clone();
                result["frame"] = json!(index + 1);
                core_reports.push(result);
            }
        }
        reports.push(json!({"case":case["name"],"consecutive_updates":completed,"sim_instructions":last,"maximum_integer_error":0,"branch_differences":0,"per_update_instructions":call_steps,"fpu_attempts":fpu_attempts,"wide_fallbacks":wide_fallbacks}));
    }
    Ok(
        json!({"scope":"instruction simulator; frozen contact deltas and old reference; attitude does not consume RNG; excludes vehicle forces, terrain queries, system cache/SDRAM/GPU and driving feel", "code_bytes":code.total_code_words()*2,"cases":reports,
        "core_measurements":{"scope":"CPU core with one-cycle instruction/data responses; cycles cover complete single-update harness; step_call_cycles span the pre-call marker store request to post-return ok store request, including call setup/return and ok store; excludes system cache, contention, SDRAM and GPU","cases":core_reports}}),
    )
}

fn feed_attitude(memory: &mut [u16], input: &Value) {
    memory[INPUT..INPUT + 2].copy_from_slice(&words(input["dt"].as_i64().unwrap() as i32));
    memory[INPUT + 2] = input["frame"].as_u64().unwrap() as u16;
    memory[INPUT + 3..INPUT + 5]
        .copy_from_slice(&words(input["longitudinal"].as_i64().unwrap() as i32));
    memory[INPUT + 5..INPUT + 7].copy_from_slice(&words(input["lateral"].as_i64().unwrap() as i32));
    for (i, v) in input["reference"].as_array().unwrap().iter().enumerate() {
        memory[INPUT + 7 + i * 2..INPUT + 9 + i * 2]
            .copy_from_slice(&words(v.as_i64().unwrap() as i32));
    }
}

fn suspension_cases(output: &Path, fixture: &Path, limit: usize) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("suspension.json"))?)?;
    fs::write(output.join("suspension.rs"), SUSPENSION)?;
    let code = compile(
        "suspension",
        r#"fn main() {
    let state = unsafe { Ptr::from_addr(0x1100).as_u16_array() };
    let config = unsafe { Ptr::from_addr(0x1200).as_u16_array() };
    let env = unsafe { Ptr::from_addr(0x2000).as_u16_array() };
    let mut events = unsafe { Ptr::from_addr(0x1000).as_u16_array() };
    let count = unsafe { Ptr::from_addr(0x1ffe).read(0) };
    let mut i = 0u16;
    while i < count {
        suspension::step(state, config, env, events);
        i += 1;
        events[3u16] = i;
    }
    halt(0x600d);
}
"#,
        output,
    )?;
    let mut reports = Vec::new();
    for case in data["cases"].as_array().unwrap() {
        let frames = case["frames"].as_array().unwrap();
        let count = frames.len().min(limit);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        for (value, address) in [(&case["initial"], 0x1100usize), (&case["config"], 0x1200)] {
            for (i, v) in value.as_array().unwrap().iter().enumerate() {
                cpu.physical_memory_mut()[address + i * 2..address + i * 2 + 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        cpu.physical_memory_mut()[COUNT] = count as u16;
        feed_suspension(cpu.physical_memory_mut(), &frames[0]["input"]);
        let mut done = 0;
        let mut last = 0;
        let mut per_update = Vec::new();
        let mut halted = false;
        for steps in 1..=count as u64 * 1_000_000 + 1000 {
            let outcome = cpu.step().expect("suspension instruction");
            let memory = cpu.physical_memory_mut();
            if memory[0x1003] as usize > done {
                let expected = &frames[done];
                let actual: Vec<i32> = memory[0x1100..0x1158]
                    .chunks_exact(2)
                    .map(|p| (u32::from(p[0]) | (u32::from(p[1]) << 16)) as i32)
                    .collect();
                assert_eq!(
                    json!(actual),
                    expected["state"],
                    "{} frame {} suspension state",
                    case["name"],
                    done + 1
                );
                assert_eq!(
                    json!(&memory[0x1000..0x1003]),
                    expected["events"],
                    "{} frame {} events",
                    case["name"],
                    done + 1
                );
                assert_eq!(expected["rng_before"], expected["rng_after"]);
                done += 1;
                per_update.push(steps - last);
                last = steps;
                if done < count {
                    feed_suspension(memory, &frames[done]["input"]);
                }
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, PASS);
                assert_eq!(done, count);
                halted = true;
                break;
            }
        }
        assert!(halted);
        reports.push(json!({"case":case["name"],"consecutive_updates":done,"sim_instructions":last,"per_update_instructions":per_update,"maximum_integer_error":0,"branch_differences":0}));
    }
    let report = json!({"scope":data["scope"],"state_i32_values":44,"code_bytes":code.total_code_words()*2,"cases":reports});
    fs::write(
        output.join("suspension-report.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    Ok(report)
}
fn feed_suspension(memory: &mut [u16], input: &Value) {
    for (i, v) in input["environment"].as_array().unwrap().iter().enumerate() {
        memory[INPUT + i * 2..INPUT + i * 2 + 2]
            .copy_from_slice(&words(v.as_i64().unwrap() as i32));
    }
    for i in 0..4 {
        for (key, offset) in [("kinds", 0), ("gaps", 1)] {
            let value = input[key][i].as_i64().unwrap() as i32;
            memory[0x1100 + i * 12 + offset * 2..0x1102 + i * 12 + offset * 2]
                .copy_from_slice(&words(value));
        }
    }
    memory[0x1100 + 36 * 2..0x1100 + 38 * 2].fill(0);
}

fn drive_cases(output: &Path, fixture: &Path, limit: usize) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("drive.json"))?)?;
    fs::write(output.join("drive.rs"), DRIVE)?;
    let code = compile(
        "drive",
        r#"fn main() {
    let s = unsafe { Ptr::from_addr(0x1100).as_u16_array() };
    let c = unsafe { Ptr::from_addr(0x1200).as_u16_array() };
    let f = unsafe { Ptr::from_addr(0x2000).as_u16_array() };
    let mut e = unsafe { Ptr::from_addr(0x1000).as_u16_array() };
    let count = unsafe { Ptr::from_addr(0x1ffe).read(0) };
    let mut i = 0u16;
    while i < count {
        let status = drive::step(s, c, f);
        e[0u16] = status;
        i += 1;
        e[1u16] = i;
    }
    halt(0x600d);
}
"#,
        output,
    )?;
    let mut reports = Vec::new();
    for case in data["cases"].as_array().unwrap() {
        let frames = case["frames"].as_array().unwrap();
        let count = frames.len().min(limit);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        for (value, address) in [(&case["initial"], 0x1100usize), (&case["config"], 0x1200)] {
            for (i, v) in value.as_array().unwrap().iter().enumerate() {
                cpu.physical_memory_mut()[address + i * 2..address + i * 2 + 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        cpu.physical_memory_mut()[COUNT] = count as u16;
        feed_drive(cpu.physical_memory_mut(), &frames[0]["input"]);
        let mut done = 0;
        let mut last = 0;
        let mut per = Vec::new();
        let mut halted = false;
        for steps in 1..=count as u64 * 1_000_000 + 1000 {
            let outcome = cpu.step().expect("drive instruction");
            let m = cpu.physical_memory_mut();
            if m[0x1001] as usize > done {
                let expected = &frames[done];
                let actual: Vec<i32> = m[0x1100..0x1120]
                    .chunks_exact(2)
                    .map(|p| (u32::from(p[0]) | (u32::from(p[1]) << 16)) as i32)
                    .collect();
                assert_eq!(
                    json!(actual),
                    expected["state"],
                    "{} frame {} drive",
                    case["name"],
                    done + 1
                );
                assert_eq!(m[0x1000], expected["status"].as_u64().unwrap() as u16);
                done += 1;
                per.push(steps - last);
                last = steps;
                if done < count {
                    feed_drive(m, &frames[done]["input"]);
                }
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, PASS);
                assert_eq!(done, count);
                halted = true;
                break;
            }
        }
        assert!(halted);
        reports.push(json!({"case":case["name"],"consecutive_updates":done,"sim_instructions":last,"per_update_instructions":per,"maximum_integer_error":0,"status_differences":0}));
    }
    Ok(
        json!({"scope":data["scope"],"code_bytes":code.total_code_words()*2,"state_i32_values":16,"cases":reports}),
    )
}
fn feed_drive(m: &mut [u16], input: &Value) {
    for (i, v) in input["frame"].as_array().unwrap().iter().enumerate() {
        m[INPUT + i * 2..INPUT + i * 2 + 2].copy_from_slice(&words(v.as_i64().unwrap() as i32));
    }
    m[0x111e..0x1120].copy_from_slice(&words(input["load"].as_i64().unwrap() as i32));
}

fn read_i32(memory: &[u16], address: usize, count: usize) -> Value {
    json!(memory[address..address + count * 2]
        .chunks_exact(2)
        .map(|p| (u32::from(p[0]) | (u32::from(p[1]) << 16)) as i32)
        .collect::<Vec<_>>())
}
fn write_i32(memory: &mut [u16], address: usize, values: &Value) {
    for (i, v) in values.as_array().unwrap().iter().enumerate() {
        memory[address + i * 2..address + i * 2 + 2]
            .copy_from_slice(&words(v.as_i64().unwrap() as i32));
    }
}
fn vehicle_cases(output: &Path, fixture: &Path, limit: usize) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("vehicle.json"))?)?;
    for (name, source) in [
        ("suspension.rs", SUSPENSION),
        ("drive.rs", DRIVE),
        ("vehicle.rs", VEHICLE),
    ] {
        fs::write(output.join(name), source)?;
    }
    let code = compile(
        "vehicle",
        r#"fn main() {
    let a = unsafe { Ptr::from_addr(0x1100).as_u16_array() };
    let s = unsafe { Ptr::from_addr(0x1180).as_u16_array() };
    let d = unsafe { Ptr::from_addr(0x1300).as_u16_array() };
    let m = unsafe { Ptr::from_addr(0x1380).as_u16_array() };
    let b = unsafe { Ptr::from_addr(0x1400).as_u16_array() };
    let sc = unsafe { Ptr::from_addr(0x1500).as_u16_array() };
    let dc = unsafe { Ptr::from_addr(0x1600).as_u16_array() };
    let ac = unsafe { Ptr::from_addr(0x1900).as_u16_array() };
    let pc = unsafe { Ptr::from_addr(0x1920).as_u16_array() };
    let se = unsafe { Ptr::from_addr(0x2000).as_u16_array() };
    let f = unsafe { Ptr::from_addr(0x2100).as_u16_array() };
    let mut e = unsafe { Ptr::from_addr(0x1000).as_u16_array() };

    let sine = unsafe { Ptr::from_addr(0x3000).as_i16_array() };
    let angle = unsafe { Ptr::from_addr(0x8000).as_i16_array() };
    let count = unsafe { Ptr::from_addr(0x1ffe).read(0) };
    let mut i = 0u16;
    while i < count {
        e[8u16] = 0;
        vehicle::boost(b, dc, f);
        vehicle::contact_stage(a, s, m, sc, se, e);
        attitude::put(d, 15, attitude::get(s, 40));
        e[8u16] = 1;
        let status = vehicle::power_stage(a, d, m, dc, f, attitude::get(s, 30));
        e[5u16] = status;
        if status != 0 {
            halt(0x7c01);
        }
        e[8u16] = 2;
        vehicle::integrate_position(m, f);
        if !vehicle::attitude_stage(a, d, ac, f, sine, angle) {
            halt(0x7c02);
        }
        vehicle::finish(s, a, m, pc, f);
        i += 1;
        e[9u16] = i;
    }
    halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.bin"))?;
    let angle = fs::read(fixture.join("angle.bin"))?;
    let mut reports = Vec::new();
    for case in data["cases"].as_array().unwrap() {
        let frames = case["frames"].as_array().unwrap();
        let count = frames.len().min(limit);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (bytes, address) in [(&sine, 0x3000), (&angle, 0x8000)] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                memory[address + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        for (name, address) in [
            ("attitude", 0x1100),
            ("suspension", 0x1180),
            ("drive", 0x1300),
            ("motion", 0x1380),
            ("boost", 0x1400),
        ] {
            write_i32(memory, address, &case["initial"][name]);
        }
        for (name, address) in [
            ("suspension_config", 0x1500),
            ("drive_config", 0x1600),
            ("pose_config", 0x1920),
        ] {
            write_i32(memory, address, &case[name]);
        }
        for (i, v) in case["attitude_config"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            memory[0x1900 + i] = v.as_u64().unwrap() as u16;
        }
        memory[COUNT] = count as u16;
        feed_vehicle(memory, &frames[0]["input"]);
        let mut done = 0;
        let mut stage = 0u16;
        let mut last = 0;
        let mut per = Vec::new();
        let mut halted = false;
        let mut stage_checks = 0;
        let fast_entry = polar_symbol(&code, "polar_fpu");
        let wide_entry = polar_symbol(&code, "polar_wide");
        let mut fpu_attempts = 0u64;
        let mut wide_fallbacks = 0u64;
        for steps in 1..=count as u64 * 1_000_000 + 1000 {
            fpu_attempts += u64::from(Some(cpu.pc()) == fast_entry);
            wide_fallbacks += u64::from(Some(cpu.pc()) == wide_entry);
            let outcome = cpu.step().expect("vehicle instruction");
            let memory = cpu.physical_memory_mut();
            if memory[0x1008] != stage {
                stage = memory[0x1008];
                if stage == 1 {
                    assert_eq!(
                        read_i32(memory, 0x1180, 44),
                        frames[done]["after_suspension"],
                        "{} frame {} suspension",
                        case["name"],
                        done + 1
                    );
                    assert_eq!(
                        json!(&memory[0x1000..0x1003]),
                        frames[done]["suspension_events"]
                    );
                    stage_checks += 1;
                }
                if stage == 2 {
                    assert_eq!(
                        read_i32(memory, 0x1300, 16),
                        frames[done]["after_powertrain"]["drive"],
                        "{} frame {} powertrain",
                        case["name"],
                        done + 1
                    );
                    assert_eq!(
                        read_i32(memory, 0x1380, 3),
                        frames[done]["after_powertrain"]["velocity"],
                        "{} frame {} world velocity",
                        case["name"],
                        done + 1
                    );
                    assert_eq!(
                        read_i32(memory, 0x1100 + 21 * 2, 3),
                        frames[done]["after_powertrain"]["angular"]
                    );
                    stage_checks += 1;
                }
            }
            if memory[0x1009] as usize > done {
                let expected = &frames[done]["state"];
                for (name, address, count) in [
                    ("attitude", 0x1100, 29),
                    ("suspension", 0x1180, 44),
                    ("drive", 0x1300, 16),
                    ("motion", 0x1380, 16),
                    ("boost", 0x1400, 4),
                ] {
                    assert_eq!(
                        read_i32(memory, address, count),
                        expected[name],
                        "{} frame {} {}",
                        case["name"],
                        done + 1,
                        name
                    );
                }
                assert_eq!(frames[done]["rng_before"], frames[done]["rng_after"]);
                done += 1;
                per.push(steps - last);
                last = steps;
                if done < count {
                    feed_vehicle(memory, &frames[done]["input"]);
                }
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, PASS, "{} after {} frames", case["name"], done);
                assert_eq!(done, count);
                halted = true;
                break;
            }
        }
        assert!(halted);
        reports.push(json!({"case":case["name"],"consecutive_updates":done,"sim_instructions":last,"per_update_instructions":per,"intermediate_stage_checks":stage_checks,"fpu_attempts":fpu_attempts,"wide_fallbacks":wide_fallbacks,"maximum_integer_error":0,"event_differences":0}));
    }
    Ok(
        json!({"scope":data["scope"],"code_bytes":code.total_code_words()*2,"exported_i32_values":109,"cases":reports,"timing_scope":"instruction counts only; complete tick core cycles not measured"}),
    )
}
fn feed_vehicle(memory: &mut [u16], input: &Value) {
    write_i32(memory, 0x2000, &input["environment"]);
    write_i32(memory, 0x2100, &input["frame"]);
    for i in 0..4 {
        for (key, offset) in [("kinds", 0), ("gaps", 1)] {
            memory[0x1180 + i * 12 + offset * 2..0x1182 + i * 12 + offset * 2]
                .copy_from_slice(&words(input[key][i].as_i64().unwrap() as i32));
        }
    }
    memory[0x1180 + 36 * 2..0x1180 + 38 * 2].fill(0);
}

fn polar_symbol(code: &CpuV3Program, name: &str) -> Option<u16> {
    let prefix = format!("{name} @ 0x");
    code.listing.lines().find_map(|line| {
        line.strip_prefix(&prefix)
            .and_then(|value| u16::from_str_radix(value.trim(), 16).ok())
    })
}
