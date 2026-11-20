//! XInput geometry through actual RCC Device5, stick, UI, gamepad and camera code.
use crate::shared_build_support_rcc_modules as resolver;
use cpu_v3::{rcc_backend, CpuV3Sim, Device, RunOutcome};
use cpu_v3_tang_nano_20k::{
    host_io::{map_xinput, XInputSample},
    input_device::InputSnapshotDevice,
};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};

struct Input(Rc<RefCell<InputSnapshotDevice>>);
impl Device for Input {
    fn read(&mut self, memory: &mut [u16], channel: u8) -> u16 {
        self.0.borrow_mut().read(memory, channel)
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        self.0.borrow_mut().write(memory, channel, value);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[derive(Default)]
struct Records {
    words: Vec<u16>,
    rows: Vec<Vec<u16>>,
}
struct Events {
    input: Rc<RefCell<InputSnapshotDevice>>,
    cases: Vec<Option<XInputSample>>,
    records: Rc<RefCell<Records>>,
}
impl Device for Events {
    fn read(&mut self, _: &mut [u16], _: u8) -> u16 {
        0
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        if channel == 0 {
            self.input
                .borrow()
                .publish(self.cases[usize::from(value)].map(map_xinput));
        } else {
            let mut s = self.records.borrow_mut();
            if channel == 1 {
                s.words.push(value);
            } else {
                let row = std::mem::take(&mut s.words);
                s.rows.push(row);
            }
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn fixed(row: &[u16], at: usize) -> f64 {
    f64::from((u32::from(row[at]) | (u32::from(row[at + 1]) << 16)) as i32) / 65536.0
}

#[test]
fn cardinal_quantization_is_unchanged_for_every_signed16_value() {
    for value in i16::MIN..=i16::MAX {
        let s = map_xinput(XInputSample {
            left: [value, 0],
            right: [0, value],
            ..XInputSample::default()
        });
        assert_eq!(s.left, [((32768 + i32::from(value)) >> 8) as u8, 128]);
        assert_eq!(
            s.right,
            [128, ((32768 - i32::from(value)).clamp(0, 65535) >> 8) as u8]
        );
    }
    for (input, expected) in [
        ([23170, 23170], [255, 0]),
        ([-23170, -23170], [0, 255]),
        ([i16::MAX, i16::MAX], [255, 0]),
        ([i16::MIN, i16::MIN], [0, 255]),
    ] {
        assert_eq!(
            map_xinput(XInputSample {
                left: input,
                ..XInputSample::default()
            })
            .left,
            expected
        );
    }
}

#[test]
fn circular_grid_roundtrips_through_real_rcc_and_application_consumers() {
    let mut points = vec![
        [0, 0],
        [i16::MAX, 0],
        [i16::MIN, 0],
        [0, i16::MAX],
        [0, i16::MIN],
        [23170, 23170],
        [-23170, 23170],
        [11585, -11585],
        [2000, 0],
        [4096, 4096],
        [i16::MAX, i16::MAX],
    ];
    // Index11 is disconnected, and is also checked by all three real consumers.
    let mut cases: Vec<_> = points
        .iter()
        .map(|&[x, y]| {
            Some(XInputSample {
                buttons: 0x1000,
                left: [x, y],
                right: [y, x],
                ..XInputSample::default()
            })
        })
        .collect();
    cases.push(None);
    for x in -5_i32..=5 {
        for y in -5_i32..=5 {
            if x * x + y * y <= 25 {
                points.push([(x * 6500) as i16, (y * 6500) as i16]);
            }
        }
    }
    for radius in [3071.0, 3072.0, 3073.0, 16000.0, 32767.0] {
        for angle in 0..16 {
            let a = f64::from(angle) * std::f64::consts::TAU / 16.0;
            points.push([
                (radius * a.cos()).round() as i16,
                (radius * a.sin()).round() as i16,
            ]);
        }
    }
    cases.extend(points.iter().skip(11).map(|&[x, y]| {
        Some(XInputSample {
            left: [x, y],
            right: [y, x],
            ..XInputSample::default()
        })
    }));
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scene: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/demo_box_rabbit/assets/scene.json")).unwrap();
    let mut matrix = Vec::new();
    for v in scene["cameras"][0]["matrix"].as_array().unwrap() {
        let n = v.as_i64().unwrap() as u32;
        matrix.extend([n as u16, (n >> 16) as u16]);
    }
    matrix.extend(
        scene["cameras"][0]["light"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as u16),
    );
    let source = format!(
        r#"
mod dualshock2;mod stick;mod stick_controls;mod gamepad;mod camera;
static S:Buf<u16,10>=Buf::new([0;10]);static RX:Buf<u16,9>=Buf::new([0;9]);
static C:Buf<u16,12>=Buf::new([0;12]);static M:Buf<u16,45>=Buf::new({matrix:?});
fn check(ok:bool){{if !ok{{halt(0xbad);}}}}
fn emit(v:fix32){{dev_send(7,1,v.lo_bits());dev_send(7,1,v.hi_bits());}}
fn main(){{
 ds2_init(S.as_array());dui_controls_reset();gamepad_init();
 let mut config=stick_default();let mut p=stick_prepare(view_of(&mut config));
 let mut i:u16=0;while i<{count}{{
  dev_send(7,0,i);ds2_capture(S.as_array(),RX.as_array());
  let l=stick_read(view_of(&mut p),S[5u16],S[6u16],S[0u16]!=0);
  let r=stick_read(view_of(&mut p),S[3u16],S[4u16],S[0u16]!=0);
  emit(l.cal_x);emit(l.cal_y);emit(l.x);emit(l.y);
  emit(r.cal_x);emit(r.cal_y);emit(r.x);emit(r.y);
  dev_send(7,1,S[0u16]);dev_send(7,1,S[2u16]);
  if i<12{{
   let ul=dui_left(S.as_array());let ur=dui_right(S.as_array());
   check(ul.x==l.x&&ul.y==l.y&&ur.x==r.x&&ur.y==r.y);
   dev_send(7,0,i);gamepad_capture();let frame=gamepad_take_frame();
   check(frame.left_x==l.x&&frame.left_y==l.y&&frame.right_x==r.x&&frame.right_y==r.y);
   check(frame.connected==S[0u16]&&frame.buttons==S[2u16]);
   demo_camera_reset(C.as_array(),0);
   check(demo_camera_update(M.as_array(),C.as_array(),S.as_array(),100));
   let mut k:u16=0;while k<5{{emit(gpu_fload(C.as_array(),k));k+=1;}}
  }}
  dev_send(7,2,0);i+=1;
 }}halt(0x600d);
}}"#,
        count = cases.len()
    );
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = rcc::frontend::compile_program_named(
        "XInput inverse consumers",
        &source,
        &options,
        &mut |name| {
            if name == "camera" {
                std::fs::read_to_string(root.join("examples/demo_box_rabbit/rcc/camera.rs"))
                    .map_err(|e| e.to_string())
            } else {
                resolver::load_module(
                    &root.join("tests/fixtures/dualshock2_ui/rcc"),
                    &root.join("rcc"),
                    "pub const DEMO_CAMERA_FIRST:u16=0;",
                    name,
                    &mut |_| {},
                )
            }
        },
    )
    .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let input = Rc::new(RefCell::new(InputSnapshotDevice::default()));
    let records = Rc::new(RefCell::new(Records::default()));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    cpu.attach_device(5, Box::new(Input(input.clone())));
    cpu.attach_device(
        7,
        Box::new(Events {
            input,
            cases: cases.clone(),
            records: records.clone(),
        }),
    );
    let outcome = cpu.run(20_000_000).unwrap();
    assert!(
        matches!(outcome, RunOutcome::Halted { signal: 0x600d, .. }),
        "{outcome:?}"
    );
    let records = records.borrow();
    assert_eq!(records.rows.len(), cases.len());
    let mut max_roundtrip = 0.0_f64;
    let inner = 24.0 / 256.0;
    // A byte contributes at most1/127 calibrated-component error. The shared
    // square-ray/dead-zone map has infinity-norm gain <=2/(outer-inner).
    // Keep the existing RCC numerical allowance2/256 as a separate budget.
    let quantization_budget = 2.0 / (127.0 * (1.0 - inner));
    for (index, (row, sample)) in records.rows.iter().zip(&cases).enumerate() {
        assert_eq!(row.len(), if index < 12 { 28 } else { 18 });
        if index == 0 {
            assert!(row[..16].iter().all(|v| *v == 0), "center must be exact");
        }
        if index == 8 {
            assert!(
                row[4..8].iter().chain(&row[12..16]).all(|v| *v == 0),
                "inverse geometry must not expand a suppressed input out of the dead zone"
            );
        }
        assert_eq!(row[16], u16::from(sample.is_some()));
        assert_eq!(
            row[17],
            sample.map_or(0, |s| !map_xinput(s).buttons_active_low)
        );
        for (side, offset) in [(true, 0), (false, 8)] {
            let Some(sample) = sample else {
                assert!((0..8).all(|j| row[offset + j] == 0));
                continue;
            };
            let sdk = if side { sample.left } else { sample.right };
            let snap = map_xinput(*sample);
            let raw = if side { snap.left } else { snap.right };
            let axis = |v: u8| {
                let d = f64::from(v) - 128.0;
                d / if d < 0.0 { 128.0 } else { 127.0 }
            };
            let cal = [axis(raw[0]), axis(raw[1])];
            let length = cal[0].hypot(cal[1]);
            let strength =
                ((cal[0].abs().max(cal[1].abs()) - inner) / (1.0 - inner)).clamp(0.0, 1.0);
            for (k, &component) in cal.iter().enumerate() {
                assert!((fixed(row, offset + 2 * k) - component).abs() <= 2.0 / 256.0);
                let expected = if length == 0.0 {
                    0.0
                } else {
                    component / length * strength
                };
                assert!(
                    (fixed(row, offset + 4 + 2 * k) - expected).abs() <= 2.0 / 256.0,
                    "RCC quantized reference index{index}"
                );
            }
            let unit = [f64::from(sdk[0]) / 32768.0, -f64::from(sdk[1]) / 32768.0];
            let radius = unit[0].hypot(unit[1]);
            let strength = ((radius.min(1.0) - inner) / (1.0 - inner)).clamp(0.0, 1.0);
            for (k, &component) in unit.iter().enumerate() {
                let expected = if radius == 0.0 {
                    0.0
                } else {
                    component / radius * strength
                };
                let error = (fixed(row, offset + 4 + 2 * k) - expected).abs();
                max_roundtrip = max_roundtrip.max(error);
                assert!(
                    error <= quantization_budget + 2.0 / 256.0,
                    "SDK roundtrip index{index}, side{side}, component{k}: error{error}"
                );
            }
        }
        if index < 12 {
            let lx = fixed(row, 4);
            let ly = fixed(row, 6);
            let rx = fixed(row, 12);
            let ry = fixed(row, 14);
            let yaw = rx * 0.3;
            let pitch = -5604.0 / 65536.0 - ry * 0.3;
            for (field, expected) in [
                (0, -yaw.cos() * lx * 0.2 + yaw.sin() * ly * 0.2),
                (1, 14418.0 / 65536.0),
                (
                    2,
                    2.0 + 52429.0 / 65536.0 + yaw.sin() * lx * 0.2 + yaw.cos() * ly * 0.2,
                ),
                (3, yaw),
                (4, pitch),
            ] {
                assert!(
                    (fixed(row, 18 + 2 * field) - expected).abs() < 0.0005,
                    "camera index{index}, field{field}: got{}, expected{expected}",
                    fixed(row, 18 + 2 * field)
                );
            }
        }
    }
    println!("actual RCC: {} snapshots, two sticks; UI/gamepad/camera12 cases; max SDK roundtrip component error={max_roundtrip:.9}, quantization budget={quantization_budget:.9}, RCC budget={:.9}",cases.len(),2.0/256.0);
}
