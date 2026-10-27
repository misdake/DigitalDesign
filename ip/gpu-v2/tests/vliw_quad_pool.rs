// Independent qualification of the shared producer/consumer pool.
mod lighting {
    pub use gpu_v2::lighting::*;
}
#[path = "../src/system/pixel/vliw_quad_pool.rs"]
mod pool;

#[test]
fn ndc_rom_exactly_matches_main_pixel_center_contract() {
    let rows = pool::ndc_words();
    let rne = |n: i64, d: i64| {
        let negative = n < 0;
        let a = n.abs();
        let mut q = a / d;
        let r = a % d;
        if 2 * r > d || (2 * r == d && q & 1 != 0) {
            q += 1;
        }
        if negative {
            -q
        } else {
            q
        }
    };
    for x in 0..400 {
        let got = (rows[x / 2] >> (16 * (x & 1))) as u16 as i16;
        assert_eq!(i64::from(got), rne((2 * x as i64 + 1 - 400) * 16384, 400));
    }
    for y in 0..240 {
        let got = (rows[200 + y / 2] >> (16 * (y & 1))) as u16 as i16;
        assert_eq!(i64::from(got), rne((240 - 2 * y as i64 - 1) * 16384, 240));
    }
    assert!(pool::verilog(true).contains(pool::MODULE));
    assert!(pool::verilog(false).contains(".INIT_RAM_3F("));
}

#[test]
#[ignore = "requires Icarus and Gowin simulation primitives"]
fn producer_pool_portable_and_native_protocol_qualification() {
    use std::{
        fs,
        process::Stdio,
        time::{Duration, Instant},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tag = std::env::var("VLIW_POOL_TEST_TAG")
        .unwrap_or_else(|_| "pool-completion-contract-v1".into());
    let out = root
        .join("target/gpu-vliw-integration-20261018/stage2")
        .join(tag);
    fs::create_dir_all(&out).unwrap();
    for (name, portable) in [("portable", true), ("native", false)] {
        fs::write(out.join(format!("{name}.v")), pool::verilog(portable)).unwrap();
    }
    let mut child = std::process::Command::new("python")
        .arg(root.join("ip/gpu-v2/tests/vliw_quad_pool.py"))
        .stdout(Stdio::from(
            fs::File::create(out.join("qualification.out")).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(out.join("qualification.err")).unwrap(),
        ))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let log = fs::read_to_string(out.join("qualification.out")).unwrap();
            assert!(
                status.success(),
                "{log}\n{}",
                fs::read_to_string(out.join("qualification.err")).unwrap()
            );
            assert!(log.contains("PASS POOL MATRIX"));
            println!("{log}");
            break;
        }
        if start.elapsed() > Duration::from_secs(240) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("pool bounded host timeout");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
