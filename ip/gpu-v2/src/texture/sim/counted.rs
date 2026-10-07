//! Closed step-1 datapath. No scheduling or concurrent cache claim.
use super::super::{format::*, ports::*};
use super::oracle::{self, Cache, CacheEvent};
use audited::{Fault, Fixed, FixedValue, Frame, FrameReport, Model};

#[derive(Debug)]
pub enum Error {
    Input(String),
    Audit(Fault),
}
impl From<Fault> for Error {
    fn from(value: Fault) -> Self {
        Self::Audit(value)
    }
}
impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::Input(value)
    }
}
pub struct Preparation {
    pub frame: FrameReport,
    pub lod: u32,
    pub lambda: u32,
    pub groups: Vec<Group4>,
    /// Byte address per group, independently derived inside the closed frame.
    pub addresses: Vec<u32>,
    /// Verbatim finished stage payloads; host decoding never feeds arithmetic.
    payloads: Vec<i128>,
}
impl Preparation {
    /// Transfer only the verbatim finished packet into the next closed stage.
    pub(crate) fn payload(&self, index: usize) -> i128 {
        self.payloads[index]
    }
}
pub struct Pixel {
    pub lane: u8,
    pub rgb: [u8; 3],
    pub frame: FrameReport,
}
pub struct Report {
    pub preparation: Preparation,
    pub pixels: Vec<Pixel>,
    pub cache_events: Vec<CacheEvent>,
}
type Bit = Fixed<1, 0, false>;
type Shift = Fixed<18, 0, true>;
pub(crate) fn not(f: &Frame<'_>, p: Bit) -> Result<Bit, Fault> {
    f.sub_same(Bit::constant::<1>(), p)
}
pub(crate) fn eq(
    f: &Frame<'_>,
    a: impl FixedValue + Copy,
    b: impl FixedValue + Copy,
) -> Result<Bit, Fault> {
    let either: Fixed<2, 0, false> = f.add(f.less(a, b)?, f.less(b, a)?)?;
    f.less(either, Fixed::<2, 0, false>::constant::<1>())
}
fn both(f: &Frame<'_>, a: Bit, b: Bit) -> Result<Bit, Fault> {
    f.select(a, b, Bit::constant::<0>())
}
fn z<const B: u32>(f: &Frame<'_>, x: Fixed<B, 0, false>) -> Result<Bit, Fault> {
    // Preserve the operand format so the structural zero-equality proof can
    // recognize a reduction NOR without relying on sampled values.
    eq(f, x, Fixed::<B, 0, false>::constant::<0>())
}
pub(crate) fn clamp(f: &Frame<'_>, x: LogWork, max: LogWork) -> Result<LogWork, Fault> {
    let x = f.select(
        f.less(x, LogWork::constant::<0>())?,
        LogWork::constant::<0>(),
        x,
    )?;
    f.select(f.less(max, x)?, max, x)
}
pub(crate) fn split(
    f: &Frame<'_>,
    parent: Coefficient,
    frac: Fraction,
    row: bool,
) -> Result<[Coefficient; 2], Fault> {
    let high = if row {
        f.branch_value(
            eq(f, parent, Coefficient::constant::<511>())?,
            |f| {
                let doubled: Fixed<9, 0, false> =
                    f.shift_left_const::<1, 9, 0, false>(f.resize_exact(frac)?)?;
                f.sub_same(doubled, f.resize_exact(not(f, z(f, frac)?)?)?)
            },
            |f| {
                let p: WeightProduct = f.product(parent, frac)?;
                f.slice::<9, 0, false, 8>(p)
            },
        )?
    } else {
        let p: WeightProduct = f.product(parent, frac)?;
        f.slice::<9, 0, false, 8>(p)?
    };
    Ok([f.sub_same(parent, high)?, high])
}
/// Exact nearest normalization for 0..130305; odd divisor has no half ties.
pub fn normalize_color(f: &Frame<'_>, value: Accumulator) -> Result<Color, Fault> {
    let h: Color = f.slice::<8, 0, false, 9>(value)?;
    let low: Color = f.slice::<8, 0, false, 0>(value)?;
    let high = f.slice::<1, 0, false, 8>(value)?;
    let sum: Fixed<9, 0, false> = f.add(h, low)?;
    let carry = f.slice::<1, 0, false, 8>(sum)?;
    let increment = f.select(high, Bit::constant::<1>(), carry)?;
    // Keep the carry through the adder, then check the proven 8-bit range.
    let result: Fixed<9, 0, false> = f.add(h, increment)?;
    f.resize_exact(result)
}
/// Virtual +1 coordinates select the bank before 8x8 wrap.
fn bank_address(
    f: &Frame<'_>,
    line: LineIndex,
    x: Fixed<4, 0, false>,
    y: Fixed<4, 0, false>,
) -> Result<(BankIndex, BankAddress), Fault> {
    let low = f.slice::<1, 0, false, 0>(x)?;
    let x1 = f.slice::<1, 0, false, 1>(x)?;
    let y0 = f.slice::<1, 0, false, 0>(y)?;
    let high = f.select(y0, not(f, x1)?, x1)?;
    let bank = f.add(
        f.shift_left_const::<1, 2, 0, false>(f.resize_exact(high)?)?,
        low,
    )?;
    let local_y = f.slice::<3, 0, false, 0>(y)?;
    let local_x = f.slice::<1, 0, false, 2>(x)?;
    let local: Fixed<4, 0, false> = f.add(
        f.shift_left_const::<1, 4, 0, false>(f.resize_exact(local_y)?)?,
        local_x,
    )?;
    let address = f.add(
        f.shift_left_const::<4, 10, 0, false>(f.resize_exact(line)?)?,
        local,
    )?;
    Ok((bank, address))
}
pub(crate) fn pack_field<const LOW: u32>(
    f: &Frame<'_>,
    word: GroupWord,
    field: impl FixedValue,
) -> Result<GroupWord, Fault> {
    f.add_same(
        word,
        f.shift_left_const::<LOW, 72, 0, false>(f.resize_exact(field)?)?,
    )
}
fn key(
    f: &Frame<'_>,
    slot: Fixed<4, 0, false>,
    n: Size,
    x: TileCoordinate,
    y: TileCoordinate,
) -> Result<Key, Fault> {
    let mut word = GroupWord::constant::<0>();
    word = pack_field::<0>(f, word, slot)?;
    word = pack_field::<4>(f, word, n)?;
    word = pack_field::<8>(f, word, x)?;
    word = pack_field::<15>(f, word, y)?;
    f.resize_exact(word)
}
fn wrap(
    f: &Frame<'_>,
    integer: IntegerCoordinate,
    side: IntegerCoordinate,
) -> Result<TexelCoordinate, Fault> {
    // Both select inputs exist: 1024+1024 needs a guard even when the add path
    // is not selected. Keep the wrap working format wider than the coordinate.
    let integer: WrapWork = f.resize_exact(integer)?;
    let side: WrapWork = f.resize_exact(side)?;
    let added = f.add_same(integer, side)?;
    let value = f.select(f.less(integer, WrapWork::constant::<0>())?, added, integer)?;
    let value = f.select(f.less(value, side)?, value, f.sub_same(value, side)?)?;
    f.resize_exact(value)
}
struct Context {
    slot: Fixed<4, 0, false>,
    quad: Fixed<4, 0, false>,
    base: ByteAddress,
    layers: [LayerContext; 2],
    queue: audited::Memory<72, 0, false>,
    cursor: std::cell::Cell<GroupCursor>,
}
#[derive(Clone, Copy)]
struct LayerContext {
    n: Size,
    side: IntegerCoordinate,
    coordinate_shift: Shift,
    tile_shift: Shift,
    prefix: Prefix,
}
fn layer_context(
    f: &Frame<'_>,
    n: Size,
    has_mip: Bit,
    prefixes: audited::Memory<13, 0, false>,
) -> Result<LayerContext, Fault> {
    let physical_n = f.select(f.less(n, Size::constant::<1>())?, Size::constant::<1>(), n)?;
    let side = f.shift(
        IntegerCoordinate::constant::<1>(),
        f.resize_exact(physical_n)?,
    )?;
    let coordinate_shift = f.sub::<18, 0, true>(physical_n, Size::constant::<8>())?;
    let tile_shift = f.sub::<18, 0, true>(n, Size::constant::<3>())?;
    let tile_shift = f.select(
        f.less(tile_shift, Shift::constant::<0>())?,
        Shift::constant::<0>(),
        tile_shift,
    )?;
    let prefix = f.read(prefixes.indexed(n))?;
    let prefix = f.select(has_mip, prefix, Prefix::constant::<0>())?;
    Ok(LayerContext {
        n,
        side,
        coordinate_shift,
        tile_shift,
        prefix,
    })
}
#[allow(clippy::too_many_arguments)] // One explicit stage, with two golden identities.
fn layer(
    f: &Frame<'_>,
    c: &Context,
    lane: usize,
    which: usize,
    q: [Coordinate; 2],
    parent: Coefficient,
    nearest: Bit,
    cumulative: &mut Coefficient,
) -> Result<(), Fault> {
    let label = format!("p{lane}.l{which}");
    let LayerContext { n, side, .. } = c.layers[which];
    f.publish(&format!("{label}.n"), n)?;
    f.publish(&format!("{label}.parent"), parent)?;
    let mut integer = [IntegerCoordinate::constant::<0>(); 2];
    let mut frac = [Fraction::constant::<0>(); 2];
    for axis in 0..2 {
        integer[axis] = f.slice::<12, 0, true, 8>(q[axis])?;
        frac[axis] = f.slice::<8, 0, false, 0>(q[axis])?;
        f.publish(&format!("{label}.q{axis}"), q[axis])?;
        f.publish(&format!("{label}.i{axis}"), integer[axis])?;
        f.publish(&format!("{label}.f{axis}"), frac[axis])?;
    }
    let weights = [const { std::cell::Cell::new(Coefficient::constant::<0>()) }; 4];
    f.branch(
        nearest,
        |_| {
            weights[0].set(parent);
            Ok(())
        },
        |f| {
            let row = split(f, parent, frac[1], true)?;
            let top = split(f, row[0], frac[0], false)?;
            let bottom = split(f, row[1], frac[0], false)?;
            for (cell, value) in weights.iter().zip([top[0], top[1], bottom[0], bottom[1]]) {
                cell.set(value);
            }
            Ok(())
        },
    )?;
    let weights = weights.map(|cell| cell.get());
    let x0 = wrap(f, integer[0], side)?;
    let y0 = wrap(f, integer[1], side)?;
    let x1 = wrap(
        f,
        f.add_same(integer[0], IntegerCoordinate::constant::<1>())?,
        side,
    )?;
    let y1 = wrap(
        f,
        f.add_same(integer[1], IntegerCoordinate::constant::<1>())?,
        side,
    )?;
    let coords = [[x0, y0], [x1, y0], [x0, y1], [x1, y1]];
    let mut keys = [Key::constant::<0>(); 4];
    let mut tile = [[TileCoordinate::constant::<0>(); 2]; 4];
    for t in 0..4 {
        tile[t] = [
            f.slice::<7, 0, false, 3>(coords[t][0])?,
            f.slice::<7, 0, false, 3>(coords[t][1])?,
        ];
        keys[t] = key(f, c.slot, n, tile[t][0], tile[t][1])?;
        f.publish(&format!("{label}.w{t}"), weights[t])?;
        for (axis, coordinate) in coords[t].iter().enumerate() {
            f.publish(&format!("{label}.t{t}.{axis}"), *coordinate)?;
        }
    }
    let mut group_number = Fixed::<3, 0, false>::constant::<0>();
    for t in 0..4 {
        let mut emit = not(f, z(f, weights[t])?)?;
        for previous in 0..t {
            let duplicate = both(
                f,
                eq(f, keys[t], keys[previous])?,
                not(f, z(f, weights[previous])?)?,
            )?;
            emit = both(f, emit, not(f, duplicate)?)?;
        }
        f.branch(
            emit,
            |f| {
                let mut word = f.resize_exact(keys[t])?;
                word = pack_field::<22>(f, word, f.slice::<3, 0, false, 0>(x0)?)?;
                word = pack_field::<25>(f, word, f.slice::<3, 0, false, 0>(y0)?)?;
                let mut group_sum = Coefficient::constant::<0>();
                for (j, weight) in weights.iter().enumerate() {
                    let w = f.select(
                        eq(f, keys[t], keys[j])?,
                        *weight,
                        Coefficient::constant::<0>(),
                    )?;
                    group_sum = f.add_same(group_sum, w)?;
                    // Static bit positions are circuit wiring, not runtime addressing.
                    word = match j {
                        0 => pack_field::<28>(f, word, w)?,
                        1 => pack_field::<37>(f, word, w)?,
                        2 => pack_field::<46>(f, word, w)?,
                        _ => pack_field::<55>(f, word, w)?,
                    };
                }
                word = pack_field::<64>(f, word, z(f, *cumulative)?)?;
                *cumulative = f.add_same(*cumulative, group_sum)?;
                word =
                    pack_field::<65>(f, word, eq(f, *cumulative, Coefficient::constant::<511>())?)?;
                word = pack_field::<66>(f, word, c.quad)?;
                // Lane index belongs to static unrolled circuit structure.
                let lane_value = match lane {
                    0 => Fixed::<2, 0, false>::constant::<0>(),
                    1 => Fixed::<2, 0, false>::constant::<1>(),
                    2 => Fixed::<2, 0, false>::constant::<2>(),
                    _ => Fixed::<2, 0, false>::constant::<3>(),
                };
                word = pack_field::<70>(f, word, lane_value)?;
                f.write(c.queue.indexed(c.cursor.get()), word)?;
                c.cursor
                    .set(f.add_same(c.cursor.get(), GroupCursor::constant::<1>())?);
                f.publish(&format!("{label}.g{t}"), word)?;
                group_number = f.add_same(group_number, Fixed::<3, 0, false>::constant::<1>())?;
                let row = f.shift(
                    f.resize_exact::<15, 0, false>(tile[t][1])?,
                    c.layers[which].tile_shift,
                )?;
                let prefix = c.layers[which].prefix;
                let index =
                    f.add::<15, 0, false>(f.add::<15, 0, false>(row, tile[t][0])?, prefix)?;
                let offset = f.shift_left_const::<7, 32, 0, false>(f.resize_exact(index)?)?;
                f.publish(&format!("{label}.a{t}"), f.add_same(c.base, offset)?)?;
                Ok(())
            },
            |_| Ok(()),
        )?;
    }
    f.publish(&format!("{label}.groups"), group_number)?;
    Ok(())
}

pub fn prepare(input: &QuadInput, slots: &[Slot]) -> Result<Preparation, Error> {
    #[cfg(test)]
    super::staged::bound::counted_call_guard::assert_not_live();
    let slot = oracle::check_input(input, slots)?;
    let (captured, force_coarsest) = capture_uv(input)?;
    let uv: Vec<i128> = captured.into_iter().flatten().map(i128::from).collect();
    // External capture: biases outside +/-32 cannot change a clamped LOD.
    let bias = (input.lod_bias.clamp(-32.0, 32.0) * 256.0).round_ties_even() as i128;
    let mut model = Model::numerical();
    let uv_store: UvStore = model.input("helper_uv", &uv)?;
    let force = model.input::<1, 0, false>("force_coarsest", &[i128::from(force_coarsest)])?;
    let bias_store: BiasStore = model.input("lod_bias_q8", &[bias])?;
    let metadata = model.input::<4, 0, false>(
        "quad_context",
        &[
            input.quad_id.into(),
            input.mask.into(),
            input.slot.into(),
            slot.max_size_log2.into(),
        ],
    )?;
    let base_store: ByteAddressStore = model.input("slot_base", &[slot.base_address.into()])?;
    let mip_store = model.input::<1, 0, false>("slot_has_mip", &[i128::from(slot.has_full_mip)])?;
    let filter_store: FilterCodeStore = model.input(
        "filter",
        &[match input.filter {
            Filter::Nearest => 0,
            Filter::Bilinear => 1,
            Filter::Trilinear => 2,
        }],
    )?;
    let log: LogEntryStore = model.table("log2_64", &LOG)?;
    let prefix: PrefixStore = model.table("mip_prefix", &PREFIX)?;
    let queue: GroupWordStore = model.scratch("Group4_fifo", 32)?;
    let f = model.compute("texture_prepare", 12000)?;
    let quad = f.read(metadata.at::<0>())?;
    let mask = f.read(metadata.at::<1>())?;
    let slot_id = f.read(metadata.at::<2>())?;
    let max_n = f.read(metadata.at::<3>())?;
    let has_mip = f.read(mip_store.at::<0>())?;
    let max_lod = f.select(has_mip, max_n, Size::constant::<0>())?;
    let max_raw: LogWork = f.shift_left_const::<8, 18, 0, true>(f.resize_exact(max_lod)?)?;
    let filter = f.read(filter_store.at::<0>())?;
    let nearest = eq(&f, filter, FilterCode::constant::<0>())?;
    let trilinear = eq(&f, filter, FilterCode::constant::<2>())?;
    let center = f.select(
        nearest,
        Coordinate::constant::<0>(),
        Coordinate::constant::<128>(),
    )?;
    let mut uvs = [[UvRaw::constant::<0>(); 2]; 4];
    for (i, row) in uvs.iter_mut().enumerate() {
        for (axis, value) in row.iter_mut().enumerate() {
            let address = match i * 2 + axis {
                0 => uv_store.at::<0>(),
                1 => uv_store.at::<1>(),
                2 => uv_store.at::<2>(),
                3 => uv_store.at::<3>(),
                4 => uv_store.at::<4>(),
                5 => uv_store.at::<5>(),
                6 => uv_store.at::<6>(),
                _ => uv_store.at::<7>(),
            };
            *value = f.binary_scale(f.read(address)?)?;
        }
    }
    let mut slope = Magnitude::constant::<0>();
    for (edge, (a, b)) in [(0, 1), (2, 3), (0, 2), (1, 3)].into_iter().enumerate() {
        for (axis, (&u, &v)) in uvs[a].iter().zip(&uvs[b]).enumerate() {
            let d: Difference = f.sub(v, u)?;
            f.publish(&format!("d{edge}.{axis}"), d)?;
            let neg = f.sub_same(Difference::constant::<0>(), d)?;
            let mag: Magnitude =
                f.resize_exact(f.select(f.less(d, Difference::constant::<0>())?, neg, d)?)?;
            slope = f.select(f.less(slope, mag)?, mag, slope)?;
        }
    }
    let overflow = f.select(
        f.read(force.at::<0>())?,
        Fixed::<1, 0, false>::constant::<1>(),
        f.less(Magnitude::constant::<131072>(), slope)?,
    )?;
    f.publish("overflow", overflow)?;
    let lod: Lod = f.branch_value(
        overflow,
        |f| f.resize_exact(max_raw),
        |f| {
            f.branch_value(
                z(f, slope)?,
                |_| Ok(Lod::constant::<0>()),
                |f| {
                    let slope: Slope = f.resize_exact(slope)?;
                    let zeros = f.leading_zeros(slope)?;
                    let h = f.sub_same(Shift::constant::<19>(), zeros)?;
                    let shift = f.sub_same(Shift::constant::<38>(), h)?;
                    let norm: Normalized = f.shift(f.resize_exact(slope)?, shift)?;
                    let tail: MantissaTail = f.slice::<38, 32, false, 0>(norm)?;
                    let k: TableIndex = f.round_to(tail)?;
                    let carry = eq(f, k, TableIndex::constant::<64>())?;
                    let address = f.slice::<6, 0, false, 0>(k)?;
                    let fraction = f.read(log.indexed(address))?;
                    let exponent = f.add_same(
                        f.sub_same(h, Shift::constant::<16>())?,
                        f.resize_exact(max_n)?,
                    )?;
                    let exponent = f.add_same(exponent, f.resize_exact(carry)?)?;
                    let integer = f.shift_left_const::<8, 18, 0, true>(exponent)?;
                    let raw = f.add_same(integer, f.resize_exact(fraction)?)?;
                    let raw = f.add_same(raw, f.resize_exact(f.read(bias_store.at::<0>())?)?)?;
                    f.publish("log.index", k)?;
                    f.publish(
                        "log.exponent",
                        f.sub_same(exponent, f.resize_exact(carry)?)?,
                    )?;
                    f.resize_exact(clamp(f, raw, max_raw)?)
                },
            )
        },
    )?;
    f.publish("lod", lod)?;
    let level: Size = f.slice::<4, 0, false, 8>(lod)?;
    let fine_n = f.sub_same(max_n, level)?;
    let lod_frac: Fraction = f.slice::<8, 0, false, 0>(lod)?;
    let lambda = f.branch_value(
        trilinear,
        |f| {
            let doubled = f.shift_left_const::<1, 9, 0, false>(f.resize_exact(lod_frac)?)?;
            f.sub_same(
                doubled,
                f.resize_exact(f.less(Fraction::constant::<128>(), lod_frac)?)?,
            )
        },
        |_| Ok(Coefficient::constant::<0>()),
    )?;
    f.publish("lambda", lambda)?;
    let parents = [f.sub_same(Coefficient::constant::<511>(), lambda)?, lambda];
    let coarse_n = f.select(
        z(&f, fine_n)?,
        fine_n,
        f.sub::<5, 0, true>(fine_n, Size::constant::<1>())
            .and_then(|x| {
                f.select(
                    f.less(x, Fixed::<5, 0, true>::constant::<0>())?,
                    Fixed::<5, 0, true>::constant::<0>(),
                    x,
                )
            })
            .and_then(|x| f.resize_exact(x))?,
    )?;
    let c = Context {
        slot: slot_id,
        quad,
        base: f.read(base_store.at::<0>())?,
        layers: [
            layer_context(&f, fine_n, has_mip, prefix)?,
            layer_context(&f, coarse_n, has_mip, prefix)?,
        ],
        queue,
        cursor: std::cell::Cell::new(GroupCursor::constant::<0>()),
    };
    let halve_coarse = f.less(Size::constant::<1>(), fine_n)?;
    for (lane, uv) in uvs.iter().enumerate() {
        let active = match lane {
            0 => f.slice::<1, 0, false, 0>(mask)?,
            1 => f.slice::<1, 0, false, 1>(mask)?,
            2 => f.slice::<1, 0, false, 2>(mask)?,
            _ => f.slice::<1, 0, false, 3>(mask)?,
        };
        f.branch(
            active,
            |f| {
                let mut q = [Coordinate::constant::<0>(); 2];
                let shift = c.layers[0].coordinate_shift;
                for axis in 0..2 {
                    let wrapped = f.slice::<16, 0, false, 0>(uv[axis])?;
                    let scaled = f.shift(f.resize_exact::<20, 0, true>(wrapped)?, shift)?;
                    q[axis] = f.sub_same(scaled, center)?;
                }
                let mut cumulative = Coefficient::constant::<0>();
                layer(f, &c, lane, 0, q, parents[0], nearest, &mut cumulative)?;
                f.branch(
                    not(f, z(f, parents[1])?)?,
                    |f| {
                        let mut coarse = q;
                        for axis in 0..2 {
                            coarse[axis] = f.branch_value(
                                halve_coarse,
                                |f| {
                                    f.resize_exact(f.slice::<19, 0, true, 1>(
                                        f.sub_same(q[axis], Coordinate::constant::<128>())?,
                                    )?)
                                },
                                |_| Ok(q[axis]),
                            )?;
                        }
                        layer(
                            f,
                            &c,
                            lane,
                            1,
                            coarse,
                            parents[1],
                            Bit::constant::<0>(),
                            &mut cumulative,
                        )
                    },
                    |_| Ok(()),
                )?;
                f.require::<true>(eq(f, cumulative, Coefficient::constant::<511>())?)?;
                Ok(())
            },
            |_| Ok(()),
        )?;
    }
    f.publish("group_count", c.cursor.get())?;
    let frame = f.finish();
    frame.audit()?;
    let get = |name: &str| frame.outputs.iter().find(|o| o.name == name).map(|o| o.raw);
    let mut groups = Vec::new();
    let mut addresses = Vec::new();
    let mut payloads = Vec::new();
    for lane in 0..4 {
        for which in 0..2 {
            for t in 0..4 {
                let label = format!("p{lane}.l{which}");
                if let Some(raw) = get(&format!("{label}.g{t}")) {
                    payloads.push(raw);
                    let w = raw as u128;
                    groups.push(Group4 {
                        key: TileKey {
                            slot: (w & 15) as u8,
                            n: ((w >> 4) & 15) as u8,
                            x: ((w >> 8) & 127) as u8,
                            y: ((w >> 15) & 127) as u8,
                        },
                        top_left_local: [((w >> 22) & 7) as u8, ((w >> 25) & 7) as u8],
                        coefficients: std::array::from_fn(|j| ((w >> (28 + 9 * j)) & 511) as u32),
                        first: w >> 64 & 1 != 0,
                        last: w >> 65 & 1 != 0,
                        quad_id: ((w >> 66) & 15) as u8,
                        lane: (w >> 70) as u8,
                    });
                    addresses.push(get(&format!("{label}.a{t}")).unwrap() as u32);
                }
            }
        }
    }
    let lod = get("lod").unwrap() as u32;
    let lambda = get("lambda").unwrap() as u32;
    Ok(Preparation {
        frame,
        lod,
        lambda,
        groups,
        addresses,
        payloads,
    })
}

/// Functional cache remains an atomic memory adapter. Returned RAW565 words
/// enter the color frame as external data, never as an arithmetic oracle.
pub fn sample<M: MemoryPort>(
    input: &QuadInput,
    cache: &mut Cache,
    memory: &mut M,
) -> Result<Report, Error> {
    let preparation = prepare(input, cache.slots())?;
    let mut cache_events = Vec::new();
    let mut pixels = Vec::new();
    for lane in 0..4 {
        let groups: Vec<_> = preparation
            .groups
            .iter()
            .zip(&preparation.payloads)
            .filter(|(g, _)| g.lane == lane)
            .collect();
        if groups.is_empty() {
            continue;
        }
        let mut texels = Vec::new();
        let mut words = Vec::new();
        let mut lines = Vec::new();
        for (g, payload) in &groups {
            words.push(**payload);
            texels.extend(
                cache
                    .read_group(g, memory, &mut cache_events)?
                    .map(i128::from),
            );
            lines.push(
                cache
                    .lookup(g.key)
                    .ok_or_else(|| "missing READY read reservation".to_owned())?
                    .0 as i128,
            );
        }
        let mut m = Model::numerical();
        let payload: GroupWordStore = m.input("Group4", &words)?;
        let data: TexelStore = m.input("cache_RAW565", &texels)?;
        let reservations: LineIndexStore = m.input("cache_reserved_line", &lines)?;
        let f = m.compute("texture_color", 8000)?;
        let mut acc = [Accumulator::constant::<0>(); 3];
        for group_index in 0..groups.len() {
            // Bounded stage-payload iteration after preparation has finished;
            // each row is read using a static circuit-address literal.
            let gi = constant_index(group_index);
            let word = f.read(payload.indexed(gi))?;
            let line = f.read(reservations.indexed(gi))?;
            let local_x = f.resize_exact::<4, 0, false>(f.slice::<3, 0, false, 22>(word)?)?;
            let local_y = f.resize_exact::<4, 0, false>(f.slice::<3, 0, false, 25>(word)?)?;
            let weights = [
                f.slice::<9, 0, false, 28>(word)?,
                f.slice::<9, 0, false, 37>(word)?,
                f.slice::<9, 0, false, 46>(word)?,
                f.slice::<9, 0, false, 55>(word)?,
            ];
            let mut partial = [Accumulator::constant::<0>(); 3];
            for (tap, weight) in weights.iter().enumerate() {
                let dx = if tap & 1 == 0 {
                    Fixed::<4, 0, false>::constant::<0>()
                } else {
                    Fixed::<4, 0, false>::constant::<1>()
                };
                let dy = if tap & 2 == 0 {
                    Fixed::<4, 0, false>::constant::<0>()
                } else {
                    Fixed::<4, 0, false>::constant::<1>()
                };
                let (bank, address) =
                    bank_address(&f, line, f.add_same(local_x, dx)?, f.add_same(local_y, dy)?)?;
                f.publish(&format!("g{group_index}.t{tap}.bank"), bank)?;
                f.publish(&format!("g{group_index}.t{tap}.address"), address)?;
                let t = f.read(data.indexed(constant_index(group_index * 4 + tap)))?;
                let r = f.slice::<5, 0, false, 11>(t)?;
                let g = f.slice::<6, 0, false, 5>(t)?;
                let b = f.slice::<5, 0, false, 0>(t)?;
                let rgb: [Color; 3] = [
                    f.add(
                        f.shift_left_const::<3, 8, 0, false>(f.resize_exact(r)?)?,
                        f.slice::<3, 0, false, 2>(r)?,
                    )?,
                    f.add(
                        f.shift_left_const::<2, 8, 0, false>(f.resize_exact(g)?)?,
                        f.slice::<2, 0, false, 4>(g)?,
                    )?,
                    f.add(
                        f.shift_left_const::<3, 8, 0, false>(f.resize_exact(b)?)?,
                        f.slice::<3, 0, false, 2>(b)?,
                    )?,
                ];
                for channel in 0..3 {
                    f.publish(&format!("g{group_index}.t{tap}.c{channel}"), rgb[channel])?;
                    let product: Accumulator = f.product(*weight, rgb[channel])?;
                    partial[channel] = f.add_same(partial[channel], product)?;
                }
            }
            for channel in 0..3 {
                acc[channel] = f.add_same(acc[channel], partial[channel])?;
                f.publish(
                    &format!("g{group_index}.partial{channel}"),
                    partial[channel],
                )?;
                f.publish(&format!("g{group_index}.acc{channel}"), acc[channel])?;
            }
        }
        for (channel, value) in acc.iter().enumerate() {
            f.publish(&format!("rgb{channel}"), normalize_color(&f, *value)?)?;
        }
        let frame = f.finish();
        frame.audit()?;
        let rgb = std::array::from_fn(|channel| {
            frame
                .outputs
                .iter()
                .find(|o| o.name == format!("rgb{channel}"))
                .unwrap()
                .raw as u8
        });
        pixels.push(Pixel { lane, rgb, frame });
    }
    Ok(Report {
        preparation,
        pixels,
        cache_events,
    })
}
// Bounded static unrolling address literals; a runtime data constructor is absent.
fn constant_index(index: usize) -> Fixed<5, 0, false> {
    macro_rules! indices { ($($i:literal),*) => {match index {$($i=>Fixed::constant::<$i>(),)* _=>unreachable!("at most 32 texel words per lane")}}; }
    indices!(
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31
    )
}
