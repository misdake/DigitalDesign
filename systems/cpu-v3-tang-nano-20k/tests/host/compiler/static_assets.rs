//! Native producer -> checked compiler -> placement/fixups -> boot -> real pixels.
use cpu_v3::{rcc_backend, StepOutcome};
use cpu_v3_tang_nano_20k::{
    boot,
    system_sim::{CpuV3SystemSim, VBlankMode},
};
use rcc::frontend::{rustc_check::RustcCheck, static_assets::AssetBank};
use std::{path::Path, time::Duration};

#[path = "../../../build_support/asset_relocation.rs"]
mod relocation;
use crate::shared_build_support_rcc_modules as resolver;

#[test]
fn native_descriptions_relocate_across_banks_without_changing_pixels() {
    let system = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = system.join("../../target/sdk-native-assets/relocation");
    std::fs::create_dir_all(&output).unwrap();
    let fixture = system.join("tests/fixtures/sdk_assets");
    assert!(std::process::Command::new("python")
        .arg("-B")
        .arg(fixture.join("producer.py"))
        .arg("--output")
        .arg(&output)
        .status()
        .unwrap()
        .success());
    let constants = std::fs::read_to_string(output.join("native_assets.rs")).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("assets.json")).unwrap()).unwrap();
    let records =
        cpu_v3_tools::applications::asset_metadata::sector_relocations(&manifest).unwrap();
    let source = std::fs::read_to_string(fixture.join("main.rs")).unwrap();
    let mut baseline = None;
    for (label, start, end) in [("high", 0x4e0000, 0x7df800), ("low", 0x100000, 0x400000)] {
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.stack_init = boot::SINGLE_BOOT_STACK;
        options.asset_banks = vec![AssetBank { bytes: start..end }];
        let mut ir = resolver::GpuSources {
            local_dir: &fixture,
            shared_dir: &system.join("rcc"),
            constants: &constants,
            ordinary_dirs: &[],
            configuration: &resolver::RendererCaptureConfiguration::without_capture(),
        }
        .compile_checked(
            "native asset fixture",
            &source,
            &options,
            &RustcCheck::new(output.join(label).join("rustc")),
            &mut |_| {},
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(ir.assets.placed.len(), 3);
        let mesh = ir
            .assets
            .placed
            .iter()
            .find(|a| a.name == "TRIANGLE_BYTES")
            .unwrap();
        assert_eq!(&mesh.data[..2], &[0, 0]);
        let expected = (mesh.address + 128) / 128;
        relocation::apply(&mut ir.assets.placed, &records).unwrap();
        let mesh = ir
            .assets
            .placed
            .iter()
            .find(|a| a.name == "TRIANGLE_BYTES")
            .unwrap();
        assert_eq!(
            usize::from(u16::from_le_bytes(mesh.data[..2].try_into().unwrap())),
            expected
        );
        let resources: Vec<_> = ir
            .assets
            .placed
            .iter()
            .map(|a| boot::BootResource::copy(a.address, a.load_data()))
            .chain(
                ir.assets
                    .buffers
                    .iter()
                    .map(|b| boot::BootResource::zero(b.address, b.bytes)),
            )
            .collect();
        let words = ir.debug.static_data_end;
        let mut data = vec![0; words * 2];
        for (at, value) in ir.take_static_initializers() {
            data[at as usize * 2..at as usize * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
        let code: Vec<_> = program.words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let package = boot::pack_application_with_data(&code, words, &data, &resources).unwrap();
        let export = output.join(label);
        std::fs::write(export.join("application.v3bin"), &code).unwrap();
        std::fs::write(export.join("application.static-data"), &data).unwrap();
        std::fs::write(export.join("application.static-words"), words.to_string()).unwrap();
        let mut recipe = String::from("format 2\napplication main.rs\n");
        for (i, resource) in resources.iter().enumerate() {
            if resource.zero_bytes != 0 {
                recipe.push_str(&format!(
                    "zero {:#x} {}\n",
                    resource.address, resource.zero_bytes
                ));
            } else {
                let name = format!("resource-{i}.bin");
                std::fs::write(export.join(&name), &resource.data).unwrap();
                recipe.push_str(&format!(
                    "copy RESOURCE_{i} {:#x} {name}\n",
                    resource.address
                ));
            }
        }
        std::fs::write(export.join("application.repack.conf"), recipe).unwrap();
        std::fs::write(output.join(format!("{label}.bin")), &package).unwrap();
        let mut sim =
            CpuV3SystemSim::from_single_flash(&package, 0xa55a, VBlankMode::PauseOnFrameIndexWait)
                .unwrap()
                .with_functional_gpu();
        let mut halted = false;
        for _ in 0..8_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_host_time(Duration::from_micros(16667));
                sim.advance_vblank();
            }
            if let StepOutcome::Halted { signal } = sim.step().unwrap() {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded {label} native-asset run");
        assert!(sim.gpu().last_error().is_none());
        let rgb = sim.render_active_framebuffer();
        assert!(
            rgb.iter().filter(|&&p| p == 0xffffff).count() > 40,
            "font glyphs"
        );
        assert!(
            rgb.iter()
                .filter(|&&p| (p >> 16) & 255 > 100 && (p & 255) < 32)
                .count()
                > 500,
            "textured triangle"
        );
        let mut ppm = b"P6\n400 240\n255\n".to_vec();
        for &p in &rgb {
            ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
        std::fs::write(output.join(format!("{label}.ppm")), ppm).unwrap();
        if let Some(before) = &baseline {
            assert_eq!(before, &rgb, "relocation changes addresses, not pixels");
        } else {
            baseline = Some(rgb);
        }
    }
}
