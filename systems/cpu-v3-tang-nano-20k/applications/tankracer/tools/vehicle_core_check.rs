//! Selected one-tick cycle captures; qualified kernel bytes and assembly stay fixed.
use super::*;
use cpu_v3::{CpuV3CoreOutput, CpuV3CoreState};
use digital_design_circuit::{external, CircuitWires, External};
use std::{cell::RefCell, rc::Rc};
struct CoreProbe {
    state: Rc<RefCell<CpuV3CoreState>>,
    input: CpuV3CoreInput,
    output: CpuV3CoreOutput,
}
impl External for CoreProbe {
    fn execute(&mut self, c: &mut CircuitWires) {
        CpuV3Core::execute_emu(&mut self.state.borrow_mut(), c, &self.input, &self.output);
    }
    fn clock(&mut self, c: &mut CircuitWires) {
        CpuV3Core::clock_emu(&mut self.state.borrow_mut(), c, &self.input, &self.output);
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
struct Region {
    name: &'static str,
    first: usize,
    end: usize,
    reads: u64,
    writes: u64,
    touched: Vec<bool>,
}
fn regions() -> Vec<Region> {
    [
        ("events", 0x1000, 0x100a),
        ("attitude", 0x1100, 0x113a),
        ("suspension", 0x1180, 0x11d8),
        ("drive", 0x1300, 0x1320),
        ("motion", 0x1380, 0x13a0),
        ("boost", 0x1400, 0x1408),
        ("suspension_config", 0x1500, 0x1534),
        ("drive_config_and_surfaces", 0x1600, 0x1808),
        ("attitude_config", 0x1900, 0x1905),
        ("pose_config", 0x1920, 0x1924),
        ("count", 0x1ffe, 0x1fff),
        ("suspension_input", 0x2000, 0x2024),
        ("drive_input", 0x2100, 0x213a),
        ("sine", 0x3000, 0x8000),
        ("angle", 0x8000, 0x8101),
        ("stack", 0xa000, 0xc000),
    ]
    .into_iter()
    .map(|(name, first, end)| Region {
        name,
        first,
        end,
        reads: 0,
        writes: 0,
        touched: vec![false; end - first],
    })
    .collect()
}
pub(super) fn check_final(memory: &[u16], expected: &Value) {
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
            "core final {name}"
        );
    }
}
pub(super) fn run_core(code: &CpuV3Program, mut memory: Vec<u16>, expected: &Value) -> Value {
    let state = Rc::new(RefCell::new(CpuV3CoreState::default()));
    let (mut circuit, (input, output)) = build_circuit(|| {
        let input = CpuV3CoreInput::allocate();
        let output = CpuV3CoreOutput::allocate();
        external(CoreProbe {
            state: state.clone(),
            input: input.clone(),
            output: output.clone(),
        });
        (input, output)
    });
    let mut instruction = None;
    let mut data = None;
    let mut first_fetch = None;
    let mut marks = [None; 4];
    let mut instruction_requests = 0u64;
    let fast_entry = polar_symbol(code, "polar_fpu");
    let wide_entry = polar_symbol(code, "polar_wide");
    let mut fpu_attempts = 0u64;
    let mut wide_fallbacks = 0u64;
    let mut reads = 0u64;
    let mut writes = 0u64;
    let mut regions = regions();
    let mut other_reads = 0u64;
    let mut other_writes = 0u64;
    let mut minimum_sp = None;
    let mut stages_checked = 0;
    for tick in 0..2_000_000u32 {
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
        let sp = state.borrow().architectural_gprs()[13];
        if sp != 0 {
            assert!(
                (0xa000..=0xc000).contains(&sp),
                "SP outside qualified stack reserve: {sp:04x}"
            );
            minimum_sp = Some(minimum_sp.map_or(sp, |old: u16| old.min(sp)));
        }
        assert!(
            !value.fault,
            "core fault {} at {}",
            value.fault_code, value.fault_pc
        );
        if value.instruction_request_valid {
            instruction_requests += 1;
            fpu_attempts += u64::from(Some(value.instruction_address as u16) == fast_entry);
            wide_fallbacks += u64::from(Some(value.instruction_address as u16) == wide_entry);
            first_fetch.get_or_insert(tick);
        }
        if value.halted {
            assert_eq!(value.halt_signal as u16, PASS);
            assert_eq!(memory[0x1009], 1);
            check_final(&memory, &expected["state"]);
            assert_eq!(stages_checked, 2);
            let marks = marks.map(|v| v.expect("missing stage marker"));
            let total = tick - first_fetch.unwrap() + 1;
            let full = marks[3] - marks[0];
            let sp = minimum_sp.unwrap();
            let regions:Vec<Value>=regions.into_iter().map(|r|json!({"region":r.name,"first_word":r.first,"end_word_exclusive":r.end,"read_requests":r.reads,"write_requests":r.writes,"unique_words_touched":r.touched.iter().filter(|v|**v).count()})).collect();
            return json!({"cycles":total,"full_tick_cycles":full,"outside_tick_harness_cycles":total-full,"stage_cycles":{"boost_and_suspension":marks[1]-marks[0],"powertrain":marks[2]-marks[1],"integration_attitude_pose_finish":marks[3]-marks[2]},"clock_hz":54000000,"full_tick_ms_at_54mhz":f64::from(full)/54000.0,"retired_words":value.retired_words,"instruction_requests":instruction_requests,"data_read_requests":reads,"data_write_requests":writes,"unclassified_reads":other_reads,"unclassified_writes":other_writes,"stack":{"initial_top_exclusive_word":49152,"minimum_sp_word":sp,"peak_reserved_bytes":(49152u32-u32::from(sp))*2,"measured_from":"architectural GPR13 every core cycle"},"regions":regions,"checked_i32_values":109,"fpu_attempts":fpu_attempts,"wide_fallbacks":wide_fallbacks,"intermediate_stage_checks":stages_checked,"maximum_integer_error":0,"event_differences":0,"marker_ticks":marks});
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
            let at = value.data_address as usize;
            if value.data_write {
                writes += 1;
            } else {
                reads += 1;
            }
            if let Some(region) = regions.iter_mut().find(|r| at >= r.first && at < r.end) {
                region.touched[at - region.first] = true;
                if value.data_write {
                    region.writes += 1;
                } else {
                    region.reads += 1;
                }
            } else {
                if value.data_write {
                    other_writes += 1;
                } else {
                    other_reads += 1;
                }
            }
            if value.data_write {
                let written = value.data_write_data as u16;
                if at == 0x1008 && written <= 2 {
                    marks[written as usize] = Some(tick);
                    if written == 1 {
                        assert_eq!(read_i32(&memory, 0x1180, 44), expected["after_suspension"]);
                        assert_eq!(
                            json!(&memory[0x1000..0x1003]),
                            expected["suspension_events"]
                        );
                        stages_checked += 1;
                    }
                    if written == 2 {
                        assert_eq!(
                            read_i32(&memory, 0x1300, 16),
                            expected["after_powertrain"]["drive"]
                        );
                        assert_eq!(
                            read_i32(&memory, 0x1380, 3),
                            expected["after_powertrain"]["velocity"]
                        );
                        assert_eq!(
                            read_i32(&memory, 0x112a, 3),
                            expected["after_powertrain"]["angular"]
                        );
                        stages_checked += 1;
                    }
                }
                if at == 0x1009 {
                    marks[3] = Some(tick);
                }
                memory[at] = written;
                0
            } else {
                u64::from(memory[at])
            }
        });
        circuit.clock_tick();
    }
    panic!("full vehicle core exceeded 2,000,000 cycles");
}
fn run_variant(
    output: &Path,
    fixture: &Path,
    qualified: &Path,
    candidate: bool,
) -> Result<Value, Box<dyn Error>> {
    // Bind this capture to the exact source snapshots and listing already used by 600-step qualification.
    for (name, source) in [
        ("integer.rs", INTEGER),
        ("attitude.rs", ATTITUDE),
        ("suspension.rs", SUSPENSION),
        ("drive.rs", DRIVE),
        ("vehicle.rs", VEHICLE),
    ] {
        if !candidate || name != "attitude.rs" {
            assert_eq!(
                fs::read_to_string(qualified.join(name))?,
                source,
                "qualified source changed: {name}"
            );
        }
        fs::write(output.join(name), source)?;
    }
    let source = fs::read_to_string(qualified.join("vehicle/input.rs"))?;
    let body = &source[source.find("fn main()").ok_or("qualified main missing")?..];
    let code = compile("vehicle-core", body, output)?;
    let assembly_identical =
        code.listing == fs::read_to_string(qualified.join("vehicle/program.asm"))?;
    if !candidate {
        assert!(
            assembly_identical,
            "core capture must use exact qualified assembly"
        );
        assert_eq!(code.total_code_words() * 2, 28114);
    }
    fs::write(
        output.join("program.bin"),
        code.words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
    )?;
    let fixture_data: Value = serde_json::from_slice(&fs::read(fixture.join("vehicle.json"))?)?;
    let sine = fs::read(fixture.join("sine.bin"))?;
    let angle = fs::read(fixture.join("angle.bin"))?;
    let mut reports = Vec::new();
    for (label, name, index) in [
        ("ordinary-orthogonal", "launch-mixed", 8usize),
        ("normalize-polar", "launch-mixed", 9),
        ("airborne-boost", "airborne-boost", 2),
    ] {
        let case = fixture_data["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap();
        let frames = case["frames"].as_array().unwrap();
        let previous = if index == 0 {
            &case["initial"]
        } else {
            &frames[index - 1]["state"]
        };
        let expected = &frames[index];
        let mut memory = vec![0u16; 65536];
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
            write_i32(&mut memory, address, &previous[name]);
        }
        for (name, address) in [
            ("suspension_config", 0x1500),
            ("drive_config", 0x1600),
            ("pose_config", 0x1920),
        ] {
            write_i32(&mut memory, address, &case[name]);
        }
        for (i, v) in case["attitude_config"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            memory[0x1900 + i] = v.as_u64().unwrap() as u16;
        }
        memory[COUNT] = 1;
        feed_vehicle(&mut memory, &expected["input"]);
        let mut result = run_core(&code, memory, expected);
        result["label"] = json!(label);
        result["qualified_case"] = json!(name);
        result["frame"] = json!(index + 1);
        result["dt_ms"] = expected["input"]["frame"][0].clone();
        result["numerics"] = json!("orthogonal-v1");
        result["all_four_airborne"] = json!(expected["input"]["frame"][15] == 4);
        reports.push(result);
    }
    let c = &fixture_data["cases"][0];
    let state_bytes = 109 * 4;
    let sc = c["suspension_config"].as_array().unwrap().len() * 4;
    let dc = c["drive_config"].as_array().unwrap().len() * 4;
    let ac = c["attitude_config"].as_array().unwrap().len() * 2;
    let pc = c["pose_config"].as_array().unwrap().len() * 4;
    let config_bytes = sc + dc + ac + pc;
    Ok(
        json!({"scope":"CpuV3Core cycle model with one-cycle instruction/data responses, request ready always true; same qualified complete physical tick harness; excludes fetch queue, native I/D caches, arbitration, SDRAM, GPU and display contention","clock_hz":54000000,"code_bytes":code.total_code_words()*2,"qualified_assembly_identical":assembly_identical,"candidate_attitude":candidate,"marker_scope":"full_tick_cycles span the pre-boost stage marker store request through the completed-update marker store request; phase spans include call/return, inter-stage setup and marker stores; cycles include complete one-update harness and HALT","memory":{"state_bytes_per_vehicle_as_tested":state_bytes,"config_bytes_as_tested":config_bytes,"config_components":{"suspension":sc,"drive_and_shared_surfaces":dc,"attitude":ac,"pose":pc},"shared_surfaces_bytes":180*4,"config_bytes_excluding_shared_surfaces":config_bytes-180*4,"frame_input_bytes":(18+29)*4,"diagnostic_and_count_bytes":22,"sine_bytes":sine.len(),"sine_first_word":12288,"sine_end_word_exclusive":32768,"angle_bytes":angle.len(),"angle_first_word":32768,"angle_end_word_exclusive":33025,"stack_reserved_first_word":40960,"stack_reserved_end_word_exclusive":49152,"stack_reserved_bytes":16384,"addresses_are":"16-bit CPU words; multiply by two for byte addresses"},"cases":reports}),
    )
}

pub(super) fn run(
    output: &Path,
    fixture: &Path,
    qualified: &Path,
) -> Result<Value, Box<dyn Error>> {
    run_variant(output, fixture, qualified, false)
}
pub(super) fn run_candidate(
    output: &Path,
    fixture: &Path,
    qualified: &Path,
) -> Result<Value, Box<dyn Error>> {
    run_variant(output, fixture, qualified, true)
}
