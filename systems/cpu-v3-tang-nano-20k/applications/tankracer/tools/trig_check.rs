//! Producer -> standard AssetBytes placement/boot packing -> existing scoped SDK read.
use super::*;
use cpu_v3_tang_nano_20k::boot;
use rcc::frontend::static_assets::AssetBank;
fn compile_asset(
    label: &str,
    body: &str,
    out: &Path,
    producer: &Path,
    start: usize,
) -> Result<(CpuV3Program, Vec<u16>, Value), Box<dyn Error>> {
    let folder = out.join(label);
    fs::create_dir_all(&folder)?;
    let sine = producer
        .join("sine.i16.bin")
        .to_string_lossy()
        .replace('\\', "/");
    let angle = producer
        .join("angle.i16.bin")
        .to_string_lossy()
        .replace('\\', "/");
    let roots=format!("mod static_assets;use static_assets::*;\n#[asset] static SINE_BYTES:AssetBytes=AssetBytes::new(include_bytes!(\"{sine}\"));\n#[asset] static ANGLE_BYTES:AssetBytes=AssetBytes::new(include_bytes!(\"{angle}\"));\n");
    let source = format!("{PREAMBLE}{roots}{body}");
    fs::write(folder.join("input.rs"), &source)?;
    let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
    options.asset_banks = vec![AssetBank {
        bytes: start..start + 0x20000,
    }];
    let sdk = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let mut ir = rcc::frontend::compile_program_named_with_libraries_checked(
        label,
        &source,
        &options,
        &[],
        &RustcCheck::new(folder.join("rustc")),
        &mut |name| {
            let name = name.rsplit('/').next().unwrap();
            let local = match name {
                "integer" => Some(INTEGER),
                "attitude" => Some(ATTITUDE),
                "suspension" => Some(SUSPENSION),
                "drive" => Some(DRIVE),
                "vehicle" => Some(VEHICLE),
                "trig_lookup" => Some(TRIG_ASSET_LOOKUP),
                _ => None,
            };
            if let Some(source) = local {
                return Ok(rcc::frontend::ModuleSource {
                    namespace: rcc::frontend::ModuleNamespace::Scoped,
                    source: source.to_owned(),
                    origin: None,
                });
            }
            if ["static_assets", "gpu_window", "gpu_layout"].contains(&name) {
                let origin = sdk
                    .join(format!("{name}.rs"))
                    .canonicalize()
                    .map_err(|e| e.to_string())?;
                let source = fs::read_to_string(&origin).map_err(|e| e.to_string())?;
                return Ok(rcc::frontend::ModuleSource {
                    namespace: rcc::frontend::ModuleNamespace::Flat,
                    source,
                    origin: Some(origin),
                });
            }
            Err(format!("unexpected asset module {name}"))
        },
    )?;
    assert_eq!(ir.assets.placed.len(), 2);
    let mut memory = vec![0u16; 0x100000];
    let mut resources = Vec::new();
    let mut placed = Vec::new();
    for asset in &ir.assets.placed {
        let load = asset.load_data();
        for (i, pair) in load.chunks_exact(2).enumerate() {
            memory[asset.address / 2 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        resources.push(boot::BootResource::copy(asset.address, load));
        placed.push(json!({"name":asset.name,"address_bytes":asset.address,"payload_bytes":asset.data.len(),"allocated_bytes":asset.load_data().len(),"first_page":asset.address/32768,"last_page":(asset.address+asset.data.len()-1)/32768}));
    }
    let static_words = ir.debug.static_data_end;
    let mut static_data = vec![0u8; static_words * 2];
    for (at, value) in ir.take_static_initializers() {
        memory[at as usize] = value;
        static_data[at as usize * 2..at as usize * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    let code = rcc_backend::try_compile(ir, &options, "main")?;
    fs::write(folder.join("program.asm"), &code.listing)?;
    let binary: Vec<u8> = code.words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let package =
        boot::pack_application_with_data(&binary, static_words, &static_data, &resources)?;
    fs::write(folder.join("application.boot.bin"), package)?;
    fs::write(folder.join("application.static-data"), static_data)?;
    Ok((
        code,
        memory,
        json!({"placed":placed,"static_words":static_words,"code_bytes":binary.len()}),
    ))
}
fn prepare(memory: &mut [u16], case: &Value, previous: &Value, frame: &Value) {
    for (name, address) in [
        ("attitude", 0x1100),
        ("suspension", 0x1180),
        ("drive", 0x1300),
        ("motion", 0x1380),
        ("boost", 0x1400),
    ] {
        write_i32(memory, address, &previous[name]);
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
    feed_vehicle(memory, &frame["input"]);
}
pub(super) fn run(output: &Path, producer: &Path) -> Result<Value, Box<dyn Error>> {
    let base = producer.parent().ok_or("producer parent missing")?;
    let vehicle_fixture = base.parent().unwrap().join("vehicle-node/fixtures-final");
    let fixture: Value = serde_json::from_slice(&fs::read(vehicle_fixture.join("vehicle.json"))?)?;
    let sine = fs::read(producer.join("sine.i16.bin"))?;
    let angle = fs::read(producer.join("angle.i16.bin"))?;
    let mut all_reports = Vec::new();
    for (label, start) in [("aligned", 0x100000), ("page-tail", 0x107f80)] {
        let (code, mut memory, placement) = compile_asset(
            label,
            r#"
fn check(actual:Array<i16>,expected:Array<i16>,count:u16){let mut i=0u16;while i<count{if trig_lookup::index(actual,i)!=integer::from_i16(expected[i]){halt(1);}if read_dseg_page(3)!=3{halt(2);}i+=1;}}
fn main(){let sine=unsafe{addr_of(&SINE_BYTES).as_i16_array()};let angle=unsafe{addr_of(&ANGLE_BYTES).as_i16_array()};check(sine,unsafe{Ptr::from_addr(0x3000).as_i16_array()},20480);check(angle,unsafe{Ptr::from_addr(0x8000).as_i16_array()},257);halt(0x600d);}
"#,
            output,
            producer,
            start,
        )?;
        for (bytes, address) in [(&sine, 0x3000), (&angle, 0x8000)] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                memory[address + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        let mut cpu = CpuV3Sim::with_physical_memory_words(memory.len());
        cpu.physical_memory_mut().copy_from_slice(&memory);
        cpu.set_boot_window(&code.words);
        let mut transitions = 0u64;
        let mut last_page = 3;
        let mut halted = false;
        let mut used = 0;
        for steps in 1..=50_000_000u64 {
            let outcome = cpu.step().expect("trig asset instruction");
            let page = cpu.data_segments()[3];
            if page != last_page {
                transitions += 1;
                last_page = page;
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, PASS);
                assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
                halted = true;
                used = steps;
                break;
            }
        }
        assert!(halted);
        all_reports.push(json!({"placement":label,"sine_entries_checked":20480,"angle_entries_checked":257,"sim_instructions":used,"dseg3_transitions":transitions,"placement_metadata":placement,"maximum_integer_error":0,"mapping_restored_after_every_read":true}));
    }
    let (bounds_code, bounds_memory, _) = compile_asset(
        "bounds",
        r#"
fn main(){let which=unsafe{Ptr::from_addr(0x1ffe).read(0)};if which==0{trig_lookup::index(unsafe{addr_of(&SINE_BYTES).as_i16_array()},20480);}else{trig_lookup::index(unsafe{addr_of(&ANGLE_BYTES).as_i16_array()},257);}halt(0x600d);}
"#,
        output,
        producer,
        0x107f80,
    )?;
    for which in 0..2u16 {
        let mut cpu = CpuV3Sim::with_physical_memory_words(bounds_memory.len());
        cpu.physical_memory_mut().copy_from_slice(&bounds_memory);
        cpu.physical_memory_mut()[COUNT] = which;
        cpu.set_boot_window(&bounds_code.words);
        let mut halted = false;
        for _ in 0..10000 {
            if let StepOutcome::Halted { signal } = cpu.step().expect("bounds instruction") {
                assert_eq!(signal, 0x7d01);
                assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
                halted = true;
                break;
            }
        }
        assert!(halted);
    }
    let qualified = base
        .parent()
        .unwrap()
        .join("vehicle-node/check-final/vehicle/input.rs");
    let source = fs::read_to_string(qualified)?;
    let body = &source[source.find("fn main()").ok_or("vehicle main missing")?..];
    let body = body
        .replace("Ptr::from_addr(0x3000)", "addr_of(&SINE_BYTES)")
        .replace("Ptr::from_addr(0x8000)", "addr_of(&ANGLE_BYTES)");
    let (code, memory, placement) = compile_asset("vehicle", &body, output, producer, 0x100000)?;
    let mut short = Vec::new();
    for case in fixture["cases"].as_array().unwrap().iter().take(4) {
        let frames = case["frames"].as_array().unwrap();
        let count = 10usize;
        let mut m = memory.clone();
        prepare(&mut m, case, &case["initial"], &frames[0]);
        m[COUNT] = count as u16;
        let mut cpu = CpuV3Sim::with_physical_memory_words(m.len());
        cpu.physical_memory_mut().copy_from_slice(&m);
        cpu.set_boot_window(&code.words);
        let mut done = 0;
        let mut used = 0;
        let mut halted = false;
        for steps in 1..=10_000_000u64 {
            let outcome = cpu.step().expect("trig asset instruction");
            if cpu.physical_memory_mut()[0x1009] as usize > done {
                super::vehicle_core_check::check_final(
                    cpu.physical_memory_mut(),
                    &frames[done]["state"],
                );
                done += 1;
                assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
                if done < count {
                    feed_vehicle(cpu.physical_memory_mut(), &frames[done]["input"]);
                }
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, PASS);
                assert_eq!(done, count);
                used = steps;
                halted = true;
                break;
            }
        }
        assert!(halted);
        short.push(json!({"case":case["name"],"updates":done,"checked_i32_values_per_update":109,"sim_instructions":used,"maximum_integer_error":0,"rng":"unchanged frozen-query boundary"}));
    }
    let mut core_reports = Vec::new();
    for (label, name, index) in [
        ("ordinary-orthogonal", "launch-mixed", 8usize),
        ("normalize-polar", "launch-mixed", 9),
        ("airborne-boost", "airborne-boost", 2),
    ] {
        let case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap();
        let frames = case["frames"].as_array().unwrap();
        let mut m = memory.clone();
        prepare(&mut m, case, &frames[index - 1]["state"], &frames[index]);
        m[COUNT] = 1;
        let mut result = super::vehicle_core_check::run_core(&code, m, &frames[index]);
        result["label"] = json!(label);
        core_reports.push(result);
    }
    Ok(
        json!({"producer_manifest":producer.join("trig-assets.json").to_string_lossy(),"all_entries":all_reports,"out_of_range_rejections":2,"vehicle_short":short,"vehicle_placement":placement,"vehicle_core":core_reports,"interface":"same trig_lookup::index(table,index)->raw fix32; backend selected at compile-time, no runtime callback; local backend views i16 values, asset backend views a low-memory AssetBytes root and calls unchanged SDK gpu_window_asset_read","scope":"complete physical short replay; existing immutable SDK DSEG3 read/save/restore and 54MHz direct-core one-cycle peer; excludes native cache/SDRAM/render timing"}),
    )
}
