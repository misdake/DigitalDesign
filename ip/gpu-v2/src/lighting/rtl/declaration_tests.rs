use super::*;

#[test]
fn context_and_physical_profiles_are_independent() {
    let (options, plans) = crate::lighting::dsp_sharing::four_cycle_shared_roms(9).unwrap();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/rb09/context-rebase/matrix");
    std::fs::create_dir_all(&out).unwrap();
    for live in [false, true] {
        for physical in [false, true] {
            let core = generate_with_optional_plans(
                LightingProfile::Fast,
                options,
                Some(&plans),
                live,
                physical,
            )
            .unwrap();
            let body = core.source.split_once("endmodule").unwrap().0;
            assert_eq!(body.contains("reg configured;"), !live);
            assert_eq!(body.contains("input wire context_valid"), !live);
            assert_eq!(body.contains("square_z_tail_q"), physical);
            let mac = body.split_once("MULTADDALU18X18 #(").unwrap().1;
            let (parameters, instance) = mac.split_once(") dsp").unwrap();
            assert!(parameters.contains(if physical {
                ".OUT_REG(1'b1)"
            } else {
                ".OUT_REG(1'b0)"
            }));
            let lane = instance.split_once('(').unwrap().0;
            assert!(body.contains(&format!(
                "{} [53:0] unit{lane}_tail;",
                if physical { "wire" } else { "reg" }
            )));
            std::fs::write(
                out.join(format!(
                    "live{}-physical{}.v",
                    u8::from(live),
                    u8::from(physical)
                )),
                core.source,
            )
            .unwrap();
        }
    }
    let mut ram = options;
    ram.ram_retained = true;
    std::fs::write(
        out.join("public-h9-ram.v"),
        generate_with_schedule_plans(LightingProfile::Fast, ram, &plans)
            .unwrap()
            .source,
    )
    .unwrap();
    for physical in [false, true] {
        assert!(generate_with_optional_plans(
            LightingProfile::SystemFast,
            options,
            Some(&plans),
            true,
            physical
        )
        .is_err());
        assert!(
            generate_with_optional_plans(LightingProfile::Fast, options, None, true, physical)
                .is_err()
        );
        let mut invalid = options;
        invalid.smooth_h9 = false;
        assert!(generate_with_optional_plans(
            LightingProfile::Fast,
            invalid,
            Some(&plans),
            true,
            physical
        )
        .is_err());
        invalid = options;
        invalid.unified_lit = false;
        assert!(generate_with_optional_plans(
            LightingProfile::Fast,
            invalid,
            Some(&plans),
            true,
            physical
        )
        .is_err());
    }
}

#[test]
fn attributed_declarations_keep_their_initializer_boundary() {
    let ram = "(* syn_ramstyle = \"block_ram\" *) reg [8:0] token_ram [0:15];\n";
    let kept = "(* syn_preserve=1, syn_keep=1 *) reg [15:0] prescaled;\n";
    let initialized = "(* syn_keep = 1 *) wire [3:0] tail = prescaled[3:0];\n";
    let (declarations, body) = hoist(&format!("{ram}{kept}{initialized}"));
    assert_eq!(
        declarations,
        format!("{ram}{kept}(* syn_keep = 1 *) wire [3:0] tail;\n")
    );
    assert_eq!(body, "assign tail = prescaled[3:0];\n");
}

#[test]
#[ignore = "requires Icarus; compile-only public retained-RAM regression"]
fn generic_retained_ram_compiles_with_iverilog() {
    use std::{fs::File, process::Command, time::Duration, time::Instant};
    let source = generate_with_options(
        LightingProfile::Fast,
        LightingRtlOptions {
            ram_retained: true,
            ..Default::default()
        },
    )
    .unwrap()
    .source;
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-lighting-retained-ram");
    std::fs::create_dir_all(&directory).unwrap();
    let rtl = directory.join("generic.v");
    std::fs::write(&rtl, source).unwrap();
    let log_path = directory.join("compile.log");
    let log = File::create(&log_path).unwrap();
    let mut child =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()))
            .args(["-g2012", "-tnull", "-s", "gpu_v2_lighting"])
            .arg(rtl)
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "Icarus {status}; see {}",
                log_path.display()
            );
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("Icarus compile watchdog; see {}", log_path.display());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
