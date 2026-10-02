//! Inspect the counted template behind timed, including rounding increment adds.
use audited::{Operation, Resource};
use gpu_v2::lighting::{ports::*, sim::timed};
use std::{collections::BTreeMap, fmt::Write, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-lighting".into()),
    );
    fs::create_dir_all(&root)?;
    let plan = timed::plan(
        &[PixelInput {
            normal: [7123, -519, 13567],
            ndc: [-65536, 12345],
        }],
        Material::default(),
        Light::default(),
        Projection::default(),
        timed::Hardware::default(),
        timed::Storage::Registers,
        timed::Strategy::Interleaved,
    )?;
    let f = &plan.template;
    let end = |name: &str| {
        f.events
            .iter()
            .find(|e| matches!(&e.operation,Operation::Publish(n) if n==name))
            .unwrap()
            .id
    };
    let (n_end, ray_end, v_end, h_end) = (end("n.2"), end("ray.2"), end("v.2"), end("h.2"));
    let half_end = f
        .events
        .iter()
        .find(|e| e.id > v_end && matches!(e.operation, Operation::Sub))
        .unwrap()
        .id
        - 1;
    let mut totals = BTreeMap::new();
    let mut stages = BTreeMap::new();
    let mut detail = String::from("event\tstage\tcategory\texact_width\toperation\tinputs\n");
    for e in &f.events {
        let Some(Resource::Adder(bits)) = e.resource else {
            continue;
        };
        let bucket = if bits <= 18 { 18 } else { 36 };
        let stage = if e.id <= n_end {
            "N normalize"
        } else if e.id <= ray_end {
            "diffuse + projection"
        } else if e.id <= v_end {
            "V normalize"
        } else if e.id <= half_end {
            "half vector"
        } else if e.id <= h_end {
            "H normalize"
        } else {
            "specular + power"
        };
        let rounding = e.inputs.iter().any(|&v| {
            matches!(
                f.events[f.values[v].producer].operation,
                Operation::RoundIncrement(_)
            )
        });
        let abs = matches!(e.operation, Operation::Sub)
            && e.output.is_some_and(|out| {
                f.events.iter().any(|s| {
                    matches!(s.operation, Operation::Select)
                        && s.inputs.get(1) == Some(&out)
                        && s.inputs.get(2) == e.inputs.get(1)
                })
            });
        let slope = matches!(e.operation, Operation::Add)
            && bits == 8
            && matches!(
                f.events[f.values[e.inputs[0]].producer].operation,
                Operation::ShiftLeft(1)
            )
            && f.values[e.inputs[1]].raw == 1;
        let address = matches!(e.operation, Operation::Add)
            && bits == 7
            && matches!(
                f.events[f.values[e.inputs[0]].producer].operation,
                Operation::ShiftLeft(6)
            );
        let category = if rounding {
            "RNE increment"
        } else if abs {
            "absolute negation"
        } else if slope {
            "square slope 2a+1"
        } else if address {
            "rsqrt page+segment"
        } else if matches!(e.operation, Operation::Sub) {
            if stage == "specular + power" {
                "power shift sign / tail"
            } else if f.values[e.inputs[0]].format.fraction == 15 {
                "rsqrt base-correction"
            } else {
                "exponent / shift control"
            }
        } else if bucket == 36 {
            "square / dot sums"
        } else if stage == "half vector" {
            "half L+V"
        } else if stage == "specular + power" {
            "power address / interpolation"
        } else {
            "ambient + diffuse"
        };
        *totals.entry((bucket, category)).or_insert(0_usize) += 1;
        *stages.entry((stage, bucket)).or_insert(0_usize) += 1;
        let inputs: Vec<_> = e
            .inputs
            .iter()
            .map(|&v| {
                (
                    v,
                    f.values[v].format,
                    f.values[v].raw,
                    f.events[f.values[v].producer].operation.clone(),
                )
            })
            .collect();
        writeln!(
            detail,
            "{}\t{stage}\t{category}\t{bits}\t{:?}\t{inputs:?}",
            e.id, e.operation
        )?;
    }
    let mut summary = String::new();
    for ((bucket, category), count) in totals {
        writeln!(summary, "adder{bucket}: {category}: {count}")?;
    }
    for ((stage, bucket), count) in stages {
        writeln!(summary, "{stage}: adder{bucket}: {count}")?;
    }
    fs::write(root.join("additions.tsv"), detail)?;
    fs::write(root.join("additions-summary.txt"), &summary)?;
    print!("{summary}");
    Ok(())
}
