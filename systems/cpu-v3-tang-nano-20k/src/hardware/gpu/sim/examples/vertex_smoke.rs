use gpu_v2_cmodel::fixed::Q16;
use gpu_v2_cmodel::vertex::{run_vertex_program, RawVertex, VertexOp};

fn q(raw: i128) -> Q16 {
    Q16::from_raw(raw).expect("smoke input fits Q16")
}

fn main() {
    let vertex = RawVertex {
        position: [q(0x10000), q(-0x20000), q(0x8000), q(0x10000)],
        rgb565: 0xf81f,
    };
    let matrix = std::array::from_fn(|row| {
        std::array::from_fn(|column| q(if row == column { 0x10000 } else { 0 }))
    });
    let program = [
        VertexOp::LoadRaw { vertex: 0 },
        VertexOp::Mvp,
        VertexOp::Publish { destination: 1 },
        VertexOp::Stop,
    ];
    let run =
        run_vertex_program(&[vertex], matrix, &program, 32).expect("bounded vertex smoke run");
    for step in &run.trace {
        if step.read_address.is_some()
            || step.issued_product.is_some()
            || step.retired_product.is_some()
            || step.published
        {
            println!(
                "edge {:2}: ram={:?} issue={:?} retire={:?} publish={}",
                step.edge,
                step.read_address,
                step.issued_product,
                step.retired_product,
                step.published,
            );
        }
    }
    let result = &run.results[0];
    println!(
        "clip raw = {:?}; RGB565 = {:04x}; overflow row = {:?}",
        result.clip.map(Q16::raw),
        result.rgb565,
        result.overflow_row
    );
}
