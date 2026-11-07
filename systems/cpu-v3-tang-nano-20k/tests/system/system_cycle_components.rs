//! Root-only clocks, physical slots and shared memory; no functional devices.
use cpu_v3_tang_nano_20k::system_cycle_emu::{
    clock::Clock,
    memory::{CacheRequest, Clients, SharedMemory},
    request_slot::Slot,
};
use gpu_v2::system::vliw_cycle::Request;
#[test]
fn exact_calendar_keeps_ratio_coincident_edges_and_native_phase() {
    for phase in [0, 31] {
        let mut c = Clock::new(
            cpu_v3_tang_nano_20k::display::ACTIVE_DISPLAY_CONFIG.pixel_clock_hz,
            phase,
        );
        let mut last = 0;
        for _ in 0..100000 {
            let e = c.peek();
            assert!(e.ticks >= last);
            last = e.ticks;
            if e.logic_rise {
                assert_eq!(e.ticks * 54_000_000, c.logic_edges * c.quantum_hz);
                assert!(e.native_half && e.native_phase == 0);
            }
            if e.native_half {
                assert_eq!(e.ticks * 216_000_000, c.native_halves * c.quantum_hz);
            }
            if e.pixel_rise {
                assert_eq!((e.ticks - phase) * 33_300_000, c.pixel_edges * c.quantum_hz);
            }
            assert_eq!(c.advance(), e);
        }
    }
}
fn slot_rows() -> Vec<(bool, Request, bool, bool, Slot)> {
    let mut slot = Slot::default();
    let mut rows = Vec::new();
    for n in 0..80 {
        let reset = n == 0 || n == 47;
        let input = Request {
            valid: n % 5 != 2,
            address_halfword: 0x400000 + n * 64,
            count: (n % 4) as u8,
        };
        let writing = n % 3 != 1;
        let ready = n % 7 > 2;
        rows.push((reset, input, writing, ready, slot));
        slot.clock(reset, input, writing, ready);
        if reset {
            assert!(!slot.request.valid);
        }
    }
    rows
}
#[test]
fn board_slot_never_borrows_same_edge_credit() {
    let rows = slot_rows();
    assert!(rows.iter().any(|(reset, input, _, ready, s)| !reset
        && input.valid
        && s.request.valid
        && *ready
        && !s.ready(*reset)));
}
fn qword(b: &[u16], a: usize) -> u64 {
    (0..4).fold(0, |v, n| v | u64::from(b[a + n]) << (n * 16))
}
#[test]
fn one_native_service_preserves_three_port_ownership_and_physical_write_visibility() {
    let initial: Vec<u16> = (0..1 << 22)
        .map(|n| (n as u16).wrapping_mul(739).wrapping_add(0x5931))
        .collect();
    let gpu_expected: Vec<_> = (0..16).map(|n| qword(&initial, 0x2000 + n * 4)).collect();
    let display_expected: Vec<_> = (0..4).map(|n| qword(&initial, 0x3000 + n * 4)).collect();
    let writes = [
        0x1873_9a50_b42f_6c01,
        0xfedc_ba98_7654_3210,
        0x0123_4567_89ab_cdef,
        0x7e91_2553_a7b0_d6c8,
    ];
    let old = initial[0x1000..0x1010].to_vec();
    let mut m = SharedMemory::new(initial);
    let (mut started, mut data_offer, mut gpu_offer, mut display_offer) =
        (false, false, false, false);
    let (mut w, mut g, mut d, mut acknowledged) = (0, 0, 0, false);
    let mut simultaneous = false;
    let mut saw_partial = false;
    for wall in 0..60000 {
        if !started && m.initialized() {
            started = true;
            data_offer = true;
            gpu_offer = true;
            display_offer = true;
        }
        let c = Clients {
            data: CacheRequest {
                valid: data_offer,
                writing: true,
                line: true,
                address: 0x1000,
                write_data: writes[w.min(3)],
                response_ready: true,
            },
            instruction: CacheRequest {
                response_ready: true,
                ..Default::default()
            },
            gpu: [
                Slot {
                    request: Request {
                        valid: gpu_offer,
                        address_halfword: 0x2000,
                        count: 3,
                    },
                    writing: false,
                },
                Slot::default(),
                Slot::default(),
            ],
            display_valid: display_offer,
            display_address: 0x3000,
            ..Default::default()
        };
        simultaneous |= c.data.valid && c.gpu[0].request.valid && c.display_valid;
        let s = m.sample(c, wall < 2);
        let f = s.feedback;
        let again = m.sample(c, wall < 2);
        assert_eq!(f, again.feedback);
        let native_before = m.native_cycles();
        if data_offer && f.data.request_ready {
            assert_eq!(&m.backing()[0x1000..0x1010], old);
            data_offer = false;
        }
        if gpu_offer && f.gpu[0].request_ready {
            gpu_offer = false;
        }
        if display_offer && f.display.request_ready {
            display_offer = false;
        }
        if f.data.write_data_ready {
            w += 1;
            assert!(w <= 4);
        }
        if f.data.response_valid {
            acknowledged = true;
            assert_eq!(w, 4);
        }
        if f.gpu[0].response.valid {
            assert_eq!(f.gpu[0].response.data, gpu_expected[g]);
            assert_eq!(f.gpu[0].response.last, g == 15);
            g += 1;
        }
        if f.display.response.valid {
            assert_eq!(f.display.response.data, display_expected[d]);
            assert_eq!(f.display.response.last, d == 3);
            d += 1;
        }
        m.commit_logic(s);
        for half in 0..4 {
            let before = m.backing()[0x1000..0x1010].to_vec();
            m.clock_native_half().unwrap();
            let changed = m.backing()[0x1000..0x1010]
                .iter()
                .zip(before)
                .filter(|(a, b)| **a != *b)
                .count();
            assert!(changed <= 2);
            assert!(changed == 0 || half & 1 != 0);
            saw_partial |= changed > 0 && !acknowledged;
        }
        assert_eq!(m.native_cycles(), native_before + 2);
        if started && acknowledged && g == 16 && d == 4 {
            for (n, &word) in writes.iter().enumerate() {
                assert_eq!(qword(m.backing(), 0x1000 + n * 4), word);
            }
            assert!(simultaneous && saw_partial);
            return;
        }
    }
    panic!("shared memory bounded60000 logic edges: started{started} W{w} G{g} D{d}")
}
#[test]
#[ignore = "production descriptor slot and an actual old-credit mutation;30s watchdog"]
fn board_slot_model_matches_original_rtl_and_detects_borrowed_credit() {
    use std::{
        fmt::Write as _,
        fs,
        process::{Command, Stdio},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/c4")
        .join(format!(
            "slot-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    let top = include_str!("../../board/top.v");
    let (_, tail) = top.split_once("module CpuV3GpuMemoryRequestSlot(").unwrap();
    let original = format!("module CpuV3GpuMemoryRequestSlot({tail}");
    let mut tb=String::from("module tb;reg clk=0,reset,in_valid,in_write,out_ready;reg[21:0]in_address;reg[1:0]in_count;wire in_ready,out_valid,out_write;wire[21:0]out_address;wire[1:0]out_count;CpuV3GpuMemoryRequestSlot dut(.*);initial begin\n");
    for (n, (reset, i, writing, ready, s)) in slot_rows().into_iter().enumerate() {
        writeln!(tb,"reset=1'd{};in_valid=1'd{};in_write=1'd{};in_address=22'h{:x};in_count=2'd{};out_ready=1'd{};#4;if({{in_ready,out_valid,out_write,out_address,out_count}}!=={{1'd{},1'd{},1'd{},22'h{:x},2'd{}}})$fatal(1,\"DIFF slot {n}\");clk=1;#1;clk=0;#5;",u8::from(reset),u8::from(i.valid),u8::from(writing),i.address_halfword&0x3fffff,i.count,u8::from(ready),u8::from(s.ready(reset)),u8::from(s.request.valid),u8::from(s.writing),s.request.address_halfword,s.request.count).unwrap();
    }
    tb.push_str("$display(\"PASS slot\");$finish;end initial begin #10000;$fatal(1,\"watchdog\");end endmodule\n");
    for mutant in [false, true] {
        let dir = root.join(if mutant {
            "borrowed_credit"
        } else {
            "original"
        });
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("dut.v"),
            if mutant {
                original.replace("!out_valid&&!reset", "(!out_valid||out_ready)&&!reset")
            } else {
                original.clone()
            },
        )
        .unwrap();
        fs::write(dir.join("tb.v"), &tb).unwrap();
        for (phase, exe, args) in [
            (
                "compile",
                "IVERILOG_EXE",
                vec!["-g2012", "-s", "tb", "-o", "test.vvp", "dut.v", "tb.v"],
            ),
            ("run", "VVP_EXE", vec!["test.vvp"]),
        ] {
            let stdout = dir.join(format!("{phase}.out"));
            let stderr = dir.join(format!("{phase}.err"));
            let mut child = Command::new(std::env::var_os(exe).expect(exe))
                .args(args)
                .current_dir(&dir)
                .stdout(Stdio::from(fs::File::create(&stdout).unwrap()))
                .stderr(Stdio::from(fs::File::create(&stderr).unwrap()))
                .spawn()
                .unwrap();
            let start = Instant::now();
            let status = loop {
                if let Some(s) = child.try_wait().unwrap() {
                    break s;
                }
                if start.elapsed() > Duration::from_secs(30) {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("{phase} timeout is not mutation detection");
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            let out = fs::read_to_string(stdout).unwrap();
            let err = fs::read_to_string(stderr).unwrap();
            if phase == "compile" || !mutant {
                assert!(status.success(), "{phase}: {out}{err}");
            } else {
                assert!(
                    !status.success() && out.contains("DIFF slot"),
                    "mutation must fail actual slot comparison: {out}{err}"
                );
            }
        }
    }
    println!("C4_SLOT_ROOT {}", root.display());
}
