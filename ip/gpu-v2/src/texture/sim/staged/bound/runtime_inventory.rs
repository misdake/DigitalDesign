//! Private configuration-matched allocation, not the legacy Session replay
//! certificate. D/LOD use actual packed registers in both storage policies;
//! Dedicated phase/valid fits within its retained conservative data ceiling.
use super::{
    control,
    inventory::{self, Inventory, Row},
    Binding,
};
use crate::texture::{
    emu::{coefficient, color, coordinate, derivative, lod},
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
    let a = (usize::BITS - (p.work_credits - 1).leading_zeros()) as u64;
    let q = (usize::BITS - p.work_credits.leading_zeros()) as u64;
    // Nearest outlives its scalar flag's last arithmetic read and must reach
    // the coefficient input. The old37-bit pass-through omitted that bit.
    let coord_live = (b.coordinate.span() + 1)
        .div_ceil(b.coordinate.ii())
        .min(p.coordinate_credits as u64);
    let coordinate_owner = r
        .rows
        .iter_mut()
        .find(|row| row.name == "coordinate pass-through")
        .ok_or("Runtime coordinate pending owner allocation")?;
    coordinate_owner.ff_bits += coord_live;
    coordinate_owner.ports =
        "actual38: parents18 + levels8 + slot4/key6 + nearest1/final1; fixed ordered owner ring";
    for (name, data, control) in [
        (
            "derivative FF",
            derivative::NUMERIC_BITS,
            derivative::CONTROL_BITS,
        ),
        ("LOD FF", lod::NUMERIC_BITS, lod::CONTROL_BITS),
        (
            "coordinate FF",
            coordinate::NUMERIC_BITS,
            coordinate::CONTROL_BITS,
        ),
    ] {
        let row = r
            .rows
            .iter_mut()
            .find(|row| row.name == name)
            .ok_or("Runtime D/LOD allocation")?;
        let required = data
            + if p.storage == control::Storage::Dedicated {
                control
            } else {
                0
            };
        if row.ff_bits < required as u64 {
            return Err("Runtime D/LOD register map exceeds retained ceiling".into());
        }
        row.ports = if p.storage == control::Storage::Dedicated {
            "actual certified packed scalar bank and phase/valid suballocated within Dedicated ceiling; no extra FF"
        } else {
            "actual certified packed scalar bank; phase/valid in existing periodic row"
        };
    }
    let coefficient_row = r
        .rows
        .iter_mut()
        .find(|r| r.name == "coefficient FF")
        .ok_or("Runtime coefficient allocation")?;
    coefficient_row.ff_bits = coefficient::NUMERIC_BITS as u64;
    coefficient_row.ports = "actual local phase/valid/products; no global replay certificate";
    if p.storage == control::Storage::Packed {
        let phases = r
            .rows
            .iter_mut()
            .find(|r| r.name == "periodic storage phase/valid")
            .ok_or("Runtime counted phase allocation")?;
        phases.ff_bits -= b.coefficient.packed.control_ff_bits;
    }
    r.rows.push(ff(
        "actual coefficient phase/valid",
        19,
        "qualified local phase8/valid11; downstream base CE independent",
    ));
    // These fixed local register maps replace only this Runtime's two counted
    // stage owners. Retain their conservative data ceilings and generic control.
    for (name, used) in [
        ("membership FF", super::runtime_membership::DATA_BITS as u64),
        ("packet FF", super::runtime_packet::DATA_BITS as u64),
    ] {
        let row = r
            .rows
            .iter_mut()
            .find(|row| row.name == name)
            .ok_or("Runtime numerical allocation")?;
        if row.ff_bits < used {
            return Err("Runtime numerical field map exceeds data ceiling".into());
        }
        row.ports = "actual fixed scalar registers; conservative data ceiling retained; no replay certificate";
    }
    if p.storage == control::Storage::Packed {
        let phases = r
            .rows
            .iter_mut()
            .find(|row| row.name == "periodic storage phase/valid")
            .ok_or("Runtime numerical phase allocation")?;
        phases.ff_bits -= b.plane.packed.control_ff_bits + b.packet.packed.control_ff_bits;
        r.rows.push(ff(
            "actual membership retained control allowance",
            7,
            "paid separately from valid7/fault1",
        ));
        r.rows.push(ff(
            "actual packet retained control allowance",
            6,
            "paid separately from valid9/fault1",
        ));
    }
    r.rows.push(ff(
        "actual membership valid/fault",
        super::runtime_membership::CONTROL_BITS as u64,
        "valid7 + separately owned terminal fault1; base CE",
    ));
    r.rows.push(ff(
        "actual packet valid/fault",
        super::runtime_packet::CONTROL_BITS as u64,
        "valid9 + separately owned terminal fault1; base CE",
    ));
    r.rows.push(ff(
        "actual coefficient queue control",
        13,
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
        92,
        "sole bank sampled at R and held through last packet capture",
    ));
    r.rows.push(ff(
        "Work pending return",
        1,
        "reserve at R, publish existing valid on later C",
    ));
    r.rows.push(ff(
        "Work read/write row pointers",
        2 * a,
        "modulo logical W, source read pointer held until ACK",
    ));
    r.rows.push(ff(
        "Work materialized count",
        q,
        "includes pending/valid source row; separate from central reservations",
    ));
    let mut actual_color = ff(
        "actual ColorEmu",
        (color::ALLOCATION.datapath_ff_bits + color::ALLOCATION.result_and_control_ff_bits) as u64,
        "existing actual color/result owner; shadow retained",
    );
    actual_color.hard_pipeline_bits = color::ALLOCATION.hard_product_bits as u64;
    r.rows.push(actual_color);
    r.rows.push(ff(
        "Runtime link",
        super::runtime::LINK_STATE_BITS as u64,
        "capture137, public lanes64, accepted masks64, Runtime terminal fault1",
    ));
    r.ff_bits = r.rows.iter().map(|r| r.ff_bits).sum();
    r.hard_pipeline_bits = r.rows.iter().map(|r| r.hard_pipeline_bits).sum();
    r.dsp18 += color::ALLOCATION.dsp18_equivalents;
    // Old coefficient rotating selector metrics are not a certificate for the
    // actual executor. Preserve only other counted stages' declarations.
    if p.storage == control::Storage::Packed {
        r.rotating_read_mux_bits -= b.coefficient.packed.read_selector_tree_bits;
        r.rotating_write_mux_bits -= b.coefficient.packed.write_selector_tree_bits;
        r.storage_control_boolean_gates -= b.coefficient.packed.control_boolean_gates;
        for stage in [&b.plane, &b.packet] {
            r.rotating_read_mux_bits -= stage.packed.read_selector_tree_bits;
            r.rotating_write_mux_bits -= stage.packed.write_selector_tree_bits;
            r.storage_control_boolean_gates -= stage.packed.control_boolean_gates;
        }
    } else {
        r.rotating_read_mux_bits -= b
            .coefficient
            .ff_banks
            .iter()
            .map(|bank| u64::from(bank.width) * bank.slots.saturating_sub(1))
            .sum::<u64>();
        for stage in [&b.plane, &b.packet] {
            r.rotating_read_mux_bits -= stage
                .ff_banks
                .iter()
                .map(|bank| u64::from(bank.width) * bank.slots.saturating_sub(1))
                .sum::<u64>();
        }
        // Dedicated retains conservative FF ceilings, but actual D/LOD use
        // the certified Packed physical slices. Bill their real steering:
        // read +489, write +1189, Boolean +7407 versus the old Dedicated bill.
        // These are topology demands, not fitted Logic or PnR measurements.
        for stage in [&b.derivative, &b.lod] {
            r.rotating_read_mux_bits -= stage
                .ff_banks
                .iter()
                .map(|bank| u64::from(bank.width) * bank.slots.saturating_sub(1))
                .sum::<u64>();
            r.rotating_read_mux_bits += stage.packed.read_selector_tree_bits;
            r.rotating_write_mux_bits += stage.packed.write_selector_tree_bits;
            r.storage_control_boolean_gates += stage.packed.control_boolean_gates;
        }
    }
    r.operand_mux_tree_bits -= b.coefficient.operand_mux_tree_bits;
    Ok(r)
}
