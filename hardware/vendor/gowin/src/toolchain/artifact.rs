//! Existing hardware snapshots: no module generation or project export on reads.
use super::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const FILE: &str = "gowin-project.json";
const KINDS: &[ResourceKind] = &[
    ResourceKind::Lut4,
    ResourceKind::FlipFlop,
    ResourceKind::SsramBit,
    ResourceKind::Bsram18K,
    ResourceKind::Multiplier18x18,
    ResourceKind::Pll,
    ResourceKind::BoardClock27M,
    ResourceKind::UserLed,
    ResourceKind::UserButton,
    ResourceKind::DebugUartTx,
    ResourceKind::SdrSdramDevice,
    ResourceKind::Ddr3Device,
    ResourceKind::SpiFlashDevice,
    ResourceKind::HdmiOutput,
];
const MODES: &[GowinDspMode] = &[
    GowinDspMode::Padd18,
    GowinDspMode::Mult9x9,
    GowinDspMode::Mult18x18,
    GowinDspMode::MultAddAlu18x18,
    GowinDspMode::Alu54d,
];
fn invalid(message: impl Into<String>) -> GowinError {
    GowinError::InvalidBuildManifest(message.into())
}
fn number(v: &Value) -> Result<u64, GowinError> {
    v.as_u64().ok_or_else(|| invalid("artifact integer"))
}
fn string(v: &Value) -> Result<&str, GowinError> {
    v.as_str().ok_or_else(|| invalid("artifact string"))
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn key<T: std::fmt::Debug>(v: T) -> String {
    format!("{v:?}")
}
fn device(v: GowinDeviceInfo) -> Value {
    json!({"name":v.device_name,"version":v.device_version,
    "part":v.part_number,"project_device_id":v.project_device_id,"programmer":v.programmer_device,
    "cable":v.programmer_cable.index(),"flash_access":key(v.external_flash_access)})
}
fn counts(v: &BTreeMap<ResourceKind, u64>) -> Value {
    Value::Object(v.iter().map(|(kind, n)| (key(kind), json!(n))).collect())
}
fn read_counts(v: &Value) -> Result<BTreeMap<ResourceKind, u64>, GowinError> {
    v.as_object()
        .ok_or_else(|| invalid("artifact resource counts"))?
        .iter()
        .map(|(name, n)| {
            let kind = KINDS
                .iter()
                .find(|k| key(k) == *name)
                .copied()
                .ok_or_else(|| invalid("unknown artifact resource"))?;
            Ok((kind, number(n)?))
        })
        .collect()
}
fn expectation(v: ResourceCountExpectation) -> Value {
    match v {
        ResourceCountExpectation::Claimed => json!(["claimed"]),
        ResourceCountExpectation::Exact(n) => json!(["exact", n]),
        ResourceCountExpectation::AtMost(n) => json!(["at-most", n]),
        ResourceCountExpectation::Between { minimum, maximum } => {
            json!(["between", minimum, maximum])
        }
    }
}
fn read_expectation(v: &Value) -> Result<ResourceCountExpectation, GowinError> {
    match string(&v[0])? {
        "claimed" => Ok(ResourceCountExpectation::Claimed),
        "exact" => Ok(ResourceCountExpectation::Exact(number(&v[1])?)),
        "at-most" => Ok(ResourceCountExpectation::AtMost(number(&v[1])?)),
        "between" => {
            let minimum = number(&v[1])?;
            let maximum = number(&v[2])?;
            if minimum > maximum {
                return Err(invalid("artifact expectation bounds"));
            }
            Ok(ResourceCountExpectation::Between { minimum, maximum })
        }
        _ => Err(invalid("artifact resource expectation")),
    }
}
fn source_path(root: &Path, name: &str) -> Result<PathBuf, GowinError> {
    let relative = Path::new(name);
    if relative
        .components()
        .any(|p| !matches!(p, std::path::Component::Normal(_)))
        || name.is_empty()
        || name.contains(['\\', ':'])
        || name.split('/').any(|p| matches!(p, "" | "." | ".."))
    {
        return Err(invalid("artifact source must be a normal relative path"));
    }
    let path = root.join(relative);
    if !fs::canonicalize(&path)?.starts_with(fs::canonicalize(root)?) {
        return Err(invalid("artifact source escapes directory"));
    }
    Ok(path)
}
pub(super) fn verify_sources(
    root: &Path,
    project: &GeneratedGowinProject,
) -> Result<(), GowinError> {
    for (name, source) in &project.files {
        if fs::read(source_path(root, &forward_slashes(name))?)? != source.as_bytes() {
            return Err(invalid(format!(
                "existing source differs from current module: {}",
                name.display()
            )));
        }
    }
    Ok(())
}
pub(super) fn write(root: &Path, project: &GeneratedGowinProject) -> Result<(), GowinError> {
    let r = &project.resources;
    let mut value = json!({"schema":"digitaldesign.gowin-project","version":1,
        "project":project.project_name,"top":project.top_module,"logic_top":project.logic_top_module,
        "target":project.target_name,"device":device(project.device),
        "files":project.files.iter().map(|(name,source)|(forward_slashes(name),hash(source.as_bytes()))).collect::<BTreeMap<_,_>>(),
        "resources":{"capacity":counts(&r.capacity),"claimed":counts(&r.claimed),"fitted_bits":counts(&r.fitted_device_capacity_bits),
            "allocations":r.allocations.iter().map(|a|json!({"id":a.id,"label":a.label,"component":a.component,
                "resources":a.resources.iter().map(|v|json!([key(v.kind),v.amount])).collect::<Vec<_>>()})).collect::<Vec<_>>()},
        "dsp":project.dsp_expectations.iter().map(|(mode,e)|(key(mode),expectation(*e))).collect::<BTreeMap<_,_>>(),
        "bsram":project.bsram_expectation.map(expectation)});
    value["artifact_id"] = json!(hash(
        &serde_json::to_vec(&value).map_err(|e| invalid(e.to_string()))?
    ));
    fs::write(
        root.join(FILE),
        serde_json::to_vec_pretty(&value).map_err(|e| invalid(e.to_string()))?,
    )?;
    Ok(())
}
pub(super) fn load<T: GowinTarget>(root: &Path) -> Result<GeneratedGowinProject, GowinError> {
    let mut value:Value=serde_json::from_slice(&fs::read(root.join(FILE)).map_err(|e|invalid(format!(
        "read {FILE}: {e}; export/build writes this snapshot, or use --seal-existing for a qualified legacy build")))?)
        .map_err(|e|invalid(e.to_string()))?;
    let id = value
        .as_object_mut()
        .ok_or_else(|| invalid("artifact object"))?
        .remove("artifact_id")
        .ok_or_else(|| invalid("artifact identity"))?;
    if value["schema"] != "digitaldesign.gowin-project"
        || value["version"] != 1
        || id != hash(&serde_json::to_vec(&value).map_err(|e| invalid(e.to_string()))?)
        || value["target"] != T::NAME
        || value["device"] != device(T::DEVICE)
    {
        return Err(invalid(
            "artifact schema, identity or fitted target mismatch",
        ));
    }
    let mut files = BTreeMap::new();
    for (name, expected) in value["files"]
        .as_object()
        .ok_or_else(|| invalid("artifact files"))?
    {
        let source = fs::read(source_path(root, name)?)?;
        if *expected != hash(&source) {
            return Err(invalid(format!("artifact source changed: {name}")));
        }
        files.insert(
            PathBuf::from(name),
            String::from_utf8(source).map_err(|e| invalid(e.to_string()))?,
        );
    }
    if files.is_empty() {
        return Err(invalid("empty artifact source closure"));
    }
    let r = &value["resources"];
    let allocations = r["allocations"]
        .as_array()
        .ok_or_else(|| invalid("artifact allocations"))?
        .iter()
        .map(|a| {
            let resources = a["resources"]
                .as_array()
                .ok_or_else(|| invalid("artifact allocation resources"))?
                .iter()
                .map(|v| {
                    let name = string(&v[0])?;
                    let kind = KINDS
                        .iter()
                        .find(|k| key(k) == name)
                        .copied()
                        .ok_or_else(|| invalid("artifact resource kind"))?;
                    Ok(crate::ResourceAmount::new(kind, number(&v[1])?))
                })
                .collect::<Result<Vec<_>, GowinError>>()?;
            Ok(crate::ResourceAllocation {
                id: number(&a["id"])?,
                label: string(&a["label"])?.into(),
                component: string(&a["component"])?.to_owned().into(),
                resources,
            })
        })
        .collect::<Result<Vec<_>, GowinError>>()?;
    let dsp_expectations = value["dsp"]
        .as_object()
        .ok_or_else(|| invalid("artifact DSP plan"))?
        .iter()
        .map(|(name, v)| {
            let mode = MODES
                .iter()
                .find(|m| key(m) == *name)
                .copied()
                .ok_or_else(|| invalid("artifact DSP mode"))?;
            Ok((mode, read_expectation(v)?))
        })
        .collect::<Result<_, GowinError>>()?;
    for field in ["project", "top", "logic_top"] {
        if validate_verilog_identifier(string(&value[field])?).is_err() {
            return Err(invalid("artifact module/project identifier"));
        }
    }
    Ok(GeneratedGowinProject {
        project_name: string(&value["project"])?.into(),
        top_module: string(&value["top"])?.into(),
        logic_top_module: string(&value["logic_top"])?.into(),
        target_name: T::NAME,
        device: T::DEVICE,
        resources: ResourceReport {
            target: T::NAME,
            capacity: read_counts(&r["capacity"])?,
            claimed: read_counts(&r["claimed"])?,
            fitted_device_capacity_bits: read_counts(&r["fitted_bits"])?,
            allocations,
        },
        dsp_expectations,
        bsram_expectation: if value["bsram"].is_null() {
            None
        } else {
            Some(read_expectation(&value["bsram"])?)
        },
        files,
    })
}

/// Validate an exported snapshot and its complete existing fit without current-source generation.
pub fn inspect_gowin_artifact<T: GowinTarget>(directory: &Path) -> Result<Value, GowinError> {
    let project = load::<T>(directory)?;
    verify_fit(directory, &project)?;
    let bitstream = GowinToolchain::from_home("").validate_existing_build(directory, &project)?;
    Ok(
        json!({"schema":"digitaldesign.gowin-audit","version":1,"target":project.target_name,
        "project":project.project_name,"top":project.top_module,"logic_top":project.logic_top_module,
        "device":device(project.device),"bitstream":canonicalize_for_gowin(bitstream)?,
        "descriptor_sha256":hash(&fs::read(directory.join(FILE))?),
        "flash_capacity_bytes":T::inventory().fitted_device_capacity_bits(ResourceKind::SpiFlashDevice).map(|v|v/8)}),
    )
}

pub(super) fn seal_fit(root: &Path, project: &GeneratedGowinProject) -> Result<(), GowinError> {
    let name = &project.project_name;
    let names = [
        FILE.to_owned(),
        build_manifest::FILE_NAME.to_owned(),
        format!("impl/pnr/{name}.fs"),
        format!("impl/pnr/{name}.rpt.txt"),
        format!("impl/pnr/{name}.tr"),
        format!("impl/gwsynthesis/{name}_syn_rsc.xml"),
    ];
    let mut files = BTreeMap::new();
    for name in names {
        files.insert(name.clone(), hash(&fs::read(source_path(root, &name)?)?));
    }
    let mut seal = json!({"schema":"digitaldesign.gowin-fit","version":1,"files":files});
    seal["fit_id"] = json!(hash(
        &serde_json::to_vec(&seal).map_err(|e| invalid(e.to_string()))?
    ));
    fs::write(
        root.join("gowin-fit.json"),
        serde_json::to_vec_pretty(&seal).map_err(|e| invalid(e.to_string()))?,
    )?;
    Ok(())
}
pub(super) fn verify_fit(root: &Path, project: &GeneratedGowinProject) -> Result<(), GowinError> {
    let mut seal: Value = serde_json::from_slice(&fs::read(root.join("gowin-fit.json"))?)
        .map_err(|e| invalid(e.to_string()))?;
    let id = seal
        .as_object_mut()
        .ok_or_else(|| invalid("fit object"))?
        .remove("fit_id")
        .ok_or_else(|| invalid("fit identity"))?;
    if seal["schema"] != "digitaldesign.gowin-fit"
        || seal["version"] != 1
        || id != hash(&serde_json::to_vec(&seal).map_err(|e| invalid(e.to_string()))?)
    {
        return Err(invalid("fit identity/schema"));
    }
    let files = seal["files"]
        .as_object()
        .ok_or_else(|| invalid("fit files"))?;
    let name = &project.project_name;
    let expected = [
        FILE.to_owned(),
        build_manifest::FILE_NAME.to_owned(),
        format!("impl/pnr/{name}.fs"),
        format!("impl/pnr/{name}.rpt.txt"),
        format!("impl/pnr/{name}.tr"),
        format!("impl/gwsynthesis/{name}_syn_rsc.xml"),
    ];
    if files.len() != expected.len() || expected.iter().any(|name| !files.contains_key(name)) {
        return Err(invalid("incomplete fit closure"));
    }
    for (name, expected) in files {
        if *expected != hash(&fs::read(source_path(root, name)?)?) {
            return Err(invalid(format!("fit artifact changed: {name}")));
        }
    }
    Ok(())
}

/// Standalone existing-artifact audit/program entry; never instantiates a Module.
pub fn run_gowin_artifact_cli<T: GowinTarget>() -> Result<(), GowinCliError> {
    let options = parse_gowin_cli_args(std::env::args_os().skip(1))?;
    if options.help {
        println!("gowin-artifact DIRECTORY [--check-existing] [--program-existing | --program-flash OFFSET FILE] [--gowin-home PATH]");
        return Ok(());
    }
    if options.build || options.program || options.audit_current {
        return Err(GowinCliError::Argument(
            "existing-artifact tool cannot build or audit current source".into(),
        ));
    }
    let directory = options
        .output
        .ok_or_else(|| GowinCliError::Argument("hardware directory required".into()))?;
    let inspection = inspect_gowin_artifact::<T>(&directory)?;
    if options.program_existing || options.program_flash.is_some() {
        let toolchain = options
            .gowin_home
            .map(GowinToolchain::from_home)
            .map(Ok)
            .unwrap_or_else(GowinToolchain::discover)?;
        let cable = options
            .cable_index
            .unwrap_or(T::DEVICE.programmer_cable.index());
        if options.program_existing {
            toolchain.program_sram_bitstream(
                T::DEVICE,
                Path::new(string(&inspection["bitstream"])?),
                cable,
            )?;
        }
        if let Some((offset, path)) = options.program_flash {
            let capacity = number(&inspection["flash_capacity_bytes"])?;
            toolchain.program_external_flash_binary(
                T::DEVICE,
                path,
                offset,
                u32::try_from(capacity).map_err(|_| invalid("Flash capacity"))?,
                cable,
            )?;
        }
    }
    println!(
        "{}",
        serde_json::to_string(&inspection).map_err(|e| invalid(e.to_string()))?
    );
    Ok(())
}

#[cfg(test)]
#[path = "artifact_tests.rs"]
mod tests;
