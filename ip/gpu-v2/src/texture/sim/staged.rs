//! Closed preparation stage boundaries. Timing/logic binding is a separate proof.
//! Stage payloads are finished numerical outputs, never rebuilt oracle values.
use super::super::{format::*, ports::*};
use super::{counted, oracle};
use audited::{Fault, Fixed, Frame, FrameReport, Model};
type Bit = Fixed<1, 0, false>;
type Shift = Fixed<18, 0, true>;
pub mod binding;
pub mod bound;
pub mod stream;

pub struct Stage {
    pub frame: FrameReport,
}
impl Stage {
    pub(super) fn raw(&self, name: &str) -> i128 {
        self.frame
            .outputs
            .iter()
            .find(|o| o.name == name)
            .expect("fixed stage output")
            .raw
    }
    fn finish(f: Frame<'_>) -> Result<Self, Fault> {
        let frame = f.finish();
        frame.audit()?;
        Ok(Self { frame })
    }
}
pub struct Plane {
    pub lane: u8,
    pub which: u8,
    pub frame: FrameReport,
    /// Only real packets, in lowest nonzero tap order.
    pub groups: Vec<Group4>,
    pub payloads: Vec<i128>,
}
pub struct Lane {
    pub lane: u8,
    pub coordinate: Stage,
    pub rows: Stage,
    pub columns: Stage,
    pub planes: Vec<Plane>,
}
pub struct Preparation {
    pub derivative: Stage,
    pub lod: Stage,
    pub lanes: Vec<Lane>,
    pub groups: Vec<Group4>,
    pub payloads: Vec<i128>,
}
fn uv_address(s: UvStore, i: usize) -> audited::Address<18, 16, true> {
    match i {
        0 => s.at::<0>(),
        1 => s.at::<1>(),
        2 => s.at::<2>(),
        3 => s.at::<3>(),
        4 => s.at::<4>(),
        5 => s.at::<5>(),
        6 => s.at::<6>(),
        _ => s.at::<7>(),
    }
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
fn derivatives(q: &QuadInput, slot: Slot) -> Result<Stage, counted::Error> {
    #[cfg(test)]
    bound::counted_call_guard::call(2);
    let (captured, force_coarsest) = capture_uv(q)?;
    let uv: Vec<_> = captured.into_iter().flatten().map(i128::from).collect();
    let mut m = Model::numerical();
    let store: UvStore = m.input("helper_uv", &uv)?;
    let force = m.input::<1, 0, false>("force_coarsest", &[i128::from(force_coarsest)])?;
    let bias: BiasStore = m.input(
        "bias",
        &[(q.lod_bias.clamp(-32.0, 32.0) * 256.0).round_ties_even() as i128],
    )?;
    let meta = m.input::<4, 0, false>(
        "meta",
        &[
            q.quad_id.into(),
            q.mask.into(),
            q.slot.into(),
            slot.max_size_log2.into(),
        ],
    )?;
    let has = m.input::<1, 0, false>("has_mip", &[i128::from(slot.has_full_mip)])?;
    let base: ByteAddressStore = m.input("base", &[slot.base_address.into()])?;
    let filter: FilterCodeStore = m.input(
        "filter",
        &[match q.filter {
            Filter::Nearest => 0,
            Filter::Bilinear => 1,
            Filter::Trilinear => 2,
        }],
    )?;
    let f = m.compute("texture_derivatives", 512)?;
    let mut uvs = [UvRaw::constant::<0>(); 8];
    for (i, v) in uvs.iter_mut().enumerate() {
        *v = f.binary_scale(f.read(uv_address(store, i))?)?;
        f.publish(&format!("uv{i}"), f.slice::<16, 0, false, 0>(*v)?)?;
    }
    let mut mags = [Magnitude::constant::<0>(); 8];
    for (edge, (a, b)) in [(0, 1), (2, 3), (0, 2), (1, 3)].into_iter().enumerate() {
        for axis in 0..2 {
            let d: Difference = f.sub(uvs[b * 2 + axis], uvs[a * 2 + axis])?;
            f.publish(&format!("d{edge}.{axis}"), d)?;
            mags[edge * 2 + axis] = f.resize_exact(f.select(
                f.less(d, Difference::constant::<0>())?,
                f.sub_same(Difference::constant::<0>(), d)?,
                d,
            )?)?;
        }
    }
    // Balanced three-level max tree; no running-maximum feedback across edges.
    for span in [4, 2, 1] {
        for i in 0..span {
            mags[i] = f.select(
                f.less(mags[i * 2], mags[i * 2 + 1])?,
                mags[i * 2 + 1],
                mags[i * 2],
            )?;
        }
    }
    f.publish(
        "slope",
        f.select(
            f.read(force.at::<0>())?,
            Magnitude::constant::<131073>(),
            mags[0],
        )?,
    )?;
    f.publish("bias", f.read(bias.at::<0>())?)?;
    f.publish("quad", f.read(meta.at::<0>())?)?;
    f.publish("mask", f.read(meta.at::<1>())?)?;
    f.publish("slot", f.read(meta.at::<2>())?)?;
    f.publish("max_n", f.read(meta.at::<3>())?)?;
    f.publish("has_mip", f.read(has.at::<0>())?)?;
    f.publish("base", f.read(base.at::<0>())?)?;
    f.publish("filter", f.read(filter.at::<0>())?)?;
    Stage::finish(f).map_err(counted::Error::from)
}
fn lod(d: &Stage) -> Result<Stage, Fault> {
    #[cfg(test)]
    bound::counted_call_guard::call(3);
    let mut m = Model::numerical();
    let slope: MagnitudeStore = m.input("slope", &[d.raw("slope")])?;
    let bias: BiasStore = m.input("bias", &[d.raw("bias")])?;
    let meta = m.input::<4, 0, false>(
        "meta",
        &[d.raw("max_n"), d.raw("quad"), d.raw("mask"), d.raw("slot")],
    )?;
    let has = m.input::<1, 0, false>("has_mip", &[d.raw("has_mip")])?;
    let filter: FilterCodeStore = m.input("filter", &[d.raw("filter")])?;
    let base: ByteAddressStore = m.input("base", &[d.raw("base")])?;
    let log: LogEntryStore = m.table("log2_64", &LOG)?;
    let prefixes: PrefixStore = m.table("mip_prefix", &PREFIX)?;
    let f = m.compute("texture_lod_context", 512)?;
    let s = f.read(slope.at::<0>())?;
    let max_n = f.read(meta.at::<0>())?;
    let has = f.read(has.at::<0>())?;
    let filter = f.read(filter.at::<0>())?;
    let maximum: LogWork = f.shift_left_const::<8, 18, 0, true>(f.resize_exact(f.select(
        has,
        max_n,
        Size::constant::<0>(),
    )?)?)?;
    let overflow = f.less(Magnitude::constant::<131072>(), s)?;
    let zero = counted::eq(&f, s, Magnitude::constant::<0>())?;
    // Substitute one before narrowing so zero/overflow paths never construct an
    // invalid shift or overflowing 20-bit slope. Final guards retain semantics.
    let safe: Slope = f.resize_exact(f.select(
        overflow,
        Magnitude::constant::<1>(),
        f.select(zero, Magnitude::constant::<1>(), s)?,
    )?)?;
    let h: Shift = f.sub_same(Shift::constant::<19>(), f.leading_zeros(safe)?)?;
    let norm: NormalizedShort = f.shift(
        f.resize_exact(safe)?,
        f.sub_same(Shift::constant::<19>(), h)?,
    )?;
    let tail: MantissaShort = f.slice::<19, 13, false, 0>(norm)?;
    let k: TableIndex = f.round_to(tail)?;
    let carry = counted::eq(&f, k, TableIndex::constant::<64>())?;
    let fraction = f.read(log.indexed(f.slice::<6, 0, false, 0>(k)?))?;
    let exponent = f.add_same(
        f.sub_same(h, Shift::constant::<16>())?,
        f.resize_exact(max_n)?,
    )?;
    let integer =
        f.shift_left_const::<8, 18, 0, true>(f.add_same(exponent, f.resize_exact(carry)?)?)?;
    let raw = f.add_same(
        f.add_same(integer, f.resize_exact(fraction)?)?,
        f.resize_exact(f.read(bias.at::<0>())?)?,
    )?;
    let guarded = f.select(
        overflow,
        maximum,
        f.select(
            zero,
            LogWork::constant::<0>(),
            counted::clamp(&f, raw, maximum)?,
        )?,
    )?;
    let lod: Lod = f.resize_exact(guarded)?;
    let level: Size = f.slice::<4, 0, false, 8>(lod)?;
    let fine = f.sub_same(max_n, level)?;
    let coarse_work = f.sub::<5, 0, true>(fine, Size::constant::<1>())?;
    let coarse: Size = f.resize_exact(f.select(
        f.less(coarse_work, Fixed::<5, 0, true>::constant::<0>())?,
        Fixed::<5, 0, true>::constant::<0>(),
        coarse_work,
    )?)?;
    let frac: Fraction = f.slice::<8, 0, false, 0>(lod)?;
    let lambda = f.select(
        counted::eq(&f, filter, FilterCode::constant::<2>())?,
        f.sub_same(
            f.shift_left_const::<1, 9, 0, false>(f.resize_exact(frac)?)?,
            f.resize_exact(f.less(Fraction::constant::<128>(), frac)?)?,
        )?,
        Coefficient::constant::<0>(),
    )?;
    f.publish("lod", lod)?;
    f.publish("lambda", lambda)?;
    f.publish(
        "nearest",
        counted::eq(&f, filter, FilterCode::constant::<0>())?,
    )?;
    f.publish("halve", f.less(Size::constant::<1>(), fine)?)?;
    for (which, n) in [fine, coarse].into_iter().enumerate() {
        let physical = f.select(f.less(n, Size::constant::<1>())?, Size::constant::<1>(), n)?;
        f.publish(&format!("n{which}"), n)?;
        f.publish(
            &format!("side{which}"),
            f.shift(
                IntegerCoordinate::constant::<1>(),
                f.resize_exact(physical)?,
            )?,
        )?;
        f.publish(
            &format!("shift{which}"),
            f.sub::<18, 0, true>(physical, Size::constant::<8>())?,
        )?;
        f.publish(
            &format!("prefix{which}"),
            f.select(has, f.read(prefixes.indexed(n))?, Prefix::constant::<0>())?,
        )?;
    }
    f.publish(
        "parent0",
        f.sub_same(Coefficient::constant::<511>(), lambda)?,
    )?;
    f.publish("parent1", lambda)?;
    f.publish(
        "last_fine",
        counted::eq(&f, lambda, Coefficient::constant::<0>())?,
    )?;
    f.publish("quad", f.read(meta.at::<1>())?)?;
    f.publish("mask", f.read(meta.at::<2>())?)?;
    f.publish("slot", f.read(meta.at::<3>())?)?;
    f.publish("base", f.read(base.at::<0>())?)?;
    Stage::finish(f)
}
fn coordinates(d: &Stage, c: &Stage, lane: usize) -> Result<Stage, Fault> {
    let flag = |name| match c.raw(name) {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Fault::Range),
    };
    coordinate_values(
        [
            u32::try_from(d.raw(&format!("uv{}", lane * 2))).map_err(|_| Fault::Range)?,
            u32::try_from(d.raw(&format!("uv{}", lane * 2 + 1))).map_err(|_| Fault::Range)?,
        ],
        i32::try_from(c.raw("shift0")).map_err(|_| Fault::Range)?,
        flag("nearest")?,
        flag("halve")?,
        [
            i16::try_from(c.raw("side0")).map_err(|_| Fault::Range)?,
            i16::try_from(c.raw("side1")).map_err(|_| Fault::Range)?,
        ],
    )
}
pub(super) fn coordinate_values(
    uv: [u32; 2],
    shift: i32,
    nearest: bool,
    halve: bool,
    side: [i16; 2],
) -> Result<Stage, Fault> {
    #[cfg(test)]
    bound::counted_call_guard::call(4);
    let mut m = Model::numerical();
    let uv = m.input::<16, 0, false>("wrapped_uv", &[i128::from(uv[0]), i128::from(uv[1])])?;
    let shift = m.input::<18, 0, true>("coordinate_shift", &[i128::from(shift)])?;
    let flags = m.input::<1, 0, false>("flags", &[i128::from(nearest), i128::from(halve)])?;
    let side: IntegerCoordinateStore =
        m.input("side", &[i128::from(side[0]), i128::from(side[1])])?;
    let f = m.compute("texture_coordinates", 512)?;
    let nearest = f.read(flags.at::<0>())?;
    let halve = f.read(flags.at::<1>())?;
    let shift = f.read(shift.at::<0>())?;
    for axis in 0..2 {
        let a = if axis == 0 {
            uv.at::<0>()
        } else {
            uv.at::<1>()
        };
        let fine: Coordinate = f.sub_same(
            f.shift(f.resize_exact(f.read(a)?)?, shift)?,
            f.select(
                nearest,
                Coordinate::constant::<0>(),
                Coordinate::constant::<128>(),
            )?,
        )?;
        let coarse = f.select(
            halve,
            f.resize_exact(
                f.slice::<19, 0, true, 1>(f.sub_same(fine, Coordinate::constant::<128>())?)?,
            )?,
            fine,
        )?;
        for (which, q) in [fine, coarse].into_iter().enumerate() {
            let integer: IntegerCoordinate = f.slice::<12, 0, true, 8>(q)?;
            let s = f.read(if which == 0 {
                side.at::<0>()
            } else {
                side.at::<1>()
            })?;
            // Wrapped UV and the centered fine/coarse construction guarantee
            // i0 in [-1, side-1], i1 in [0, side]. Each tap needs only one
            // boundary test, rather than the general signed modulo helper.
            let a: TexelCoordinate = f.resize_exact(f.select(
                f.less(integer, IntegerCoordinate::constant::<0>())?,
                f.sub_same(s, IntegerCoordinate::constant::<1>())?,
                integer,
            )?)?;
            let next = f.add_same(integer, IntegerCoordinate::constant::<1>())?;
            let b: TexelCoordinate = f.resize_exact(f.select(
                counted::eq(&f, next, s)?,
                IntegerCoordinate::constant::<0>(),
                next,
            )?)?;
            f.publish(&format!("q{which}.{axis}"), q)?;
            f.publish(&format!("f{which}.{axis}"), f.slice::<8, 0, false, 0>(q)?)?;
            f.publish(&format!("t{which}.{axis}.0"), a)?;
            f.publish(&format!("t{which}.{axis}.1"), b)?;
        }
    }
    Stage::finish(f)
}
fn rows(c: &Stage, q: &Stage) -> Result<Stage, Fault> {
    let mut m = Model::numerical();
    let parent: CoefficientStore = m.input("parents", &[c.raw("parent0"), c.raw("parent1")])?;
    let frac: FractionStore = m.input("fv", &[q.raw("f0.1"), q.raw("f1.1")])?;
    let nearest = m.input::<1, 0, false>("nearest", &[c.raw("nearest")])?;
    let f = m.compute("texture_row_weights", 128)?;
    let nearest = f.read(nearest.at::<0>())?;
    for which in 0..2 {
        let p = f.read(if which == 0 {
            parent.at::<0>()
        } else {
            parent.at::<1>()
        })?;
        let v = f.read(if which == 0 {
            frac.at::<0>()
        } else {
            frac.at::<1>()
        })?;
        let active = counted::not(&f, counted::eq(&f, p, Coefficient::constant::<0>())?)?;
        let high = f.branch_value(
            nearest,
            |_| Ok(Coefficient::constant::<0>()),
            |f| {
                f.branch_value(
                    active,
                    |f| Ok(counted::split(f, p, v, true)?[1]),
                    |_| Ok(Coefficient::constant::<0>()),
                )
            },
        )?;
        f.publish(&format!("r{which}.0"), f.sub_same(p, high)?)?;
        f.publish(&format!("r{which}.1"), high)?;
    }
    Stage::finish(f)
}
fn columns(c: &Stage, q: &Stage, r: &Stage) -> Result<Stage, Fault> {
    let mut m = Model::numerical();
    let rows: CoefficientStore = m.input(
        "rows",
        &(0..2)
            .flat_map(|p| (0..2).map(move |t| r.raw(&format!("r{p}.{t}"))))
            .collect::<Vec<_>>(),
    )?;
    let frac: FractionStore = m.input("fu", &[q.raw("f0.0"), q.raw("f1.0")])?;
    let nearest = m.input::<1, 0, false>("nearest", &[c.raw("nearest")])?;
    let f = m.compute("texture_column_weights", 128)?;
    let nearest = f.read(nearest.at::<0>())?;
    for which in 0..2 {
        let u = f.read(if which == 0 {
            frac.at::<0>()
        } else {
            frac.at::<1>()
        })?;
        for row in 0..2 {
            let p = f.read(pair_address(rows, which * 2 + row))?;
            let active = counted::not(&f, counted::eq(&f, p, Coefficient::constant::<0>())?)?;
            let high = f.branch_value(
                nearest,
                |_| Ok(Coefficient::constant::<0>()),
                |f| {
                    f.branch_value(
                        active,
                        |f| Ok(counted::split(f, p, u, false)?[1]),
                        |_| Ok(Coefficient::constant::<0>()),
                    )
                },
            )?;
            f.publish(&format!("w{which}.{}", row * 2), f.sub_same(p, high)?)?;
            f.publish(&format!("w{which}.{}", row * 2 + 1), high)?;
        }
    }
    Stage::finish(f)
}
fn decode(word: i128) -> Group4 {
    let w = word as u128;
    Group4 {
        key: TileKey {
            slot: (w & 15) as u8,
            n: ((w >> 4) & 15) as u8,
            x: ((w >> 8) & 127) as u8,
            y: ((w >> 15) & 127) as u8,
        },
        top_left_local: [((w >> 22) & 7) as u8, ((w >> 25) & 7) as u8],
        coefficients: std::array::from_fn(|j| ((w >> (28 + j * 9)) & 511) as u32),
        first: w >> 64 & 1 != 0,
        last: w >> 65 & 1 != 0,
        quad_id: ((w >> 66) & 15) as u8,
        lane: (w >> 70) as u8,
    }
}
fn plane(c: &Stage, q: &Stage, w: &Stage, lane: usize, which: usize) -> Result<Plane, Fault> {
    let mut m = Model::numerical();
    let weights: CoefficientStore = m.input(
        "weights",
        &(0..4)
            .map(|t| w.raw(&format!("w{which}.{t}")))
            .collect::<Vec<_>>(),
    )?;
    let coords: TexelCoordinateStore = m.input(
        "coords",
        &(0..2)
            .flat_map(|axis| (0..2).map(move |t| q.raw(&format!("t{which}.{axis}.{t}"))))
            .collect::<Vec<_>>(),
    )?;
    let meta = m.input::<4, 0, false>(
        "identity",
        &[c.raw("slot"), c.raw(&format!("n{which}")), c.raw("quad")],
    )?;
    let coarse_parent: CoefficientStore = m.input("coarse_parent", &[c.raw("parent1")])?;
    let lane_id = m.input::<2, 0, false>("lane", &[lane as i128])?;
    let fine_plane = m.input::<1, 0, false>("fine_plane", &[i128::from(which == 0)])?;
    let f = m.compute("texture_group_expansion", 2500)?;
    let ws: [Coefficient; 4] = std::array::from_fn(|_| Coefficient::constant::<0>());
    let mut ws = ws;
    for (t, v) in ws.iter_mut().enumerate() {
        *v = f.read(pair_address(weights, t))?;
    }
    f.require::<true>(f.less(Coefficient::constant::<0>(), ws[0])?)?;
    let tx = [
        f.slice::<7, 0, false, 3>(f.read(coords.at::<0>())?)?,
        f.slice::<7, 0, false, 3>(f.read(coords.at::<1>())?)?,
    ];
    let ty = [
        f.slice::<7, 0, false, 3>(f.read(coords.at::<2>())?)?,
        f.slice::<7, 0, false, 3>(f.read(coords.at::<3>())?)?,
    ];
    let lx = f.slice::<3, 0, false, 0>(f.read(coords.at::<0>())?)?;
    let ly = f.slice::<3, 0, false, 0>(f.read(coords.at::<2>())?)?;
    let slot = f.read(meta.at::<0>())?;
    let n = f.read(meta.at::<1>())?;
    let quad = f.read(meta.at::<2>())?;
    let is_fine = f.read(fine_plane.at::<0>())?;
    let lane_value = f.read(lane_id.at::<0>())?;
    let final_plane = f.select(
        is_fine,
        counted::eq(
            &f,
            f.read(coarse_parent.at::<0>())?,
            Coefficient::constant::<0>(),
        )?,
        Bit::constant::<1>(),
    )?;
    f.publish("final_plane", final_plane)?;
    let mut same = [[Bit::constant::<0>(); 4]; 4];
    let same_x = counted::eq(&f, tx[0], tx[1])?;
    let same_y = counted::eq(&f, ty[0], ty[1])?;
    let same_xy = f.select(same_x, same_y, Bit::constant::<0>())?;
    let mut emit = [Bit::constant::<0>(); 4];
    let mut nonzero = [Bit::constant::<0>(); 4];
    for t in 0..4 {
        nonzero[t] = counted::not(&f, counted::eq(&f, ws[t], Coefficient::constant::<0>())?)?;
    }
    for t in 0..4 {
        emit[t] = nonzero[t];
        for j in 0..4 {
            same[t][j] = match t ^ j {
                0 => Bit::constant::<1>(),
                1 => same_x,
                2 => same_y,
                _ => same_xy,
            };
            if j < t {
                emit[t] = f.select(
                    f.select(same[t][j], nonzero[j], Bit::constant::<0>())?,
                    Bit::constant::<0>(),
                    emit[t],
                )?;
            }
        }
    }
    for t in 0..4 {
        let mut last = final_plane;
        for later in emit.iter().skip(t + 1) {
            last = f.select(*later, Bit::constant::<0>(), last)?;
        }
        let mut word = GroupWord::constant::<0>();
        word = counted::pack_field::<0>(&f, word, slot)?;
        word = counted::pack_field::<4>(&f, word, n)?;
        word = counted::pack_field::<8>(&f, word, tx[t & 1])?;
        word = counted::pack_field::<15>(&f, word, ty[t >> 1])?;
        word = counted::pack_field::<22>(&f, word, lx)?;
        word = counted::pack_field::<25>(&f, word, ly)?;
        for (j, &weight) in ws.iter().enumerate() {
            let weight = f.select(same[t][j], weight, Coefficient::constant::<0>())?;
            word = match j {
                0 => counted::pack_field::<28>(&f, word, weight)?,
                1 => counted::pack_field::<37>(&f, word, weight)?,
                2 => counted::pack_field::<46>(&f, word, weight)?,
                _ => counted::pack_field::<55>(&f, word, weight)?,
            };
        }
        // A conserving floor split leaves positive tap0 for every active plane.
        let first = if t == 0 {
            is_fine
        } else {
            Bit::constant::<0>()
        };
        word = counted::pack_field::<64>(&f, word, first)?;
        word = counted::pack_field::<65>(&f, word, last)?;
        word = counted::pack_field::<66>(&f, word, quad)?;
        word = counted::pack_field::<70>(&f, word, lane_value)?;
        f.publish(&format!("emit{t}"), emit[t])?;
        f.publish(&format!("packet{t}"), word)?;
    }
    let stage = Stage::finish(f)?;
    let payloads: Vec<_> = (0..4)
        .filter(|&t| stage.raw(&format!("emit{t}")) != 0)
        .map(|t| stage.raw(&format!("packet{t}")))
        .collect();
    let groups = payloads.iter().copied().map(decode).collect();
    Ok(Plane {
        lane: lane as u8,
        which: which as u8,
        frame: stage.frame,
        groups,
        payloads,
    })
}
pub fn prepare(input: &QuadInput, slots: &[Slot]) -> Result<Preparation, counted::Error> {
    let slot = oracle::check_input(input, slots)?;
    let derivative = derivatives(input, slot)?;
    let lod = lod(&derivative)?;
    let mut lanes = vec![];
    let mut groups = vec![];
    let mut payloads = vec![];
    for lane in 0..4 {
        if lod.raw("mask") >> lane & 1 == 0 {
            continue;
        }
        let coordinate = coordinates(&derivative, &lod, lane)?;
        let rows = rows(&lod, &coordinate)?;
        let columns = columns(&lod, &coordinate, &rows)?;
        let mut planes = vec![];
        for which in 0..2 {
            if lod.raw(&format!("parent{which}")) == 0 {
                continue;
            }
            let p = plane(&lod, &coordinate, &columns, lane, which)?;
            groups.extend(p.groups.iter().cloned());
            payloads.extend(&p.payloads);
            planes.push(p);
        }
        lanes.push(Lane {
            lane: lane as u8,
            coordinate,
            rows,
            columns,
            planes,
        });
    }
    Ok(Preparation {
        derivative,
        lod,
        lanes,
        groups,
        payloads,
    })
}
