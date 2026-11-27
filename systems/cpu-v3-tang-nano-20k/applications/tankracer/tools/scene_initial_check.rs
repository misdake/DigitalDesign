//! Constructor visibility, glow records and vertex flags against original calls.
use super::*;

pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("scene-initial.json"))?)?;
    let code = compile(
        "scene-initial",
        r#"
fn ptr(a:u16)->Array<u16>{unsafe{Ptr::from_addr(a).as_u16_array()}}
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn main(){
 let saved=read_dseg_page(2);let saved3=read_dseg_page(3);
 unsafe{write_dseg_page(2,138);write_dseg_page(3,137);}
 let count=ptr(0x2021)[0u16];let mut i=0u16;
 let mut input=ptr(0x2600);while i<37{input[i]=0;i+=1;}input[0u16]=1;input[1u16]=ptr(0x2020)[0u16];input[2u16]=count;input[3u16]=ptr(0x2006)[0u16];input[33u16]=150;
 copy(at(input,4),ptr(0x2002),4);copy(at(input,8),ptr(0x2100),24);i=0;
 while i<count{
  copy(at(input,37+i*12),ptr(0x2200+i*12),12);i+=1;
 }
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);
 let angle:Buf<u16,4>=Buf::new([45440,5,514,0]);
 let size=scene_initial::random_fields(ptr(0x9400),input,ptr(0x2000),unsafe{sine.as_array().as_ptr().as_i16_array()},unsafe{angle.as_array().as_ptr().as_i16_array()});
 if size!=8+count*22+input[1u16]{halt(0x7e9d);}
 copy(ptr(0x1000),ptr(0x9400),8);let mut glows=ptr(0x1008);i=0;while i<88{glows[i]=0;i+=1;}
 copy(glows,ptr(0x9408),count*22);copy(ptr(0x8000),ptr(0x9408+count*22),input[1u16]);
 let state=flock::State{flight:ptr(0x9000),birds:ptr(0x9040),models:unsafe{Ptr::from_addr(0).as_array::<flock::Model>()},model_count:0,range:fix32::zero()};
 flock::initialize(&state,ptr(0x2400),ptr(0x2022)[0u16] as i16,ptr(0x2000),unsafe{sine.as_array().as_ptr().as_i16_array()});
 emitter::initialize_timer(ptr(0x9300),ptr(0x2000));
 let inverse:Buf<u16,4>=Buf::new([45440,64,8192,0]);
 let camera_ok=scene_initial::camera(ptr(0x1206),ptr(0x2500),ptr(0x2520),ptr(0x2000),unsafe{sine.as_array().as_ptr().as_i16_array()},unsafe{inverse.as_array().as_ptr().as_i16_array()});
 let mut status=ptr(0x123e);status[0u16]=0;if camera_ok{status[0u16]=1;}
 copy(ptr(0x1060),ptr(0x8000),130);copy(ptr(0x10e2),ptr(0x2000),2);copy(ptr(0x10e4),ptr(0x9000),288);copy(ptr(0x1204),ptr(0x9300),2);
 if read_dseg_page(2)!=138 || read_dseg_page(3)!=137{halt(0x7e98);}
 unsafe{write_dseg_page(2,saved);write_dseg_page(3,saved3);}halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let inverse = fs::read(fixture.join("cosine-angle.bin"))?;
    let cases = data["cases"].as_array().unwrap();
    let mut checked = 0usize;
    let mut instructions = 0u64;
    let mut minimum = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (i, pair) in sine.as_chunks::<2>().0.iter().enumerate() {
            memory[45056 * 64 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        for (i, pair) in inverse.as_chunks::<2>().0.iter().enumerate() {
            memory[45440 * 64 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        memory[0x2022] = case["maneuver"].as_i64().unwrap() as u16;
        for (i, value) in case["flock_origin"].as_array().unwrap().iter().enumerate() {
            memory[0x2400 + i * 2..0x2402 + i * 2]
                .copy_from_slice(&words(value.as_i64().unwrap() as i32));
        }
        memory[138 * 16384..139 * 16384].fill(0xa5a5);
        memory[137 * 16384..138 * 16384].fill(0x6c39);
        let remove = case["remove"].as_i64();
        for (i, value) in [
            case["seed"].as_u64().unwrap() as i32,
            case["add"].as_i64().unwrap() as i32,
            remove.unwrap_or(0) as i32,
            i32::from(remove.is_some()),
        ]
        .into_iter()
        .enumerate()
        {
            memory[0x2000 + i * 2..0x2002 + i * 2].copy_from_slice(&words(value));
        }
        memory[0x2020] = case["flag_count"].as_u64().unwrap() as u16;
        let records = case["records"].as_array().unwrap();
        memory[0x2021] = records.len() as u16;
        for (i, value) in case["basis"]
            .as_array()
            .unwrap()
            .iter()
            .chain(case["origin"].as_array().unwrap())
            .enumerate()
        {
            memory[0x2100 + i * 2..0x2102 + i * 2]
                .copy_from_slice(&words(value.as_i64().unwrap() as i32));
        }
        let pose_input = memory[0x2100..0x2118].to_vec();
        for (i, value) in case["camera_basis"]
            .as_array()
            .unwrap()
            .iter()
            .chain(case["origin"].as_array().unwrap())
            .enumerate()
        {
            memory[0x2500 + i * 2..0x2502 + i * 2]
                .copy_from_slice(&words(value.as_i64().unwrap() as i32));
        }
        for (i, value) in case["camera_near"].as_array().unwrap().iter().enumerate() {
            memory[0x2520 + i * 2..0x2522 + i * 2]
                .copy_from_slice(&words(value.as_i64().unwrap() as i32));
        }
        let camera_input = memory[0x2500..0x2526].to_vec();
        memory[0x1206..0x123e].fill(0x5a3d);
        for (i, record) in records.iter().enumerate() {
            for (j, value) in record.as_array().unwrap().iter().enumerate() {
                let at = 0x2200 + i * 12 + j * 2;
                memory[at..at + 2].copy_from_slice(&words(value.as_i64().unwrap() as i32));
            }
        }
        let mut expected = vec![0; 575];
        expected[518..574].fill(0x5a3d);
        if let Some(camera) = case["camera"].as_array() {
            assert_eq!(camera.len(), 28);
            for (i, value) in camera.iter().enumerate() {
                expected[518 + i * 2..520 + i * 2]
                    .copy_from_slice(&words(value.as_i64().unwrap() as i32));
            }
            expected[574] = 1;
        }
        for (i, value) in case["visibility"].as_array().unwrap().iter().enumerate() {
            expected[i * 2..i * 2 + 2].copy_from_slice(&words(value.as_i64().unwrap() as i32));
        }
        for (i, record) in case["glows"].as_array().unwrap().iter().enumerate() {
            for (j, value) in record.as_array().unwrap().iter().enumerate() {
                let at = 8 + i * 22 + j * 2;
                expected[at..at + 2].copy_from_slice(&words(value.as_i64().unwrap() as i32));
            }
        }
        expected[96..226].fill(0xa5a5);
        let flags = case["flags"].as_array().unwrap();
        for (i, value) in flags.iter().enumerate() {
            expected[96 + i] = value.as_u64().unwrap() as u16;
        }
        expected[226..228].copy_from_slice(&words(case["final_seed"].as_u64().unwrap() as i32));
        let flock = case["flock"].as_array().unwrap();
        assert_eq!(flock.len(), 288);
        for (i, value) in flock.iter().enumerate() {
            expected[228 + i] = value.as_u64().unwrap() as u16;
        }
        expected[516..518].copy_from_slice(&words(case["emitter_timer"].as_i64().unwrap() as i32));
        let mut passed = false;
        let mut started = false;
        for step in 0..2_000_000u64 {
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum = minimum.min(sp);
                assert!(sp >= 0x7000);
            }
            if let StepOutcome::Halted { signal } = cpu.step().map_err(|e| format!("{e:?}"))? {
                assert_eq!(signal, PASS, "scene initial case {index}");
                instructions += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "scene initial case {index}: instruction bound");
        let memory = cpu.physical_memory_mut();
        for (i, &want) in expected.iter().enumerate() {
            assert_eq!(
                memory[0x1000 + i],
                want,
                "scene initial case {index} word {i}"
            );
        }
        assert_eq!(&memory[0x2100..0x2118], &pose_input);
        assert_eq!(&memory[0x2500..0x2526], &camera_input);
        assert!(
            memory[138 * 16384 + flags.len()..138 * 16384 + 4096]
                .iter()
                .all(|&v| v == 0xa5a5)
        );
        assert!(
            memory[137 * 16384..138 * 16384]
                .iter()
                .all(|&v| v == 0x6c39)
        );
        assert!(
            memory[138 * 16384 + 4096 + 288..138 * 16384 + 4864]
                .iter()
                .all(|&v| v == 0xa5a5)
        );
        assert!(
            memory[138 * 16384 + 4866..138 * 16384 + 5120]
                .iter()
                .all(|&v| v == 0xa5a5)
        );
        let generated = 8 + records.len() * 22 + flags.len();
        assert!(
            memory[138 * 16384 + 5120 + generated..139 * 16384]
                .iter()
                .all(|&v| v == 0xa5a5)
        );
        checked += expected.len();
    }
    let world = run_world(output, fixture)?;
    let report = json!({"cases":cases.len(),"checked_u16_values":checked,
        "maximum_integer_error":0,"code_bytes":code.total_code_words()*2,
        "minimum_stack_word":minimum,"maximum_stack_bytes":(32768-usize::from(minimum))*2,
        "instruction_steps":instructions,"scope":data["scope"],
        "backend":"instruction, bounded flag bank, immutable transforms and shared RNG", "world":world});
    fs::write(
        output.join("scene-initial-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn run_world(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("scene-initial-world.json"))?)?;
    let code = compile(
        "scene-initial-world",
        r#"
fn ptr(a:u16)->Array<u16>{unsafe{Ptr::from_addr(a).as_u16_array()}}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn main(){
 let saved=read_dseg_page(2);let saved3=read_dseg_page(3);
 let cells=static_assets::AssetBytes{first_sector:35072,sector_count:256,byte_len_lo:32768,byte_len_hi:0};
 let world=ground::World::begin(cells,attitude::get(ptr(0x2000),0),ptr(0x2100));
 copy(ptr(0x1000),world.noise.as_array(),16);attitude::put(ptr(0x2000),0,world.seed);
 vehicle_shield::initialize(ptr(0x1010),ptr(0x2000));copy(ptr(0x133a),ptr(0x2000),2);
 unsafe{write_dseg_page(2,138);write_dseg_page(3,139);}
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);
 let count=ptr(0x2002)[0u16];let mut i=0u16;let mut source=0u16;let mut dest=0u16;
 let angle:Buf<u16,4>=Buf::new([45440,5,514,0]);
 while i<count{
  if source>16346 || dest>16083{halt(0x7e9a);}
  let row=ptr(0x8000+source);let input_words=row[0u16];let output_words=row[1u16];
  if input_words<37 || input_words>16382-source || output_words>16380-dest{halt(0x7e9a);}
  copy(ptr(0xc000+dest),ptr(0x2000),2);
  let produced=scene_initial::random_fields(ptr(0xc004+dest),ptr(0x8002+source),ptr(0x2000),unsafe{sine.as_array().as_ptr().as_i16_array()},unsafe{angle.as_array().as_ptr().as_i16_array()});
  if produced!=output_words || read_dseg_page(2)!=138 || read_dseg_page(3)!=139{halt(0x7e9b);}
  copy(ptr(0xc002+dest),ptr(0x2000),2);
  source+=input_words+2;dest+=output_words+4;i+=1;
 }
 unsafe{write_dseg_page(2,saved);write_dseg_page(3,saved3);}halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let angle = fs::read(fixture.join("angle.i16.bin"))?;
    let mut checked = 0usize;
    let mut updates = 0usize;
    let mut instructions = 0u64;
    let mut minimum = u16::MAX;
    for (case_index, case) in data["cases"].as_array().unwrap().iter().enumerate() {
        let mut input = Vec::new();
        let mut expected = Vec::new();
        let mut owners = Vec::new();
        let records = case["records"].as_array().unwrap();
        for (index, row) in records.iter().enumerate() {
            let values: Vec<u16> = row["input"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u16)
                .collect();
            assert_eq!(
                values.len(),
                37 + usize::from(values[2]) * 12
                    + if values[36] != 0 {
                        usize::from(values[34]) * 6
                    } else {
                        0
                    }
            );
            let mut product_words = match values[0] {
                0 => 8,
                1 => 8 + usize::from(values[2]) * 22 + usize::from(values[1]),
                2 => 296,
                3 => 10,
                4 | 5 => 0,
                _ => panic!("constructor kind"),
            };
            if values[0] != 4 && values[0] != 5 {
                product_words += usize::from(values[34]) * 48;
            }
            assert_eq!(row["expected"].as_array().unwrap().len(), product_words);
            input.extend([values.len() as u16, product_words as u16]);
            input.extend(values);
            expected.extend(words(row["random_before"].as_u64().unwrap() as i32));
            expected.extend(words(row["random_after"].as_u64().unwrap() as i32));
            expected.extend(
                row["expected"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u16),
            );
            owners.resize(expected.len(), index);
        }
        assert!(input.len() < 16384 && expected.len() < 16384);
        let mut globals: Vec<u16> = case["noise"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u16)
            .collect();
        globals.extend(
            case["shield"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u16),
        );
        globals.extend(words(case["globals_seed"].as_u64().unwrap() as i32));
        assert_eq!(globals.len(), 828);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        memory[0x2000..0x2002].copy_from_slice(&words(case["seed"].as_u64().unwrap() as i32));
        memory[0x2002] = records.len() as u16;
        memory[137 * 16384..138 * 16384].fill(0x6c39);
        memory[137 * 16384..137 * 16384 + 9].copy_from_slice(&[0x5443, 1, 0, 9, 0, 0, 0, 0, 0]);
        let terrain = memory[137 * 16384..138 * 16384].to_vec();
        memory[138 * 16384..139 * 16384].fill(0xa53a);
        memory[138 * 16384..138 * 16384 + input.len()].copy_from_slice(&input);
        memory[139 * 16384..140 * 16384].fill(0x5a3d);
        for (i, pair) in sine.as_chunks::<2>().0.iter().enumerate() {
            memory[45056 * 64 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        for (i, pair) in angle.as_chunks::<2>().0.iter().enumerate() {
            memory[45440 * 64 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        let mut passed = false;
        let mut started = false;
        for step in 0..10_000_000u64 {
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum = minimum.min(sp);
                assert!(sp >= 0x7000);
            }
            if let StepOutcome::Halted { signal } = cpu.step().map_err(|e| format!("{e:?}"))? {
                assert_eq!(signal, PASS, "world constructor case {case_index}");
                instructions += step;
                passed = true;
                break;
            }
        }
        assert!(
            passed,
            "world constructor case {case_index}: instruction bound"
        );
        let memory = cpu.physical_memory_mut();
        assert_eq!(
            &memory[0x1000..0x133c],
            globals.as_slice(),
            "world globals {case_index}"
        );
        for (word, &value) in expected.iter().enumerate() {
            let row = &records[owners[word]];
            assert_eq!(
                memory[139 * 16384 + word],
                value,
                "world {case_index} object {} {} word {word}",
                row["class"],
                row["name"]
            );
        }
        assert_eq!(
            &memory[0x2000..0x2002],
            &words(case["final_seed"].as_u64().unwrap() as i32)
        );
        assert_eq!(&memory[137 * 16384..138 * 16384], terrain.as_slice());
        assert_eq!(
            &memory[138 * 16384..138 * 16384 + input.len()],
            input.as_slice()
        );
        assert!(
            memory[138 * 16384 + input.len()..139 * 16384]
                .iter()
                .all(|&v| v == 0xa53a)
        );
        assert!(
            memory[139 * 16384 + expected.len()..140 * 16384]
                .iter()
                .all(|&v| v == 0x5a3d)
        );
        checked += globals.len() + expected.len() + 2;
        updates += records.len();
    }
    Ok(
        json!({"cases":data["cases"].as_array().unwrap().len(),"constructed_objects":updates,"checked_u16_values":checked,
        "maximum_integer_error":0,"code_bytes":code.total_code_words()*2,"maximum_stack_bytes":(32768-usize::from(minimum))*2,
        "instruction_steps":instructions,"scope":data["scope"]}),
    )
}
