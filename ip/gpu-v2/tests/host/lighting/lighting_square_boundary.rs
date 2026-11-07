//! The low and high split-square operands must retain the same register age.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    lighting::{dsp_sharing, rtl, LightingProfile},
    system::{pixel::quad_ingress::rtl::Backend, vliw_command},
};

#[test]
fn selected_square_corrections_keep_the_prescaled_register_boundary() {
    let surface = MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x140000,
        width: 400,
        height: 240,
    };
    for backend in [Backend::Native, Backend::Portable] {
        let source = vliw_command::verilog(surface, backend).unwrap();
        let light = source
            .split_once("module gpu_v2_lighting(")
            .unwrap()
            .1
            .split_once("endmodule")
            .unwrap()
            .0;
        let mut macs = light.split("MULTADDALU18X18 #(").skip(1);
        let mac = macs.next().unwrap();
        assert!(macs.next().is_none());
        let (parameters, rest) = mac.split_once(") dsp").unwrap();
        assert!(parameters.contains(".OUT_REG(1'b1)"));
        for port in [
            "A0REG",
            "B0REG",
            "A1REG",
            "B1REG",
            "CREG",
            "PIPE0_REG",
            "PIPE1_REG",
        ] {
            assert!(parameters.contains(&format!(".{port}(1'b0)")));
        }
        let unit = rest.split_once('(').unwrap().0;
        assert!(light.contains(&format!("wire [53:0] unit{unit}_tail;")));
        assert!(light.contains(&format!("assign unit{unit}_tail=unit{unit}_dsp_out;")));
        assert!(!light.contains(&format!("unit{unit}_tail<=unit{unit}_dsp_out;")));
        for (coordinate, producer, consumer) in [("z", 12, 24), ("y", 11, 28)] {
            assert!(light.contains(&format!(
                "(* syn_preserve=1, syn_keep=1 *) reg [15:0] unit{producer}_p0;"
            )));
            assert!(light.contains(&format!(
                "(* syn_keep=1 *) wire [6:0] square_{coordinate}_tail_q=unit{producer}_p0[6:0];"
            )));
            let dsp = light
                .split_once(&format!(") dsp{consumer}("))
                .unwrap()
                .1
                .split_once("\n);")
                .unwrap()
                .0;
            assert!(dsp.contains(&format!(".B({{2'b00,square_{coordinate}_tail_q}})")));
            assert!(dsp.contains(".CE(datapath_ce),.RESET(1'b0)"));
        }
        if matches!(backend, Backend::Native) {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/gpu-lighting-square-boundary");
            std::fs::create_dir_all(&out).unwrap();
            std::fs::write(out.join("dut.v"), source).unwrap();
        }
    }
    // Explicit public calendars do not opt into the private II4 physical preset.
    for shared in [false, true] {
        let (options, plans) = if shared {
            dsp_sharing::four_cycle_shared_roms(9)
        } else {
            dsp_sharing::four_cycle(9)
        }
        .unwrap();
        let generic =
            rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &plans).unwrap();
        assert!(!generic.source.contains("square_z_tail_q"));
        assert!(!generic.source.contains("square_y_tail_q"));
        let mac = generic.source.split_once("MULTADDALU18X18 #(").unwrap().1;
        let (parameters, rest) = mac.split_once(") dsp").unwrap();
        assert!(parameters.contains(".OUT_REG(1'b0)"));
        let unit = rest.split_once('(').unwrap().0;
        assert!(generic
            .source
            .contains(&format!("reg [53:0] unit{unit}_tail;")));
        assert!(generic
            .source
            .contains(&format!("unit{unit}_tail<=unit{unit}_dsp_out;")));
    }
}
