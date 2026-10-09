//! Bounded Rust-only frontend/DSP/port experiment, no emulator or RTL.
use gpu_v2::{
    command_processor::ports::Command,
    frontend::{
        ports::Input,
        sim::timed::{self, Action},
    },
    scratchpad::ports::DmaDescriptor,
    vertex::{
        ports::*,
        sim::{counted, oracle, timed as transform},
    },
};
fn main() -> Result<(), String> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/gpu-v2-frontend".into());
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let context = Context {
        mvp: [
            [50001, -34567, 12289, 8001],
            [45678, 12345, -65536, 14123],
            [72345, -33456, 27411, 23455],
            [10012, -10034, 10111, 65536],
        ],
        normal_matrix: [[11585, -11585, 0], [11585, 11585, 0], [0, 0, 16384]],
        base: [-32768, 12345, -100000],
        grid_shift: 6,
    };
    let vertices = (0..64)
        .map(|v| {
            PackedVertex::encode(
                [
                    (v * 17 % 1024) as u16,
                    (v * 43 % 1024) as u16,
                    (v * 97 % 1024) as u16,
                ],
                [((v * 13 % 255) as i16 - 127) as i8, 90, -47],
                [(v * 67 % 4096) as u16, (4095 - v * 53 % 4096) as u16],
                (v * 1001) as u16,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    for &v in &vertices {
        let expected = oracle::run(&context, v, &oracle::Config::default())?;
        let c = counted::run(&context, v)?;
        if c.output != expected.output {
            return Err("probe numeric mismatch".into());
        }
    }
    let mut summary=String::from("wide,narrow,mvp_read_ports,setup,single_complete,single_body_publish,body_ii,body_span,batch8,batch64,macros,tiles,halfslots,bsram,ram16,live_bits8,wide_util8,narrow_util8,frontend_two_draw_cycles\n");
    for ports in 1..=2 {
        for (wide, narrow) in [(1, 1), (1, 2), (2, 1), (1, 3), (2, 2)] {
            let h = transform::Hardware {
                wide,
                narrow,
                matrix_read_ports: ports,
                ..transform::Hardware::default()
            };
            let single = transform::run(&context, &vertices[..1], h)?;
            let (ii, span) = single.periodic_body()?;
            let batch = transform::run(&context, &vertices[..8], h)?;
            let full = transform::run(&context, &vertices, h)?;
            let mut memory = Vec::new();
            for v in &vertices[..8] {
                for cell in v.0 {
                    memory.extend(cell.to_le_bytes());
                }
            }
            for v in &vertices {
                for cell in v.0 {
                    memory.extend(cell.to_le_bytes());
                }
            }
            let input = Input {
                memory_base: 0x1000,
                memory,
                commands: vec![
                    Command::Dma(DmaDescriptor {
                        physical_addr: 0x1000,
                        scratchpad_addr: 0,
                        byte_count: 96,
                        completion_token: 0,
                    }),
                    Command::Dma(DmaDescriptor {
                        physical_addr: 0x1060,
                        scratchpad_addr: 4096,
                        byte_count: 768,
                        completion_token: 1,
                    }),
                    Command::Wait(0),
                    Command::Draw {
                        region: 0,
                        byte_offset: 0,
                        vertices: 8,
                        context: context.clone(),
                    },
                    Command::Wait(1),
                    Command::Draw {
                        region: 1,
                        byte_offset: 0,
                        vertices: 64,
                        context: context.clone(),
                    },
                    Command::Fence,
                ],
            };
            let frontend = timed::run(
                &input,
                timed::Config {
                    hardware: h,
                    ..timed::Config::default()
                },
                &[],
            )?;
            let dsp = batch.dsp.audit().map_err(|e| format!("{e:?}"))?;
            let ram = batch.memory_usage()?;
            let (wu, nu) = batch.dsp_utilization();
            let line=format!("{wide},{narrow},{ports},{},{},{},{ii},{span},{},{},{},{},{},{},{},{},{wu:.4},{nu:.4},{}\n",single.setup_cycles,single.publication[0],single.publication[0]-single.setup_cycles,batch.publication.iter().max().unwrap(),full.publication.iter().max().unwrap(),dsp.macros,dsp.tiles,dsp.multiplier_half_slots,ram.bsram_blocks,ram.ssram_cells,batch.retained.peak_bits,frontend.cycles);
            print!("{line}");
            summary.push_str(&line);
            if ports == 1 && wide == 1 && narrow == 1 {
                std::fs::write(
                    format!("{output}/trace.txt"),
                    frontend
                        .records
                        .iter()
                        .map(|r| format!("{r:?}\n"))
                        .collect::<String>(),
                )
                .map_err(|e| e.to_string())?;
                std::fs::write(
                    format!("{output}/issue-rom.csv"),
                    format!(
                        "cycle,event,resource,lane\n{}",
                        single
                            .rom
                            .iter()
                            .map(|r| format!(
                                "{},{},{:?},{:?}\n",
                                r.cycle, r.event, r.resource, r.lane
                            ))
                            .collect::<String>()
                    ),
                )
                .map_err(|e| e.to_string())?;
                std::fs::write(
                    format!("{output}/goldens.txt"),
                    single
                        .counted
                        .frame
                        .outputs
                        .iter()
                        .map(|o| format!("{} {:?} {}\n", o.name, o.format, o.raw))
                        .collect::<String>(),
                )
                .map_err(|e| e.to_string())?;
                std::fs::write(
                    format!("{output}/counts.txt"),
                    format!(
                        "{:?}\nDSP {:?}\nRAM {:?}\n",
                        single.counted.frame.counts, dsp, ram
                    ),
                )
                .map_err(|e| e.to_string())?;
                let during=frontend.records.iter().filter(|r|matches!(r.action,Action::DmaWrite{lease,..} if lease.region==1) && frontend.records.iter().any(|start|start.cycle<=r.cycle && matches!(start.action,Action::DrawStart{lease,..} if lease.region==0))).count();
                println!("DMA region1 writes after draw0 started: {during}");
            }
        }
    }
    std::fs::write(format!("{output}/summary.csv"), summary).map_err(|e| e.to_string())?;
    Ok(())
}
