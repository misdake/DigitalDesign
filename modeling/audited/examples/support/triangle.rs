//! Framework driver: Q4 edge coefficients, one attribute sample and a small rcp.
//! This is deliberately a bounded kernel, not the existing full GPU setup model.
#![forbid(unsafe_code)]
use audited::{
    ExecutionMode, Fault, Fixed, Frame, FrameReport, Hardware, Limits, Memory, Model, PortShape,
    ProductRoute,
};

type Coordinate = Fixed<18, 4, true>;
type Edge = Fixed<36, 8, true>;
type Attribute = Fixed<18, 8, true>;
type Seed = Fixed<18, 16, false>;

fn normalized_rcp(
    frame: &Frame<'_>,
    denominator: Fixed<18, 16, false>,
    table: &Memory<18, 16, false>,
    route: ProductRoute,
) -> Result<Fixed<18, 16, true>, Fault> {
    // Explicit comparisons and control dependencies enforce this toy LUT domain.
    frame.require::<false>(frame.less(denominator, Seed::constant::<65536>())?)?;
    frame.require::<true>(frame.less(denominator, Seed::constant::<131072>())?)?;
    let index = frame.slice::<2, 0, false, 14>(denominator)?;
    let seed = frame.read(table.indexed(index))?;
    let dr = frame.mul::<36, 32, false>(denominator, seed, ProductRoute::Native18)?;
    let residual =
        frame.sub::<36, 32, true>(Fixed::<36, 32, true>::constant::<8589934592>(), dr)?;
    let refined = frame.mul::<54, 48, true>(residual, seed, route)?;
    frame.round_to::<18, 16, true>(refined)
}

pub fn run(route: ProductRoute) -> Result<FrameReport, Fault> {
    run_kernel(
        route,
        ExecutionMode::Scheduled,
        &[16, 32, 80, 32, 16, 96],
        &[32, 160, 224],
    )
}

/// Step-1 driver: external inputs and the same numerical kernel, no schedule.
pub fn run_numerical(
    route: ProductRoute,
    coordinates: &[i128; 6],
    attributes: &[i128; 3],
) -> Result<FrameReport, Fault> {
    run_kernel(route, ExecutionMode::Numerical, coordinates, attributes)
}

fn run_kernel(
    route: ProductRoute,
    mode: ExecutionMode,
    xy: &[i128; 6],
    attr: &[i128; 3],
) -> Result<FrameReport, Fault> {
    if route == ProductRoute::Native18 {
        return Err(Fault::Format);
    }
    let mut model = if mode == ExecutionMode::Numerical {
        Model::numerical()
    } else {
        Model::new(Hardware::one_wide_two_narrow())?
    };
    let ports = PortShape {
        read_ports: 1,
        write_ports: 1,
        read_latency: 1,
        max_reads_per_frame: 32,
        max_writes_per_frame: 32,
    };
    let (coordinates, attributes, coefficients, samples) = if mode == ExecutionMode::Numerical {
        (
            model.input::<18, 4, true>("vertices_xy", xy)?,
            model.input::<18, 8, true>("vertex_attribute", attr)?,
            model.scratch::<36, 8, true>("edge_c", 3)?,
            model.scratch::<18, 8, true>("sample", 1)?,
        )
    } else {
        (
            model.ram::<18, 4, true>("vertices_xy", 6, ports)?,
            model.ram::<18, 8, true>("vertex_attribute", 3, ports)?,
            model.ram::<36, 8, true>("edge_c", 3, ports)?,
            model.ram::<18, 8, true>("sample", 1, ports)?,
        )
    };
    let seeds = [
        Seed::constant::<65536>(),
        Seed::constant::<52429>(),
        Seed::constant::<43691>(),
        Seed::constant::<37449>(),
    ];
    let table = if mode == ExecutionMode::Numerical {
        model.table("rcp_seed", &seeds)?
    } else {
        model.rom(
            "rcp_seed",
            &seeds,
            PortShape {
                write_ports: 0,
                max_reads_per_frame: 2,
                max_writes_per_frame: 0,
                ..ports
            },
        )?
    };
    let frame = if mode == ExecutionMode::Numerical {
        model.compute("triangle_edge_attribute_rcp", 1024)?
    } else {
        model.begin_frame(
            "triangle_edge_attribute_rcp",
            Limits {
                max_cycle: 256,
                max_events: 1024,
            },
        )
    };
    // Compile-time literals can enter runtime data only through recorded writes.
    if mode == ExecutionMode::Scheduled {
        frame.write(coordinates.at::<0>(), Coordinate::constant::<16>())?;
        frame.write(coordinates.at::<1>(), Coordinate::constant::<32>())?;
        frame.write(coordinates.at::<2>(), Coordinate::constant::<80>())?;
        frame.write(coordinates.at::<3>(), Coordinate::constant::<32>())?;
        frame.write(coordinates.at::<4>(), Coordinate::constant::<16>())?;
        frame.write(coordinates.at::<5>(), Coordinate::constant::<96>())?;
        frame.write(attributes.at::<0>(), Attribute::constant::<32>())?;
        frame.write(attributes.at::<1>(), Attribute::constant::<160>())?;
        frame.write(attributes.at::<2>(), Attribute::constant::<224>())?;
    }
    let x = [
        frame.read(coordinates.at::<0>())?,
        frame.read(coordinates.at::<2>())?,
        frame.read(coordinates.at::<4>())?,
    ];
    let y = [
        frame.read(coordinates.at::<1>())?,
        frame.read(coordinates.at::<3>())?,
        frame.read(coordinates.at::<5>())?,
    ];
    let mut c = [Edge::constant::<0>(); 3];
    let mut edge_values = [Edge::constant::<0>(); 3];
    for i in 0..3 {
        // Host indices select the three fixed graph instances; they are not geometry.
        let j = (i + 1) % 3;
        let k = (i + 2) % 3;
        let a = frame.sub_same(y[j], y[k])?;
        let b = frame.sub_same(x[k], x[j])?;
        let xy: Edge = frame.product(x[j], y[k])?;
        let yx: Edge = frame.product(x[k], y[j])?;
        c[i] = frame.sub_same(xy, yx)?;
        let ax =
            frame.mul::<36, 8, true>(a, Coordinate::constant::<32>(), ProductRoute::Native18)?;
        let by =
            frame.mul::<36, 8, true>(b, Coordinate::constant::<48>(), ProductRoute::Native18)?;
        let sum = frame.add_same(ax, by)?;
        edge_values[i] = frame.add_same(sum, c[i])?;
    }
    frame.write(coefficients.at::<0>(), c[0])?;
    frame.write(coefficients.at::<1>(), c[1])?;
    frame.write(coefficients.at::<2>(), c[2])?;
    // The output plane is reloaded from the typed port, not reused from host raw data.
    let area01 = frame.add_same(
        frame.read(coefficients.at::<0>())?,
        frame.read(coefficients.at::<1>())?,
    )?;
    let area = frame.add_same(area01, frame.read(coefficients.at::<2>())?)?;
    let attr = [
        frame.read(attributes.at::<0>())?,
        frame.read(attributes.at::<1>())?,
        frame.read(attributes.at::<2>())?,
    ];
    let p0 = frame.mul::<54, 16, true>(edge_values[0], attr[0], route)?;
    let p1 = frame.mul::<54, 16, true>(edge_values[1], attr[1], route)?;
    let p2 = frame.mul::<54, 16, true>(edge_values[2], attr[2], route)?;
    let weighted01 = frame.add_same(p0, p1)?;
    let weighted = frame.add_same(weighted01, p2)?;
    let local_area = frame.resize_exact::<18, 8, false>(area)?;
    // The actual denominator determines both the mantissa and the binary exponent.
    // U18 range is a checked contract; this is not a general signed/wide setup path.
    let (normalized, exponent) = frame.normalize_positive(local_area)?;
    let reciprocal = normalized_rcp(&frame, normalized, &table, route)?;
    let numerator = frame.round_to::<36, 8, true>(weighted)?;
    let product = frame.mul::<54, 24, true>(numerator, reciprocal, route)?;
    let restore = frame.sub_same(Fixed::<18, 0, true>::constant::<0>(), exponent)?;
    let product = frame.shift(product, restore)?;
    let sample = frame.round_to::<18, 8, true>(product)?;
    frame.write(samples.at::<0>(), sample)?;
    frame.publish("sample_q8", frame.read(samples.at::<0>())?)?;
    frame.publish("area_q8", area)?;
    frame.publish("weighted_q16", weighted)?;
    // A non-power-of-two reciprocal exercises Newton and rounding independently.
    let rcp = normalized_rcp(&frame, Seed::constant::<98304>(), &table, route)?;
    let n = Fixed::<18, 12, true>::constant::<8192>();
    let q = frame.mul::<36, 28, true>(n, rcp, ProductRoute::Native18)?;
    frame.publish(
        "two_over_one_point_five_q12",
        frame.round_to::<18, 12, true>(q)?,
    )?;
    Ok(frame.finish())
}
