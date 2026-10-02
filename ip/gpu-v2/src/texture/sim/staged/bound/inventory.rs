//! Declared storage inventory; no LUT/FF packing or frequency estimate.
//! Each row is a dedicated allocation, not the peak live-data count. FF input
//! fields have continuous fanout; rotating banks require selection logic.
use super::{control, Binding, StagePlan};
use crate::texture::sim::timed;

#[derive(Clone, Debug)]
pub struct Row {
    pub name: &'static str,
    pub ff_bits: u64,
    pub sdp4_cells: u64,
    /// Public framework's conservative 16x1 composition accounting.
    pub ram16x1_cells: u64,
    pub bsram: usize,
    pub hard_pipeline_bits: u64,
    pub ports: &'static str,
}
#[derive(Clone, Debug)]
pub struct Inventory {
    pub rows: Vec<Row>,
    pub ff_bits: u64,
    pub sdp4_cells: u64,
    pub ram16x1_cells: u64,
    pub bsram: usize,
    pub hard_pipeline_bits: u64,
    /// Bit-wide 2:1 tree nodes for one read selector per rotating FF bank.
    /// This topology demand is not a fitted Logic count. Distinct consumer
    /// phases, write decode and arithmetic muxes are additional.
    pub rotating_read_mux_bits: u64,
    pub rotating_write_mux_bits: u64,
    pub storage_control_boolean_gates: u64,
    pub operand_mux_tree_bits: u64,
    pub dsp18: usize,
}
fn ff(name: &'static str, bits: u64, ports: &'static str) -> Row {
    Row {
        name,
        ff_bits: bits,
        sdp4_cells: 0,
        ram16x1_cells: 0,
        bsram: 0,
        hard_pipeline_bits: 0,
        ports,
    }
}
fn ram(name: &'static str, width: u64, depth: u64, ports: &'static str) -> Row {
    let banks = depth.max(16).next_power_of_two().div_ceil(16);
    Row {
        name,
        ff_bits: 0,
        sdp4_cells: width.div_ceil(4) * banks,
        ram16x1_cells: width * banks,
        bsram: 0,
        hard_pipeline_bits: 0,
        ports,
    }
}
fn stage(name: &'static str, p: &StagePlan, packed: bool) -> Row {
    let mut r = ff(
        name,
        if packed {
            p.packed.ff_bits as u64
        } else {
            p.fixed_ff_bits
        },
        "one write/physical FF/edge; continuous FF fanout; periodic read selection",
    );
    r.hard_pipeline_bits = p.dsp_pipeline_bits;
    r
}
pub fn describe(
    b: &Binding,
    p: control::Hardware,
    c: &timed::Hardware,
) -> Result<Inventory, String> {
    p.validate()?;
    if c.packet_storage == timed::PacketStorage::Pool64 && p.packet_credits != 16 {
        return Err("pool inventory fixed producer16".into());
    }
    c.validate()
        .map_err(|e| format!("inventory cache: {e:?}"))?;
    if c.prefetch || c.preparation != timed::PreparationMode::BoundStages {
        return Err("inventory requires bound, no-prefetch composition".into());
    }
    let stages = [
        (&b.derivative, "derivative FF"),
        (&b.lod, "LOD FF"),
        (&b.coordinate, "coordinate FF"),
        (&b.coefficient, "coefficient FF"),
        (&b.plane, "membership FF"),
        (&b.packet, "packet FF"),
    ];
    let packed = p.storage == control::Storage::Packed;
    let mut rows: Vec<_> = stages
        .iter()
        .map(|(p, name)| stage(name, p, packed))
        .collect();
    if packed {
        rows.push(ff("periodic storage phase/valid", stages.iter().map(|(p, _)| p.packed.control_ff_bits).sum(),
            "six independent CE-gated one-hot phase rings and fixed-delay issue-valid shift registers"));
    }
    // Separate width rounding per ROM, not ceil(sum(width)/4).
    rows.push(ram(
        "LOD log ROM",
        8,
        64,
        "one indexed read/8 enabled clocks; registered output",
    ));
    rows.push(ram(
        "mip prefix ROM",
        13,
        11,
        "one indexed read/8 enabled clocks; registered output",
    ));
    if rows.iter().map(|r| r.ram16x1_cells).sum::<u64>() != b.lod.rom_ram16_cells as u64 {
        return Err("ROM accounting disagrees with binding".into());
    }
    rows.push(ff(
        "shared contexts",
        387 * p.contexts as u64,
        "admission/D/LOD update distinct fields; oldest lane read; slot reuse after capture",
    ));
    let coord_live = (b.coordinate.span() + 1)
        .div_ceil(b.coordinate.ii())
        .min(p.coordinate_credits as u64);
    rows.push(ff(
        "coordinate pass-through",
        37 * coord_live,
        "parents18 + mip8 + slot/quad8 + lane2 + final1; issue/capture",
    ));
    rows.push(ff(
        "coordinate ready records",
        150 * p.coordinate_credits as u64,
        "one write/2 clocks, one read/2 clocks; independent bounded ready FIFO",
    ));
    let coef_live = (b.coefficient.span() + 1).div_ceil(b.coefficient.ii());
    rows.push(ff(
        "coefficient pass-through",
        99 * coef_live,
        "coords80 + mip8 + slot/quad8 + lane2 + final1; issue/capture",
    ));
    rows.push(ff(
        "coefficient ready records",
        171 * 2,
        "one write/2 clocks, one plane read/clock; two bounded records",
    ));
    rows.push(ram(
        "plane work records",
        94,
        p.work_credits as u64,
        "one append/clock, one head read/clock; cursor2 updated in separate head FF",
    ));
    // Keep the RAM row immutable while its head is expanded; cursor is cached.
    rows.push(ff(
        "plane head cursor",
        3,
        "cursor2 + valid1; consumer only",
    ));
    if c.packet_storage == timed::PacketStorage::Native {
        rows.push(ff(
            "packet holding records",
            72 * p.packet_credits as u64,
            "one append/clock, one emit/clock; includes pipeline/output reservations",
        ));
    }
    rows.push(ff(
        "prep completion/live",
        16 * (1 + 6 + 3) + 16,
        "ID indexed; packets-left6, original context3, valid; early context release",
    ));
    // Tokens follow fixed stage phases; issue time in Rust is a harness clock.
    let token_slots = coord_live + coef_live + b.plane.span() + 2 + b.packet.span() + 2;
    let ctrl = 8 * token_slots + 20 * p.contexts as u64 + 96;
    rows.push(ff(
        "prep phase/queue control",
        ctrl,
        "quad4/lane2/plane1/valid1 pipeline; order, lane cursors, pointers, credits/phases",
    ));
    rows.push(ram(
        "cache way tags",
        18,
        64,
        "four way read slices; one allocation write; READY same-edge sink",
    ));
    rows.push(ff(
        "cache line control",
        128 + 48 + 64,
        "state2/line, PLRU3/set, reservation1/line",
    ));
    rows.push(ff(
        "slot table",
        16 * 64,
        "fixed ABI allocation incl reserved bits; drained rebind; head read",
    ));
    rows.push(ff(
        "miss directory/active",
        4 * (22 + 32 + 6 + 1) + 64 + 6 + 12,
        "four incl active; key/addr/line/priority; external request ID64; beat/start/control",
    ));
    if c.preparation == timed::PreparationMode::BoundStages {
        rows.push(ff(
            "admitted quad slot",
            16 * 4,
            "actual slot binding survives prep release until cache/color quad retirement",
        ));
    }
    if c.packet_storage == timed::PacketStorage::Native {
        rows.push(ram(
            "Group4 FIFO",
            72,
            c.group_capacity as u64,
            "one write + independent head read/clock; distinct-row read/write",
        ));
    } else {
        rows.push(Row{name:"packet payload banks",ff_bits:0,sdp4_cells:0,ram16x1_cells:0,bsram:2,hard_pipeline_bits:0,ports:"2x512x36 SDP; use64 rows only; one W and one R; paid macro output holds pending data"});
        rows.push(ff("packet producer descriptors",16*(6+4+2+1),"conservative FF ring: row6/quad4/lane2/published1; reservation W and publication bit updates"));
        rows.push(ff(
            "packet Group descriptors",
            32 * 6,
            "conservative FF ring: row6; includes pending/head in logical32 credit",
        ));
        rows.push(ff("packet two heads",2*(72+1),"two owned payloads/valids; consumer selects old valid only; 72+2 bit mux nodes additional"));
        rows.push(ff(
            "packet pending/pointers",
            8 + 2,
            "pending row6/target1/valid1; read and consume head pointers; no third soft72 payload",
        ));
        rows.push(ff(
            "packet index front fields",
            12 + 6,
            "conservative bounded descriptor head capture; neither is a packet payload",
        ));
        rows.push(ff("packet ring/credit control",5*6+5+6+6+5+2*4+2*5,"allocate/reclaim/write/transfer/read row pointers6; P/G/row/written counts; index ring pointers; counter/compare/decode needs lowering"));
    }
    // Conservatively retain every optional field of the Rust color token.
    let depth = c.color_latency();
    rows.push(ff(
        "color tokens",
        (72 + 64 + 51 + 51 + 6 + 4 + 4) * depth,
        "packet/word/partial/sum/line/age/valid; shift/read/hold; dedicated allocation",
    ));
    rows.push(ff(
        "color tree/feedback/normalize",
        153 + 57 + 51,
        "registered add tree and same-pixel feedback; exact /511",
    ));
    rows.push(ff(
        "result FIFO",
        30 * c.result_capacity as u64,
        "RGB24 + quad4 + lane2; one push/pop; output backpressure",
    ));
    rows.push(ff(
        "cache queue/quad control",
        16 * (4 + 1 + 6) + 16 + 64,
        "remaining lane-mask4 + produced1 + validation cursor6/ID; live; queue pointers/credits/fault",
    ));
    rows.push(Row {
        name: "cache data",
        ff_bits: 0,
        sdp4_cells: 0,
        ram16x1_cells: 0,
        bsram: 4,
        hard_pipeline_bits: 0,
        ports: "4x1024x18 configured, 16 used; registered read1 + independent refill write",
    });
    rows.push(Row {
        name: "color DSP pipeline",
        ff_bits: 0,
        sdp4_cells: 0,
        ram16x1_cells: 0,
        bsram: 0,
        hard_pipeline_bits: 12 * 17 * c.multiply_latency,
        ports: "12 independent 9x8 sites; packed with coefficient sites into 8 DSP18",
    });
    let rotating_read_mux_bits = if packed {
        stages
            .iter()
            .map(|(p, _)| p.packed.read_selector_tree_bits)
            .sum()
    } else {
        stages
            .iter()
            .flat_map(|(p, _)| &p.ff_banks)
            .map(|bank| u64::from(bank.width) * bank.slots.saturating_sub(1))
            .sum()
    };
    Ok(Inventory {
        ff_bits: rows.iter().map(|r| r.ff_bits).sum(),
        sdp4_cells: rows.iter().map(|r| r.sdp4_cells).sum(),
        ram16x1_cells: rows.iter().map(|r| r.ram16x1_cells).sum(),
        bsram: rows.iter().map(|r| r.bsram).sum(),
        hard_pipeline_bits: rows.iter().map(|r| r.hard_pipeline_bits).sum(),
        rotating_read_mux_bits,
        rotating_write_mux_bits: if packed {
            stages
                .iter()
                .map(|(p, _)| p.packed.write_selector_tree_bits)
                .sum()
        } else {
            0
        },
        storage_control_boolean_gates: if packed {
            stages
                .iter()
                .map(|(p, _)| p.packed.control_boolean_gates)
                .sum()
        } else {
            0
        },
        operand_mux_tree_bits: stages.iter().map(|(p, _)| p.operand_mux_tree_bits).sum(),
        dsp18: 8,
        rows,
    })
}
