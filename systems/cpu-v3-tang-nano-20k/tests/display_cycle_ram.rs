//! Bounded edge differential against the exported production DisplayLineBuffer.
//! This qualifies the RAM leaf, not the complete Display or physical CDC.
use cpu_v3_tang_nano_20k::hardware::display_cycle::{Edges, LineBuffer, LineBufferInput};
use cpu_v3_tang_nano_20k::DisplayLineBuffer;
use digital_design_hardware::Module;
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn step(
    ram: &mut LineBuffer,
    edges: Edges,
    input: LineBufferInput,
    out: &mut String,
    index: usize,
) {
    ram.clock(edges, &input);
    let [a, b] = ram.outputs();
    writeln!(
        out,
        "{index} {} {} {} {:x} {:016x} {:x} {:x} {:08x} {} {:08x} {}",
        u8::from(edges.logic_rise),
        u8::from(edges.pixel_rise),
        u8::from(input.write_enable),
        input.write_address,
        input.write_data,
        input.read_address_a,
        input.read_address_b,
        a.value,
        u8::from(a.known),
        b.value,
        u8::from(b.known)
    )
    .unwrap();
}

fn vectors() -> (String, usize, serde_json::Value) {
    let mut ram = LineBuffer::default();
    let mut text = String::new();
    let mut index = 0;
    let mut coverage = [0usize; 8];
    let mut push = |edges, input| {
        step(&mut ram, edges, input, &mut text, index);
        let q = ram.outputs();
        coverage[0] += usize::from(edges.logic_rise);
        coverage[1] += usize::from(edges.pixel_rise);
        coverage[2] += usize::from(edges.logic_rise && edges.pixel_rise);
        coverage[3] += usize::from(!edges.pixel_rise);
        if edges.pixel_rise {
            coverage[4] += usize::from(q[0].collision);
            coverage[5] += usize::from(q[1].collision);
            coverage[6] += usize::from(!q[0].known || !q[1].known);
            coverage[7] += usize::from(input.read_address_a >= 416);
        }
        index += 1;
    };
    // Uninitialized data/unused cells, all ROM cells, and registered-Q holding.
    for address in 0..512 {
        push(
            Edges {
                logic_rise: false,
                pixel_rise: true,
            },
            LineBufferInput {
                read_address_a: address,
                read_address_b: 511 - address,
                ..Default::default()
            },
        );
        push(Edges::default(), LineBufferInput::default());
    }
    // Write both halves, then force each bank's simultaneous same-cell old read.
    for address in [0, 99, 100, 199, 200, 299, 300, 399] {
        for generation in 0..3 {
            push(
                Edges {
                    logic_rise: true,
                    pixel_rise: true,
                },
                LineBufferInput {
                    write_enable: true,
                    write_address: address,
                    write_data: 0x1234_5678_9abc_def0
                        ^ (u64::from(address) << 24)
                        ^ (generation * 0x1111_2222_3333_4444),
                    read_address_a: address,
                    read_address_b: address,
                    ..Default::default()
                },
            );
        }
        push(
            Edges {
                logic_rise: false,
                pixel_rise: true,
            },
            LineBufferInput {
                read_address_a: address,
                read_address_b: address,
                ..Default::default()
            },
        );
    }
    // Relative production rising-edge ratio 54/33.3 = 60/37, with two phases.
    // Edges, rather than periods in the testbench delay, define this RAM contract.
    for phase in [0, 17] {
        for t in 0..16000_u32 {
            let edges = Edges {
                logic_rise: t % 37 == 0,
                pixel_rise: (t + phase) % 60 == 0,
            };
            if !edges.logic_rise && !edges.pixel_rise {
                continue;
            }
            push(
                edges,
                LineBufferInput {
                    write_enable: t % 3 != 0,
                    write_address: ((t / 37) % 400) as u16,
                    write_data: (u64::from(t) << 32) | u64::from(!t),
                    read_address_a: ((t / 60) % 512) as u16,
                    read_address_b: ((t / 60 + 199) % 512) as u16,
                    ..Default::default()
                },
            );
        }
    }
    assert!(index <= 4096);
    assert!(coverage[2] >= 24 && coverage[3] >= 512);
    assert!(coverage[4] >= 24 && coverage[5] >= 24);
    assert!(coverage[6] > 0 && coverage[7] >= 96);
    (
        text,
        index,
        serde_json::json!({
            "logic_edges":coverage[0], "pixel_edges":coverage[1],
            "coincident_edges":coverage[2], "read_q_holds":coverage[3],
            "bank_collisions":[coverage[4],coverage[5]],
            "unknown_reads":coverage[6], "table_reads":coverage[7]
        }),
    )
}

#[test]
fn registered_reads_hold_and_report_collisions_without_resetting_storage() {
    let mut ram = LineBuffer::default();
    let input = LineBufferInput {
        write_enable: true,
        write_address: 7,
        write_data: 0x1234_5678_9abc_def0,
        read_address_a: 7,
        read_address_b: 8,
        ..Default::default()
    };
    ram.clock(
        Edges {
            logic_rise: true,
            pixel_rise: true,
        },
        &input,
    );
    let [a, b] = ram.outputs();
    assert!(!a.known && a.collision && !b.known && !b.collision);
    let held = ram.outputs();
    ram.clock(Edges::default(), &LineBufferInput::default());
    assert_eq!(ram.outputs(), held);
    ram.clock(
        Edges {
            logic_rise: false,
            pixel_rise: true,
        },
        &input,
    );
    assert_eq!(ram.outputs()[0].value, 0x9abc_def0);
    assert!(!ram.outputs()[1].known);
    assert_eq!(ram.peek(1, 7).value, 0x1234_5678);
    assert!(ram.outputs()[0].known && !ram.outputs()[0].collision);
}

fn run(mut command: Command, out: &Path, name: &str) -> Output {
    command.stdout(Stdio::from(
        std::fs::File::create(out.join(format!("{name}.stdout"))).unwrap(),
    ));
    command.stderr(Stdio::from(
        std::fs::File::create(out.join(format!("{name}.stderr"))).unwrap(),
    ));
    let argv: Vec<_> = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    std::fs::write(
        out.join(format!("{name}.command.json")),
        serde_json::to_string_pretty(&argv).unwrap(),
    )
    .unwrap();
    let start = Instant::now();
    let mut child = command.spawn().expect("external RTL tool");
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("{name} exceeded 30s wall bound");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "explicit bounded production RAM RTL differential and DUT mutations"]
fn production_ram_edges_and_real_dut_mutations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/display-cycle-ram");
    std::fs::create_dir_all(&out).unwrap();
    let (trace, count, coverage) = vectors();
    std::fs::write(out.join("vectors.txt"), trace).unwrap();
    let source = DisplayLineBuffer::verilog_source().unwrap();
    std::fs::write(out.join("tb.v"), include_str!("display_cycle_ram_tb.v")).unwrap();
    let cases = [
        ("production", source.clone(), true),
        (
            "wrong_bank",
            source.replace("write_data[31:0]", "write_data[63:32]"),
            false,
        ),
        (
            "live_q",
            source.replace("always @(posedge read_clock)", "always @*"),
            false,
        ),
    ];
    let mut results = Vec::new();
    for (name, source, pass) in cases {
        std::fs::write(out.join(format!("{name}.v")), &source).unwrap();
        let exe = out.join(format!("{name}.vvp"));
        let mut compile =
            Command::new(std::env::var("IVERILOG_EXE").unwrap_or_else(|_| "iverilog".into()));
        compile
            .args(["-g2012", "-s", "tb", "-o"])
            .arg(&exe)
            .arg(out.join("tb.v"))
            .arg(out.join(format!("{name}.v")));
        assert!(run(compile, &out, &format!("{name}-compile"))
            .status
            .success());
        let mut simulate = Command::new(std::env::var("VVP_EXE").unwrap_or_else(|_| "vvp".into()));
        simulate
            .arg(exe)
            .arg(format!("+vectors={}", out.join("vectors.txt").display()));
        let status = run(simulate, &out, name).status;
        let stdout = std::fs::read_to_string(out.join(format!("{name}.stdout"))).unwrap();
        if pass {
            assert!(
                status.success() && stdout.contains(&format!("PASS RAM {count}")),
                "{stdout}"
            );
        } else {
            assert!(
                !status.success() && stdout.contains("RAM_Q_"),
                "mutation not detected: {name}: {stdout}"
            );
        }
        results.push(serde_json::json!({"case":name,"expected_pass":pass,"exit":status.code()}));
    }
    let mut files = serde_json::Map::new();
    for entry in std::fs::read_dir(&out).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|ext| ext == "v" || ext == "txt")
        {
            let hash = Command::new("git")
                .args(["hash-object"])
                .arg(&path)
                .output()
                .unwrap();
            assert!(hash.status.success());
            files.insert(
                path.file_name().unwrap().to_string_lossy().into_owned(),
                String::from_utf8(hash.stdout).unwrap().trim().into(),
            );
        }
    }
    for relative in [
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/cycle/line_buffer.rs",
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/cycle/mod.rs",
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/precode_table.rs",
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/display_line_buffer.v",
        "systems/cpu-v3-tang-nano-20k/src/display.rs",
        "systems/cpu-v3-tang-nano-20k/tests/display_cycle_ram.rs",
    ] {
        let hash = Command::new("git")
            .arg("-C")
            .arg(&root)
            .arg("hash-object")
            .arg(relative)
            .output()
            .unwrap();
        assert!(hash.status.success());
        files.insert(
            relative.to_owned(),
            String::from_utf8(hash.stdout).unwrap().trim().into(),
        );
    }
    let identity = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(identity.status.success());
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "scope":"DisplayLineBuffer leaf, behavioral old-Q; no full Display/physical CDC claim",
            "git_head":String::from_utf8(identity.stdout).unwrap().trim(),"coverage":coverage,
        "edge_rows":count,"max_edge_rows":4096,"wall_seconds_per_process":30,
            "source_object_hashes":files,"results":results,
        }))
        .unwrap(),
    )
    .unwrap();
}
