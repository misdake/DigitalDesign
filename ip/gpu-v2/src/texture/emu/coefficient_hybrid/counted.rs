//! Temporary test-only closed counted bodies adapted from staged/bound.rs
//! membership/packet. Only operand ingress changes; no original result Frame
//! drives these bodies. Calendar/lowering are still counted replay, not emu.
use super::{coefficient::Output, Member};
use crate::texture::format::*;
use crate::texture::sim::{counted, staged::Stage};
use audited::{Fault, Fixed, Frame, Model};
type Bit = Fixed<1, 0, false>;
fn raw(p: &Member, name: &str) -> i128 {
    p.raw(name)
}
fn finish(f: Frame<'_>) -> Result<Stage, Fault> {
    let frame = f.finish();
    frame.audit()?;
    Ok(Stage { frame })
}
fn pair_address<const B: u32, const F: u32, const S: bool>(
    s: audited::Memory<B, F, S>,
    i: usize,
) -> audited::Address<B, F, S> {
    match i {
        0 => s.at::<0>(),
        1 => s.at::<1>(),
        2 => s.at::<2>(),
        _ => s.at::<3>(),
    }
}
pub(super) fn membership(output: Output, which: usize) -> Result<Stage, Fault> {
    let mut m = Model::numerical();
    let weights: CoefficientStore = m.input(
        "weights",
        &(0..4)
            .map(|t| i128::from(output.weights[which][t]))
            .collect::<Vec<_>>(),
    )?;
    let coords: TexelCoordinateStore = m.input(
        "coords",
        &(0..4)
            .map(|i| i128::from(output.metadata.coordinates[which][i]))
            .collect::<Vec<_>>(),
    )?;
    let identity = m.input::<4, 0, false>(
        "identity",
        &[
            i128::from(output.metadata.slot),
            i128::from(output.metadata.levels[which]),
            i128::from(output.metadata.key / 4),
        ],
    )?;
    let lane_id = m.input::<2, 0, false>("lane", &[i128::from(output.metadata.key % 4)])?;
    let flags = m.input::<1, 0, false>(
        "flags",
        &[
            i128::from(which == 0),
            i128::from(output.metadata.last_fine),
        ],
    )?;
    let f = m.compute("texture_membership", 256)?;
    let a = f.read(coords.at::<0>())?;
    let b = f.read(coords.at::<1>())?;
    let c0 = f.read(coords.at::<2>())?;
    let d = f.read(coords.at::<3>())?;
    let tx = [f.slice::<7, 0, false, 3>(a)?, f.slice::<7, 0, false, 3>(b)?];
    let ty = [
        f.slice::<7, 0, false, 3>(c0)?,
        f.slice::<7, 0, false, 3>(d)?,
    ];
    let same_x = counted::eq(&f, tx[0], tx[1])?;
    let same_y = counted::eq(&f, ty[0], ty[1])?;
    let same_xy = f.select(same_x, same_y, Bit::constant::<0>())?;
    let same = [Bit::constant::<1>(), same_x, same_y, same_xy];
    let mut nonzero = [Bit::constant::<0>(); 4];
    for (t, v) in nonzero.iter_mut().enumerate() {
        let w = f.read(pair_address(weights, t))?;
        f.publish(&format!("w{t}"), w)?;
        *v = counted::not(&f, counted::eq(&f, w, Coefficient::constant::<0>())?)?;
    }
    for t in 0..4 {
        let mut emit = nonzero[t];
        for j in 0..t {
            emit = f.select(
                f.select(same[t ^ j], nonzero[j], Bit::constant::<0>())?,
                Bit::constant::<0>(),
                emit,
            )?;
        }
        f.publish(&format!("emit{t}"), emit)?;
    }
    for i in 0..2 {
        f.publish(&format!("tx{i}"), tx[i])?;
        f.publish(&format!("ty{i}"), ty[i])?;
    }
    f.publish("lx", f.slice::<3, 0, false, 0>(a)?)?;
    f.publish("ly", f.slice::<3, 0, false, 0>(c0)?)?;
    f.publish("same_x", same_x)?;
    f.publish("same_y", same_y)?;
    f.publish("slot", f.read(identity.at::<0>())?)?;
    f.publish("n", f.read(identity.at::<1>())?)?;
    f.publish("quad", f.read(identity.at::<2>())?)?;
    f.publish("lane", f.read(lane_id.at::<0>())?)?;
    let fine = f.read(flags.at::<0>())?;
    f.publish("fine", fine)?;
    f.publish(
        "final",
        f.select(fine, f.read(flags.at::<1>())?, Bit::constant::<1>())?,
    )?;
    finish(f)
}
pub(super) fn packet(p: &Member, tap: usize) -> Result<Stage, Fault> {
    let mut m = Model::numerical();
    let weights: CoefficientStore = m.input(
        "weights",
        &(0..4).map(|i| raw(p, &format!("w{i}"))).collect::<Vec<_>>(),
    )?;
    let tiles = m.input::<7, 0, false>(
        "tiles",
        &[raw(p, "tx0"), raw(p, "tx1"), raw(p, "ty0"), raw(p, "ty1")],
    )?;
    let local = m.input::<3, 0, false>("local", &[raw(p, "lx"), raw(p, "ly")])?;
    let meta = m.input::<4, 0, false>("meta", &[raw(p, "slot"), raw(p, "n"), raw(p, "quad")])?;
    let lane = m.input::<2, 0, false>("lane", &[raw(p, "lane"), tap as i128])?;
    let flags = m.input::<1, 0, false>(
        "flags",
        &[
            raw(p, "same_x"),
            raw(p, "same_y"),
            raw(p, "fine"),
            raw(p, "final"),
        ],
    )?;
    let emit = m.input::<1, 0, false>(
        "emit",
        &(0..4)
            .map(|i| raw(p, &format!("emit{i}")))
            .collect::<Vec<_>>(),
    )?;
    let f = m.compute("texture_packet", 384)?;
    let tap = f.read(lane.at::<1>())?;
    let x = f.slice::<1, 0, false, 0>(tap)?;
    let y = f.slice::<1, 0, false, 1>(tap)?;
    let e: [Bit; 4] = [
        f.read(emit.at::<0>())?,
        f.read(emit.at::<1>())?,
        f.read(emit.at::<2>())?,
        f.read(emit.at::<3>())?,
    ];
    let valid = f.select(y, f.select(x, e[3], e[2])?, f.select(x, e[1], e[0])?)?;
    f.require::<true>(valid)?;
    let sx = f.read(flags.at::<0>())?;
    let sy = f.read(flags.at::<1>())?;
    let fine = f.read(flags.at::<2>())?;
    let mut last = f.read(flags.at::<3>())?;
    for (i, active) in e.iter().enumerate().skip(1) {
        let greater = match i {
            1 => f.less(tap, Fixed::<2, 0, false>::constant::<1>())?,
            2 => f.less(tap, Fixed::<2, 0, false>::constant::<2>())?,
            _ => f.less(tap, Fixed::<2, 0, false>::constant::<3>())?,
        };
        last = f.select(
            f.select(greater, *active, Bit::constant::<0>())?,
            Bit::constant::<0>(),
            last,
        )?;
    }
    let first = f.select(
        x,
        Bit::constant::<0>(),
        f.select(y, Bit::constant::<0>(), fine)?,
    )?;
    let mut word = GroupWord::constant::<0>();
    word = counted::pack_field::<0>(&f, word, f.read(meta.at::<0>())?)?;
    word = counted::pack_field::<4>(&f, word, f.read(meta.at::<1>())?)?;
    word = counted::pack_field::<8>(
        &f,
        word,
        f.select(x, f.read(tiles.at::<1>())?, f.read(tiles.at::<0>())?)?,
    )?;
    word = counted::pack_field::<15>(
        &f,
        word,
        f.select(y, f.read(tiles.at::<3>())?, f.read(tiles.at::<2>())?)?,
    )?;
    word = counted::pack_field::<22>(&f, word, f.read(local.at::<0>())?)?;
    word = counted::pack_field::<25>(&f, word, f.read(local.at::<1>())?)?;
    for i in 0..4 {
        let mx = if i & 1 == 0 {
            f.select(x, sx, Bit::constant::<1>())?
        } else {
            f.select(x, Bit::constant::<1>(), sx)?
        };
        let my = if i & 2 == 0 {
            f.select(y, sy, Bit::constant::<1>())?
        } else {
            f.select(y, Bit::constant::<1>(), sy)?
        };
        let w = f.select(
            f.select(mx, my, Bit::constant::<0>())?,
            f.read(pair_address(weights, i))?,
            Coefficient::constant::<0>(),
        )?;
        word = match i {
            0 => counted::pack_field::<28>(&f, word, w)?,
            1 => counted::pack_field::<37>(&f, word, w)?,
            2 => counted::pack_field::<46>(&f, word, w)?,
            _ => counted::pack_field::<55>(&f, word, w)?,
        };
    }
    word = counted::pack_field::<64>(&f, word, first)?;
    word = counted::pack_field::<65>(&f, word, last)?;
    word = counted::pack_field::<66>(&f, word, f.read(meta.at::<2>())?)?;
    word = counted::pack_field::<70>(&f, word, f.read(lane.at::<0>())?)?;
    f.publish("packet", word)?;
    finish(f)
}
