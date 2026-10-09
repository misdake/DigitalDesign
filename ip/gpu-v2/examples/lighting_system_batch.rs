//! Actual isolated emu batches; output handshakes and context drain are counted.
use gpu_v2::lighting::{emu::LightingEmu, ports::*, LightingProfile};
use std::fmt::Write;

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/gpu-v2-lighting/system-study".into());
    let mut csv=String::from("profile,mode,pixels,II,first_valid_edges,first_transfer_edges,context_to_drain_edges,context_to_next_context_edges,peak_tokens,id_slots\n");
    {
        let profile = LightingProfile::SystemFast;
        for full in [true, false] {
            for count in [16, 64, 256] {
                let mut emu = LightingEmu::with_system_profile(profile, 10000).unwrap();
                let context = LightingContext {
                    material: Material {
                        specular_color: if full { [255; 3] } else { [0; 3] },
                        ..Default::default()
                    },
                    light: Light::default(),
                    projection: Projection::default(),
                    epoch: 59,
                };
                let idle = LightingTick {
                    reset: false,
                    ce: true,
                    context: None,
                    input: None,
                    output_ready: true,
                };
                assert!(
                    emu.tick(LightingTick {
                        context: Some(context),
                        ..idle
                    })
                    .unwrap()
                    .context_ready
                );
                let mut sent = 0;
                let mut retired = 0;
                let mut first_accept = None;
                let mut first_valid = None;
                let mut first_transfer = None;
                let mut peak = 0;
                let mut end = 0;
                for edge in 1..10000 {
                    let input = (sent < count).then(|| LightingRequest {
                        pixel: PixelInput {
                            normal: [8192, 4096, 12288],
                            ndc: [(sent * 17) / 4, 0],
                        },
                        id: (sent as u32).wrapping_mul(0x9e3779b9) ^ 0x80004001,
                    });
                    let s = emu.tick(LightingTick { input, ..idle }).unwrap();
                    if s.input_ready && input.is_some() {
                        first_accept.get_or_insert(edge);
                        sent += 1;
                    }
                    if let Some(output) = s.output {
                        assert_eq!(
                            output.id,
                            (retired as u32).wrapping_mul(0x9e3779b9) ^ 0x80004001
                        );
                        assert_eq!(output.epoch, 59);
                        first_transfer.get_or_insert(edge);
                        retired += 1;
                    }
                    if emu.signals(idle).output.is_some() {
                        first_valid.get_or_insert(edge);
                    }
                    peak = peak.max(emu.in_flight());
                    if retired == count {
                        end = edge;
                        assert_eq!(emu.in_flight(), 0);
                        break;
                    }
                }
                assert_eq!(retired, count);
                assert!(emu.signals(idle).context_ready);
                assert!(
                    emu.tick(LightingTick {
                        context: Some(LightingContext {
                            epoch: 60,
                            ..context
                        }),
                        ..idle
                    })
                    .unwrap()
                    .context_ready
                );
                let rtl = gpu_v2::lighting::rtl::generate_with_options(
                    profile,
                    gpu_v2::lighting::rtl::LightingRtlOptions::system_profile(),
                )
                .unwrap();
                writeln!(
                    csv,
                    "{profile:?},{},{count},{},{},{},{end},{}, {peak},{}",
                    if full { "full" } else { "diffuse" },
                    emu.initiation_interval(),
                    first_valid.unwrap() - first_accept.unwrap(),
                    first_transfer.unwrap() - first_accept.unwrap(),
                    end + 1,
                    rtl.id_slots
                )
                .unwrap();
            }
        }
    }
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(format!("{dir}/batches.csv"), &csv).unwrap();
    print!("{csv}");
}
