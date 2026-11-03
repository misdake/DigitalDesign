#[path = "support/vliw_draw/mod.rs"]
mod support;

#[test]
fn selected_cp_reads_object_rows_and_submits_twice() {
    for view in [false, true] {
        let run = support::run(support::Case {
            view,
            ..Default::default()
        });
        assert!(run.error.is_none());
        assert_eq!(run.completions, 2);
        assert!(run
            .trace
            .iter()
            .any(|e| !e.input.ce && e.input.draw.uniform_done));
    }
}
#[test]
fn cp_rejects_state_objects_on_the_original_monitor_edge() {
    for (bad, expected) in [
        ("missing_stored", "CP target drain contract"),
        ("global_code", "CP state data contract"),
        ("target_alias", "CP state data contract"),
        ("view_shape", "CP state data contract"),
        ("color_high", "CP mesh color encoding"),
        ("bad_local", "CP local DMA contract"),
        ("bad_mesh", "CP mesh DMA contract"),
    ] {
        let run = support::run(support::Case {
            bad: Some(bad),
            ..Default::default()
        });
        assert_eq!(
            run.error.unwrap().to_string(),
            format!("command_processor: {expected}")
        );
    }
}

#[test]
#[ignore = "bounded final-command export/CP differential; requires Python and Icarus"]
fn final_profile_and_cp_objects_match_selected_rtl() {
    use gpu_v2::{
        framebuffer::ports::MaterializedSurface,
        system::{pixel::quad_ingress::rtl::Backend, vliw_command},
    };
    use std::{
        fs,
        path::Path,
        process::{Command, Stdio},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = repo.join("target/gpu-cycle-draw").join(format!(
        "checkpoint-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let surface = MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x140000,
        width: 400,
        height: 240,
    };
    for (label, backend) in [("portable", Backend::Portable), ("native", Backend::Native)] {
        fs::write(
            root.join(format!("command-{label}.v")),
            vliw_command::verilog(surface, backend).unwrap(),
        )
        .unwrap();
    }
    for (name, case) in [
        ("pot", support::Case::default()),
        (
            "view",
            support::Case {
                view: true,
                ..Default::default()
            },
        ),
    ]
    .into_iter()
    .chain(
        [
            "missing_stored",
            "global_code",
            "target_alias",
            "view_shape",
            "color_high",
            "bad_local",
            "bad_mesh",
        ]
        .into_iter()
        .map(|bad| {
            (
                bad,
                support::Case {
                    bad: Some(bad),
                    ..Default::default()
                },
            )
        }),
    ) {
        let run = support::run(case);
        let (schema, rows) = support::vectors(&run);
        fs::write(root.join("schema.txt"), schema).unwrap();
        fs::write(root.join(format!("{name}.txt")), rows).unwrap();
    }
    let mut child = Command::new("python")
        .arg(repo.join("ip/gpu-v2/tests/vliw_cycle_draw.py"))
        .arg(&root)
        .stdout(Stdio::from(
            fs::File::create(root.join("qualification.out")).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(root.join("qualification.err")).unwrap(),
        ))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            assert!(
                s.success(),
                "{}\n{}\n{}",
                root.display(),
                fs::read_to_string(root.join("qualification.out")).unwrap(),
                fs::read_to_string(root.join("qualification.err")).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(600) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("CP checkpoint host watchdog: {}", root.display());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    println!("DRAW_CP_CHECKPOINT {}", root.display());
}
