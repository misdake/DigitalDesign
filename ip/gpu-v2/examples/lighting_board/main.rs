//! Standalone, volatile Tang Nano 20K qualification of the selected Floor calendar.
use digital_design_circuit::CircuitWires;
use digital_design_hardware_gowin::*;
use gpu_v2::lighting::{
    calendars::UnifiedCalendar,
    ports::*,
    rtl,
    sim::{counted, oracle},
    LightingProfile, LightingQuantization,
};
use std::{error::Error, fmt::Write, fs, path::PathBuf};

type Reporter = DiagnosticReporter<0x0c, 234, 27_000, 2_700_000>;
const CONTEXTS: usize = 22;
const PIXELS: usize = 32;
const OUTPUT: &str = "target/lighting_board_gowin";

fn contexts() -> Vec<LightingContext> {
    (0..CONTEXTS)
        .map(|i| {
            // Generic unit vectors retain real variable DSP operands on the board.
            let vectors = [[1., 2., 3.], [-2., 3., 1.], [3., -1., -2.], [-1., -2., 3.]];
            let v = vectors[i % vectors.len()];
            let length = f64::sqrt(v.iter().map(|x| x * x).sum());
            LightingContext {
                material: Material {
                    shininess_code: if i < 5 { 8 } else { (i - 5) as u8 },
                    specular_color: if i < 5 { [0; 3] } else { [255; 3] },
                    ..Material::default()
                },
                light: Light {
                    direction: v.map(|x| (x / length * 16384.).round() as i16),
                    ambient: (31 + i * 7) as u16,
                    directional: if i == 0 { 0 } else { (129 + i * 5) as u16 },
                },
                projection: Projection {
                    ray_scale: [8192 + (i % 3) as i16 * 1024, 10240],
                    k: if i % 2 == 0 { 8192 } else { 12288 },
                },
                epoch: 0x8200 + i as u16,
            }
        })
        .collect()
}

fn vector(context: usize, index: usize) -> (CompactPixelInput, u32) {
    let boundary = [
        [0, 0, 0],
        [-2048, 2047, 0],
        [2047, -2048, 2047],
        [1, 0, 0],
        [-1, 0, 0],
        [0, 0, 1024],
        [0, 0, -1024],
        [1024, 0, 0],
        [0, 1024, 0],
    ];
    let k = (context * PIXELS + index) as i32;
    (
        CompactPixelInput {
            normal: boundary.get(index).copied().unwrap_or([
                ((k * 311 + 23) % 4096 - 2048) as i16,
                ((k * 197 + 13) % 4096 - 2048) as i16,
                ((k * 83 + 41) % 4096 - 2048) as i16,
            ]),
            ndc: if index == 0 {
                [-16384, 16384]
            } else if index == 1 {
                [16384, -16384]
            } else {
                [(k * 503) % 32769 - 16384, (k * 1097 + 7) % 32769 - 16384]
            },
        },
        0xfedcba98_u32.wrapping_sub((k as u32).wrapping_mul(0x23456789)),
    )
}

fn generated() -> Result<String, Box<dyn Error>> {
    let q = LightingQuantization::CompensatedFloor;
    let calendar = UnifiedCalendar::selected(q);
    let core = rtl::generate_with_schedule_plans(
        LightingProfile::Fast,
        calendar.options(q),
        &calendar.plans(q)?,
    )?;
    assert_eq!(
        (
            core.latency,
            core.diffuse_latency,
            core.specular_ii,
            core.diffuse_ii
        ),
        (38, 38, 2, 2)
    );
    let mut roms = String::from("reg [99:0] input_rom [0:1023];\nreg [49:0] golden_rom [0:1023];\ninteger init_index;\ninitial begin\nfor(init_index=0;init_index<1024;init_index=init_index+1) begin input_rom[init_index]=0;golden_rom[init_index]=0;end\n");
    let golden = oracle::Config::from_counted(counted::Config {
        rsqrt_q13: true,
        ..counted::Config::lit_queue_resource_profile(LightingProfile::Fast, q)
    });
    for (c, context) in contexts().into_iter().enumerate() {
        for i in 0..PIXELS {
            let (compact, id) = vector(c, i);
            let expanded = compact.expanded().map_err(|e| format!("{e:?}"))?;
            let result = oracle::evaluate_output(
                expanded,
                context.material,
                context.light,
                context.projection,
                golden,
            )
            .map_err(|e| format!("{e:?}"))?;
            let rows = compact.rows().map_err(|e| format!("{e:?}"))?;
            let input = u128::from(rows[0]) | u128::from(rows[1]) << 36 | u128::from(id) << 68;
            let expected = u64::from(id) | u64::from(result.g) << 32 | u64::from(result.h) << 41;
            writeln!(
                roms,
                "input_rom[{}]=100'h{input:025x};golden_rom[{}]=50'h{expected:013x};",
                c * PIXELS + i,
                c * PIXELS + i
            )?;
        }
    }
    roms.push_str("end\n");
    let mut uniforms = String::from("always @* begin\nlight_x=0;light_y=0;light_z=16384;ambient=0;directional=0;ray_x=8192;ray_y=8192;ray_k=8192;context_mode=1;context_code=8;\ncase(context_index)\n");
    for (i, c) in contexts().iter().enumerate() {
        writeln!(uniforms,"5'd{i}:begin light_x={};light_y={};light_z={};ambient={};directional={};ray_x={};ray_y={};ray_k={};context_mode={};context_code={};end",c.light.direction[0],c.light.direction[1],c.light.direction[2],c.light.ambient,c.light.directional,c.projection.ray_scale[0],c.projection.ray_scale[1],c.projection.k,c.mode(),c.material.shininess_code)?;
    }
    uniforms.push_str("default:begin end\nendcase\nend\n");
    let board = include_str!("self_test.v")
        .replace("__ROMS__", &roms)
        .replace("__UNIFORMS__", &uniforms)
        .replace("__REPORTER__", &Reporter::verilog_identity().module_name());
    Ok(format!(
        "`ifndef LIGHTING_BOARD_SIM\n`define GPU_V2_GOWIN_DSP\n`endif\n{}\n{}\n{}",
        core.source,
        Reporter::generated_verilog_source().unwrap(),
        board
    ))
}

struct LightingBoard;
impl HardwareIdentity for LightingBoard {
    const TARGET_RESOURCE_LEAF: bool = true;
    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("LightingBoard")
    }
}
impl Module for LightingBoard {
    type Input = TangNano20KInputs;
    type Output = TangNano20KDebugOutputs;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;
    fn execute_emu(_: &mut (), _: &mut CircuitWires, _: &Self::Input, _: &Self::Output) {
        panic!("RTL board probe");
    }
    fn verilog_source() -> Option<String> {
        Some(generated().expect("validated board fixtures"))
    }
    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_tb.v").into())
    }
    fn target_resources() -> Vec<TargetResourceRequest> {
        // Conservative whole-image ceilings, including test ROMs. Exact use is audited after fit.
        vec![TargetResourceRequest {
            component: "lighting-board-qualification",
            resources: vec![
                ResourceAmount::new(ResourceKind::Bsram18K, 24),
                ResourceAmount::new(ResourceKind::SsramBit, 4096),
                ResourceAmount::new(ResourceKind::Multiplier18x18, 18),
                ResourceAmount::new(ResourceKind::Pll, 1),
            ],
        }]
    }
}

fn project() -> GowinProject<TangNano20K> {
    let binding =
        GowinBoardBinding::new("lighting_board_top", "clk", "clk", TangNano20K::CLOCK_27M)
            .require(Clock27M)
            .require(UserButtons::<2>)
            .require(UserLeds::<6>)
            .require(DebugUartTx)
            .bind_port(
                GowinPortDirection::Input,
                "buttons",
                "buttons",
                TangNano20K::USER_BUTTONS,
            )
            .bind_port(
                GowinPortDirection::Output,
                "leds",
                "leds",
                TangNano20K::USER_LEDS,
            )
            .bind_port(
                GowinPortDirection::Output,
                "uart_tx",
                "uart_tx",
                [TangNano20K::DEBUG_UART_TX],
            );
    GowinProject::new("lighting_board").with_board_binding(binding)
        .add_source_file("src/lighting-clock.sdc", "create_clock -name clk -period 37.037037 [get_ports {clk}]\ncreate_generated_clock -name lighting_clk -source [get_ports {clk}] -multiply_by 20 -divide_by 9 [get_pins {u_logic/pll/CLKOUT}]\nset_clock_groups -asynchronous -group [get_clocks {clk}] -group [get_clocks {lighting_clk}]\n")
        .expect_dsp_mode(GowinDspMode::Mult9x9,ResourceCountExpectation::Exact(8))
        .expect_dsp_mode(GowinDspMode::Mult18x18,ResourceCountExpectation::Exact(10))
        .expect_dsp_mode(GowinDspMode::MultAddAlu18x18,ResourceCountExpectation::Exact(2))
        .expect_bsram_blocks(ResourceCountExpectation::Exact(16))
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.iter().any(|a| {
        !matches!(
            a.as_str(),
            "--build" | "--check-existing" | "--program-existing"
        )
    }) {
        return Err("use --build, --check-existing or --program-existing; output is target/lighting_board_gowin".into());
    }
    let output = PathBuf::from(OUTPUT);
    let p = project().export::<LightingBoard>(&output)?;
    fs::write(
        output.join("signature_tb.v"),
        include_str!("signature_tb.v"),
    )?;
    fs::write(output.join("fixture-summary.txt"),"CompensatedFloor,selected free per-edge,38 advancing edges,60MHz,22 contexts,32 pixels/context,2 passes,1408 outputs; independent integer oracle; DDHT v1 test ID 0x0c\n")?;
    if !arguments.is_empty() {
        let tools = GowinToolchain::discover()?;
        if arguments.iter().any(|a| a == "--build") {
            tools.build(&output, &p)?;
        }
        if arguments
            .iter()
            .any(|a| matches!(a.as_str(), "--check-existing" | "--program-existing"))
        {
            let bitstream = tools.validate_existing_build(&output, &p)?;
            println!("Audited {}", bitstream.display());
            if arguments.iter().any(|a| a == "--program-existing") {
                tools.program_sram_bitstream(
                    p.device,
                    &bitstream,
                    p.device.programmer_cable.index(),
                )?;
            }
        }
    }
    println!("Prepared {}", output.display());
    Ok(())
}
