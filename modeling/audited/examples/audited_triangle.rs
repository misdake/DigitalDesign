//! Small framework driver. Host formatting happens only after the frame closes.
#![forbid(unsafe_code)]
use audited::{ExecutionMode, FrameReport, ProductRoute, Resource};
#[path = "support/triangle.rs"]
mod triangle;
use std::fmt::Write as _;
use std::path::Path;

fn csv_field(value: impl std::fmt::Display) -> String {
    format!("\"{}\"", value.to_string().replace('"', "\"\""))
}
fn save(report: &FrameReport, directory: &Path, label: &str) -> Result<(), String> {
    let cycle = |value: u64| {
        if report.mode == ExecutionMode::Scheduled {
            value.to_string()
        } else {
            String::new()
        }
    };
    let mut csv = String::from(
        "event,operation,resource,lane,issue_cycle,ready_cycle,inputs,control,output,format,raw\n",
    );
    for e in &report.events {
        let value = e.output.map(|i| &report.values[i]);
        writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{},{}",
            e.id,
            csv_field(format!("{:?}", e.operation)),
            csv_field(format!("{:?}", e.resource)),
            csv_field(format!("{:?}", e.lane)),
            cycle(e.issue_cycle),
            cycle(e.ready_cycle),
            csv_field(format!("{:?}", e.inputs)),
            csv_field(format!("{:?}", e.control)),
            csv_field(format!("{:?}", e.output)),
            csv_field(value.map_or_else(String::new, |v| format!("{:?}", v.format))),
            value.map_or_else(String::new, |v| v.raw.to_string())
        )
        .unwrap();
    }
    std::fs::write(directory.join(format!("{label}-events.csv")), csv)
        .map_err(|e| e.to_string())?;
    let mut csv = String::from(
        "memory,operation,event,issue_cycle,ready_cycle,row,bits,fraction,signed,raw\n",
    );
    for e in &report.events {
        use audited::Operation;
        let (memory, row, id, operation) = match e.operation {
            Operation::Read { memory, row } => (memory, row, e.output.unwrap(), "read"),
            Operation::Write { memory, row } => (memory, row, e.inputs[0], "write"),
            _ => continue,
        };
        let v = &report.values[id];
        writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{}",
            csv_field(&report.memories[memory].name),
            operation,
            e.id,
            cycle(e.issue_cycle),
            cycle(e.ready_cycle),
            row,
            v.format.bits,
            v.format.fraction,
            v.format.signed,
            v.raw
        )
        .unwrap();
    }
    std::fs::write(directory.join(format!("{label}-memory.csv")), csv)
        .map_err(|e| e.to_string())?;
    std::fs::write(
        directory.join(format!("{label}-report.txt")),
        format!("{report:#?}"),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn main() -> Result<(), String> {
    let directory = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/gpu-v2-audited-fixed".into());
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    for (label, route) in [
        ("native-pair", ProductRoute::Native18Pair),
        ("wide-route", ProductRoute::Wide36),
        ("numerical", ProductRoute::Native18Pair),
    ] {
        let report = if label == "numerical" {
            triangle::run_numerical(route, &[16, 32, 80, 32, 16, 96], &[32, 160, 224])
        } else {
            triangle::run(route)
        }
        .map_err(|e| format!("{e:?}"))?;
        report.audit().map_err(|e| format!("{e:?}"))?;
        if !report.valid {
            return Err(format!("invalid frame: {:?}", report.faults));
        }
        let cycles = if report.mode == ExecutionMode::Scheduled {
            report.cycles.to_string()
        } else {
            "not_scheduled".into()
        };
        let capacity = if report.mode == ExecutionMode::Scheduled {
            report.hardware.dsp18_units().to_string()
        } else {
            "not_bound".into()
        };
        println!("audited_triangle,route={label},mode={:?},cycles={cycles},events={},DSP18_capacity={capacity},physical18={},physical36={},adders={:?},valid={}",report.mode,report.events.len(),report.counts.resources.get(&Resource::Dsp18).copied().unwrap_or(0),report.counts.resources.get(&Resource::Dsp36).copied().unwrap_or(0),report.counts.resources.iter().filter(|(r,_)| matches!(r,Resource::Adder(_))).collect::<Vec<_>>(),report.valid);
        // Report all declared units, including unused capacity, without hiding control costs.
        for (resource, unit) in &report.hardware.units {
            let issues = report.counts.resources.get(resource).copied().unwrap_or(0);
            let peak = report
                .issue_histogram
                .iter()
                .filter(|((r, _), _)| r == resource)
                .map(|(_, count)| *count)
                .max()
                .unwrap_or(0);
            println!("unit,{label},{resource:?},lanes={},latency={},initiation={},issues={issues},peak_issue={peak}",
                unit.lanes, unit.latency, unit.initiation);
        }
        for value in &report.outputs {
            println!(
                "output,{label},{},format={:?},raw={}",
                value.name, value.format, value.raw
            );
        }
        for (id, m) in report.memories.iter().enumerate() {
            println!(
                "memory,{label},{},kind={:?},reads={},writes={},read_bits={},write_bits={}",
                m.name,
                m.kind,
                report
                    .counts
                    .resources
                    .get(&Resource::Read(id))
                    .copied()
                    .unwrap_or(0),
                report
                    .counts
                    .resources
                    .get(&Resource::Write(id))
                    .copied()
                    .unwrap_or(0),
                report.counts.read_bits.get(&id).copied().unwrap_or(0),
                report.counts.write_bits.get(&id).copied().unwrap_or(0)
            );
        }
        save(&report, directory, label)?;
    }
    println!("scope=small_fixed_point_framework_driver,full_triangle_setup=not_migrated,no_division_operation=true,logic_timing=not_fitted");
    Ok(())
}
