//! Private configuration-matched allocation, not the legacy Session replay
//! certificate. D/LOD use actual packed registers in both storage policies;
//! Each live arithmetic owner replaces the corresponding replay allocation.
use super::{
    control,
    inventory::{self, Inventory, Row},
    Binding,
};
use crate::texture::{
    emu::{
        coefficient::compact_stage as coefficient, color, context_store, coordinate,
        coordinate_store, derivative, lod, packet_store_ordered as packet_store, uv_store,
    },
    sim::timed,
};
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
pub(super) fn describe(
    b: &Binding,
    p: control::Hardware,
    c: &timed::Hardware,
) -> Result<Inventory, String> {
    let mut r = inventory::describe(b, p, c)?;
    r.rows.retain(|row| {
        !matches!(
            row.name,
            "packet producer descriptors"
                | "packet Group descriptors"
                | "packet two heads"
                | "packet pending/pointers"
                | "packet index front fields"
                | "packet ring/credit control"
        )
    });
    let payload = r
        .rows
        .iter_mut()
        .find(|row| row.name == "packet payload banks")
        .ok_or("Runtime packet pool allocation")?;
    payload.hard_pipeline_bits = 72;
    payload.ports = "two native512x36 banks; logical64 rows, one W/one synchronous R, retained hard DO under CE0";
    r.rows.push(ff(
        "packet owned payload heads",
        packet_store::HEAD_FF_BITS,
        "two72-bit owned returns only; no descriptor row arrays or owner payload shadow",
    ));
    r.rows.push(ff("packet ordered control",packet_store::CONTROL_FF_BITS,
        "five row cursors, P16/G32/row48/unwritten counts, two owned heads/pending and terminal fault; no owner RAM/publication bitmap"));

    // The live cache retains a single one-edge read, never the independent
    // timed reference's numerical Color or its result-credit allocation.
    r.rows.retain(|row| {
        !matches!(
            row.name,
            "color tokens" | "color tree/feedback/normalize" | "result FIFO" | "color DSP pipeline"
        )
    });
    r.rows.push(ff(
        "cache pending read",
        72 + 6 + 1,
        "packet72/line6/valid1; issue to next enabled capture; 1R/1W, pin released at capture",
    ));
    // The ordered capture has one live refill owner. The queued reference
    // allocation is a separate policy, never four descriptors in Runtime.
    let demand = r
        .rows
        .iter_mut()
        .find(|row| row.name == "miss directory/active")
        .ok_or("Runtime demand allocation")?;
    demand.ff_bits = 22 + 32 + 6 + 64 + 2 + 5 + 1;
    demand.ports = "one key22/address32/line6 descriptor; request ID64, phase2, next beat5, valid1; model ceiling, not a native RTL fit";
    // Remove the reference's twelve 9x8 multiplier sites.
    r.dsp18 -= 6;
    // These are alternative Runtime allocations, not storage added alongside
    // a Session replay cursor. Runtime never uses that legacy cursor/table.
    let completion = r
        .rows
        .iter_mut()
        .find(|row| row.name == "prep completion/live")
        .ok_or("Runtime completion allocation")?;
    completion.ports = "actual remaining mask4 replaces packets-left6/ID within retained ceiling; original context3/valid1 and live16";
    let cache_control = r
        .rows
        .iter_mut()
        .find(|row| row.name == "cache queue/quad control")
        .ok_or("Runtime live packet validation allocation")?;
    cache_control.ports = "remaining4/produced1; old cursor6 replaced by packet remaining4/open1/live1 per ID; cache live16 and queue/control64 retained";
    let a = (usize::BITS - (p.work_credits - 1).leading_zeros()) as u64;
    let q = (usize::BITS - p.work_credits.leading_zeros()) as u64;
    let ca = (usize::BITS - (p.contexts - 1).leading_zeros()) as u64;
    let cq = (usize::BITS - p.contexts.leading_zeros()) as u64;
    r.rows.retain(|row| {
        !matches!(
            row.name,
            "shared contexts" | "coordinate pass-through" | "coordinate ready records"
        )
    });
    r.rows.push(Row {
        name: "context UV SDP36", ff_bits: 0, sdp4_cells: 0, ram16x1_cells: 0,
        bsram: 1, hard_pipeline_bits: 36,
        ports: "one native512x36 block, 4*contexts useful rows; serialized W1 and reserved synchronous C R1; no D writeback",
    });
    r.rows.push(ff("UV shared raw/D and C heads", uv_store::PAYLOAD_FF_BITS,
        "raw144 retained once until both D capture and four36bit RAM writes; C return32 only until old coordinate acceptance"));
    r.rows.push(ff("UV RAM controller", uv_store::CONTROL_FF_BITS,
        "raw slot3/write-index3/taken1/valid1; C slot3/lane2/reserved1/ready1; pending1; next-wall capture on CE0"));
    r.rows.push(Row { name: "context stage metadata SRAM", ff_bits: 0,
        sdp4_cells: context_store::runtime::SDP4_CELLS, ram16x1_cells: context_store::runtime::RAM16_BITS,
        bsram: 0, hard_pipeline_bits: 0,
        ports: "Raw36/Ready decision26 padded28 fixed16-row banks; each1W/1 async R; Derived53 borrows D terminal bank through next phase0 old identity read; no Derived RAM or payload head" });
    r.sdp4_cells += context_store::runtime::SDP4_CELLS;
    r.ram16x1_cells += context_store::runtime::RAM16_BITS;
    r.rows.push(ff("ordered context control", 5*ca + 4*cq + 2*(ca+1) + 4*p.contexts as u64 + 2 + 1,
        "five modulo-N heads, four counts, two owner ptr/valid, per-row state2/valid1/reclaim1, global C-head ordinal2 and terminal fault1; actual Context8 queue RTL has74 bits; no absolute time or searches"));
    r.rows.push(Row { name: "Coordinate split SRAM banks", ff_bits: 0,
        sdp4_cells: coordinate_store::COMPACT_SDP4_CELLS, ram16x1_cells: coordinate_store::COMPACT_RAM16_BITS,
        bsram: 0, hard_pipeline_bits: 0,
        ports: "16x40 owner (38 useful) and16x36 fine-only return; each bank1W/1 asynchronous R; no FF payload head",
    });
    r.rows.push(ff("Coordinate store control", coordinate_store::control_bits(p.coordinate_credits),
        "three modulo-capacity pointers plus occupied/ready counts; no per-row valid flags or duplicate counters"));
    r.sdp4_cells += coordinate_store::COMPACT_SDP4_CELLS;
    r.ram16x1_cells += coordinate_store::COMPACT_RAM16_BITS;
    let work = r
        .rows
        .iter_mut()
        .find(|row| row.name == "plane work records")
        .ok_or("compact Work allocation")?;
    let banks = (p.work_credits as u64).div_ceil(16);
    r.sdp4_cells -= work.sdp4_cells;
    r.ram16x1_cells -= work.ram16x1_cells;
    work.sdp4_cells = 78_u64.div_ceil(4) * banks;
    work.ram16x1_cells = 78 * banks;
    work.ports="immutable compact Member76/cursor2; one asynchronous read and one full write; two ordered heads";
    r.sdp4_cells += work.sdp4_cells;
    r.ram16x1_cells += work.ram16x1_cells;
    // Keep the former generic control ceiling for downstream machinery until
    // its own sequential lowering is audited; new owners above are explicit.
    for (name, data, control) in [
        (
            "derivative FF",
            derivative::NUMERIC_BITS,
            derivative::CONTROL_BITS,
        ),
        ("LOD FF", lod::NUMERIC_BITS, lod::CONTROL_BITS),
        (
            "coordinate FF",
            coordinate::compact_axis::SHARED_NUMERIC_BITS,
            coordinate::CONTROL_BITS,
        ),
    ] {
        let row = r
            .rows
            .iter_mut()
            .find(|row| row.name == name)
            .ok_or("Runtime D/LOD allocation")?;
        row.ff_bits = data as u64;
        row.ports = if name == "coordinate FF" {
            "V input16/config16/joint fine result36; one axis kernel, no U payload shadow, independent CE phase/valid"
        } else {
            "actual certified packed scalar bank; independent CE phase/valid"
        };
        r.rows.push(ff(
            match name {
                "derivative FF" => "actual derivative phase/valid",
                "LOD FF" => "actual LOD phase/valid",
                _ => "actual coordinate phase/valid",
            },
            control as u64,
            "fixed reset phase; every register holds on CE0",
        ));
    }
    r.rows
        .retain(|row| row.name != "periodic storage phase/valid");
    let coefficient_row = r
        .rows
        .iter_mut()
        .find(|r| r.name == "coefficient FF")
        .ok_or("Runtime coefficient allocation")?;
    coefficient_row.ff_bits = coefficient::NUMERIC_BITS as u64;
    coefficient_row.ports = "fixed input37/horizontal17/row18/fine36 stages; local parity/valid/DSP CE; no calendar bank";
    coefficient_row.hard_pipeline_bits = coefficient::ALLOCATION.hard_product_bits as u64;
    for (name, bits) in [
        (
            "coefficient pass-through",
            coefficient::ALLOCATION.metadata_bits,
        ),
        (
            "coefficient ready records",
            coefficient::ALLOCATION.ready_bits,
        ),
    ] {
        r.rows
            .iter_mut()
            .find(|row| row.name == name)
            .ok_or("coefficient retained owner")?
            .ff_bits = bits as u64;
    }
    let metadata = r
        .rows
        .iter_mut()
        .find(|row| row.name == "coefficient pass-through")
        .ok_or("coefficient metadata BSRAM")?;
    metadata.bsram = coefficient::ALLOCATION.metadata_bsram;
    metadata.hard_pipeline_bits = coefficient::ALLOCATION.metadata_hard_bits as u64;
    metadata.ports = "admission W1; synchronous head R1 on publication or pop; retained until consumption; no metadata copy in ready weights";
    r.bsram += metadata.bsram;
    let ready = r
        .rows
        .iter_mut()
        .find(|row| row.name == "coefficient ready records")
        .ok_or("coefficient ready row")?;
    ready.sdp4_cells = coefficient::READY_SDP4_CELLS as u64;
    ready.ram16x1_cells = coefficient::READY_RAM16_BITS as u64;
    ready.ports = "two explicit72-bit FF rows; age4 W1, asynchronous old head R1; age5 control publication, age6 consumption; no FF head";
    r.sdp4_cells += ready.sdp4_cells;
    r.ram16x1_cells += ready.ram16x1_cells;
    r.rows.push(ff(
        "actual coefficient phase/valid",
        coefficient::ALLOCATION.phase_valid_bits as u64,
        "actual coefficient phase/valid; downstream base CE independent",
    ));
    // Runtime selects the elaborated short leaves. Removed alignment banks
    // are absent from the inventory and are never updated in the emulator.
    for (name, used) in [
        (
            "membership FF",
            super::serial_rtl::MEMBERSHIP_SHORT_DATA_BITS,
        ),
        ("packet FF", super::serial_rtl::PACKET_SHORT_DATA_BITS),
    ] {
        let row = r
            .rows
            .iter_mut()
            .find(|row| row.name == name)
            .ok_or("Runtime numerical allocation")?;
        row.ff_bits = used as u64;
        row.ports = "actual compact arithmetic registers; intermediate payload copies and alignment tail removed";
    }
    r.rows.push(ff(
        "actual membership valid/fault",
        super::serial_rtl::MEMBERSHIP_SHORT_CONTROL_BITS as u64,
        "compact Common72/Member76; valid2 + fault1; base CE",
    ));
    r.rows.push(ff(
        "actual packet valid/fault",
        super::serial_rtl::PACKET_SHORT_CONTROL_BITS as u64,
        "capture77/packet72; valid2 + fault1; reservation at actual E0, ordered W at E2; shared effective CE",
    ));
    r.rows.push(ff(
        "actual coefficient queue control",
        coefficient::ALLOCATION.queue_control_bits as u64,
        "retained separately; generic preparation control not reduced",
    ));
    r.rows.push(ff(
        "actual coefficient terminal fault",
        1,
        "standalone block owner retained",
    ));
    r.rows.push(ff(
        "coefficient partial plane cursor",
        1,
        "old ready2 whole-row ownership",
    ));
    r.rows.push(ff(
        "Work return/head payload",
        super::compact_transport::HEAD_DATA_BITS,
        "two fixed ordered banks: asynchronous SSRAM R captures data/valid, next edge consumes",
    ));
    // Replace the legacy single cursor2/valid1 with the complete prefetch ring.
    r.rows.retain(|row| row.name != "plane head cursor");
    r.rows.push(ff(
        "Work prefetch control",
        super::compact_transport::HEAD_CONTROL_BITS,
        "two cursor2/valid1; head read/fill pointers1; occupancy2; no extra return age",
    ));
    r.rows.push(ff(
        "Work read/write row pointers",
        3 * a,
        "modulo logical W: write, prefetch, and source-release held until ACK",
    ));
    r.rows.push(ff(
        "Work materialized count",
        q,
        "includes pending/valid source row; separate from central reservations",
    ));
    let mut actual_color = ff(
        "actual ColorEmu",
        (color::ALLOCATION.datapath_ff_bits + color::ALLOCATION.result_and_control_ff_bits) as u64,
        "sole live color arithmetic/result-credit owner; no cache color shadow",
    );
    actual_color.hard_pipeline_bits =
        (color::ALLOCATION.hard_product_bits + color::ALLOCATION.result_output_hard_bits) as u64;
    actual_color.bsram = color::ALLOCATION.bsram;
    actual_color.sdp4_cells = color::ALLOCATION.sdp4_cells as u64;
    actual_color.ram16x1_cells = 4 * actual_color.sdp4_cells;
    r.sdp4_cells += actual_color.sdp4_cells;
    r.ram16x1_cells += actual_color.ram16x1_cells;
    r.rows.push(actual_color);
    r.rows.push(ff(
        "Runtime link",
        super::runtime::LINK_STATE_BITS as u64,
        "capture137, Runtime terminal fault1; no public/accepted mask copies",
    ));
    r.ff_bits = r.rows.iter().map(|r| r.ff_bits).sum();
    r.bsram = r.rows.iter().map(|r| r.bsram).sum();
    // Two-way old head payload/cursor selection: one mux node per bit.
    // Target enable/decode is declared control above, not fitted Logic.
    r.rotating_read_mux_bits += 78;
    r.hard_pipeline_bits = r.rows.iter().map(|r| r.hard_pipeline_bits).sum();
    r.dsp18 += color::ALLOCATION.dsp18_equivalents;
    // D/LOD use their packed scalar calendars. Coordinate's numeric row above
    // describes the selected shared-axis registers. Retain generic selector
    // certificates as diagnostic ceilings, independent of Dedicated/Packed;
    // they do not price the selected axis operand mux or imply fitted Logic.
    let live = [&b.derivative, &b.lod, &b.coordinate];
    r.rotating_read_mux_bits = 78
        + live
            .iter()
            .map(|p| p.packed.read_selector_tree_bits)
            .sum::<u64>();
    r.rotating_write_mux_bits = live.iter().map(|p| p.packed.write_selector_tree_bits).sum();
    r.storage_control_boolean_gates = live.iter().map(|p| p.packed.control_boolean_gates).sum();
    r.operand_mux_tree_bits -= b.coefficient.operand_mux_tree_bits;
    Ok(r)
}
