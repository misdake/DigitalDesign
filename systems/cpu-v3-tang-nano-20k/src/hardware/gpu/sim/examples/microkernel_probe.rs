//! Readable end-to-end microkernel trace. This example is a debugging tool,
//! not an independent numerical golden.

use gpu_v2_cmodel::dma::DmaDesc;
use gpu_v2_cmodel::fixed::{Q14, Q16};
use gpu_v2_cmodel::format::{encode_stream, InputVertex, Uniform};
use gpu_v2_cmodel::micro_audit::audit_v0_trace;
use gpu_v2_cmodel::microkernel::{Command, DrawDesc, Microkernel, MvpMode, ROM};
use gpu_v2_cmodel::normal_contract::{check_scale_preserving, TRIAL_GRAM_TOLERANCE};
use gpu_v2_cmodel::timing::MemoryTiming;

fn q16(raw: i32) -> Q16 {
    Q16::from_raw(i128::from(raw)).unwrap()
}
fn q14(raw: i16) -> Q14 {
    Q14::from_raw(i128::from(raw)).unwrap()
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let seed = args
        .first()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(19);
    let verbose = args.iter().any(|arg| arg == "--trace");
    let mode = if args.iter().any(|arg| arg == "--dualwide") {
        MvpMode::DualWide
    } else if args.iter().any(|arg| arg == "--staged") {
        MvpMode::Staged
    } else {
        MvpMode::Streaming
    };
    let mut mvp = [[q16(0); 4]; 4];
    for (row, coefficients) in mvp.iter_mut().enumerate() {
        coefficients[row] = q16(0x10000);
    }
    mvp[0][1] = q16(0x8000);
    let mut normal = [[q14(0); 3]; 3];
    normal[0][1] = q14(-0x4000);
    normal[1][0] = q14(0x4000);
    normal[2][2] = q14(0x4000);
    let uniform = Uniform { mvp, normal };
    let driver_check = check_scale_preserving(normal, TRIAL_GRAM_TOLERANCE)
        .expect("driver-side normal matrix scale promise");
    let vertices = [
        InputVertex {
            position: [q16(0x10000), q16(0), q16(0), q16(0x10000)],
            normal: [q14(0x2000), q14(0), q14(0)],
            rgba: [255, 0, 0, 255],
        },
        InputVertex {
            position: [q16(0), q16(0x10000), q16(0), q16(0x10000)],
            normal: [q14(0), q14(0x2000), q14(0)],
            rgba: [0, 255, 0, 255],
        },
        InputVertex {
            position: [q16(0), q16(0), q16(0x10000), q16(0x10000)],
            normal: [q14(0), q14(0), q14(0x2000)],
            rgba: [0, 0, 255, 255],
        },
    ];
    let mut bytes = vec![0_u8; 512];
    bytes[..128].copy_from_slice(&uniform.encode());
    bytes[256..416].copy_from_slice(&encode_stream(vertices));
    let words = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| u64::from_le_bytes(*chunk))
        .collect();
    let mut model = Microkernel::new_with_mode(
        words,
        MemoryTiming {
            grant_wait: 2,
            first_beat: 3,
            beat_gap: 1,
            jitter: 2,
        },
        seed,
        mode,
    );
    for command in [
        Command::Dma(DmaDesc {
            physical_addr: 0,
            scratchpad_addr: 0,
            byte_count: 128,
            completion_token: 0,
        }),
        Command::Dma(DmaDesc {
            physical_addr: 256,
            scratchpad_addr: 256,
            byte_count: 160,
            completion_token: 1,
        }),
        Command::Draw(DrawDesc {
            uniform_token: 0,
            vertex_token: 1,
            uniform_addr: 0,
            stream_addr: 256,
            stream_bytes: 160,
            vertex_count: 3,
            triangle_count: 1,
            compact_grid: None,
        }),
    ] {
        model.submit(command).unwrap();
    }
    let edges = model.run(500).expect("bounded microkernel run");
    let audit = audit_v0_trace(&model.trace, mode).expect("independent trace audit");
    println!("seed={seed} mode={mode:?} complete={edges} edges");
    println!(
        "driver normal preflight: max Gram error={}",
        driver_check.max_abs_error
    );
    println!("trace audit: {:?}", audit);
    for step in &model.trace {
        if verbose {
            let operation = format!("{:?}", ROM[usize::from(step.pc)]);
            println!("{:3} {:<20} event={:?} sp_r={:?} sp_w={:?} dsp=({:?},{:?};{:?}) retire=({:?},{:?};{:?}) out={:?} tri={}",
                step.edge, operation, step.dispatched_event, step.scratch_read, step.scratch_write,
                step.wide_issue, step.wide_issue_second, step.small_issue, step.wide_retire, step.wide_retire_second, step.small_retire, step.result_write, step.triangle_pushed);
        } else if step.dispatched_event.is_some()
            || step.dma_completed.is_some()
            || step.vertex_published.is_some()
            || step.triangle_pushed
        {
            println!(
                "edge {:3}: event={:?} dma_done={:?} vertex={:?} triangle={}",
                step.edge,
                step.dispatched_event,
                step.dma_completed,
                step.vertex_published,
                step.triangle_pushed
            );
        }
    }
    for id in 0..3 {
        let vertex = model.results.vertex(id).unwrap();
        println!(
            "transformed[{id}]: clip={:?} normal={:?} rgba={:?}",
            vertex.clip.map(Q16::raw),
            vertex.normal.map(Q14::raw),
            vertex.rgba
        );
    }
    println!("setup_queue: {:?}", model.setup_queue);
}
