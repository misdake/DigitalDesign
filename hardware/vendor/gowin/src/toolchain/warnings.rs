//! Fail-closed warning classification. Raw vendor diagnostics are never edited.
use super::GowinError;
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

struct Cell {
    kind: String,
    name: String,
    ports: Vec<(String, Vec<Net>)>,
}

#[derive(Clone, Eq, PartialEq)]
struct Net {
    name: String,
    hi: Option<u32>,
    lo: Option<u32>,
}

struct MappedDesign {
    cells: BTreeMap<String, Vec<Cell>>,
    uses: BTreeMap<(String, Option<u32>), usize>,
}

impl MappedDesign {
    fn consumed(&self, net: &Net) -> bool {
        if ["GND", "VCC"].contains(&net.name.as_str()) {
            return false;
        }
        match net.lo.zip(net.hi) {
            Some((lo, hi)) => (lo..=hi).all(|bit| {
                self.uses
                    .get(&(net.name.clone(), Some(bit)))
                    .is_some_and(|&uses| uses >= 2)
            }),
            None => self
                .uses
                .get(&(net.name.clone(), None))
                .is_some_and(|&uses| uses >= 2),
        }
    }

    fn preserved_shadow(&self, cell: &Cell, base: &str) -> bool {
        let Some(stem) = cell.name.strip_suffix("_s0") else {
            return false;
        };
        if base != format!("{}_1_Q", stem.rsplit('/').next().unwrap()) {
            return false;
        }
        let key = format!("{}_s1", stem.rsplit('/').next().unwrap());
        self.cells.get(&key).is_some_and(|cells| {
            cells.iter().any(|active| {
                active.kind == "DFFS"
                    && active.name == format!("{stem}_s1")
                    && ["D", "CLK"].iter().all(|port| {
                        let net = |c: &Cell| {
                            c.ports
                                .iter()
                                .find(|(p, _)| p == port)
                                .filter(|(_, ns)| ns.len() == 1)
                                .map(|(_, ns)| ns[0].clone())
                        };
                        net(cell).is_some() && net(cell) == net(active)
                    })
                    && active
                        .ports
                        .iter()
                        .any(|(p, ns)| p == "Q" && ns.iter().any(|n| self.consumed(n)))
            })
        })
    }
    fn parse(text: &str) -> Self {
        let instances = Regex::new(r"(?ms)^(\w+)[ \t]+(\\\S+|\w+)[ \t]+\((.*?)\);").unwrap();
        let ports = Regex::new(r"\.(\w+)\((.*?)\)").unwrap();
        let nets = Regex::new(r"(\\[^\s{},]+|[A-Za-z_]\w*)\s*(?:\[(\d+)(?::(\d+))?\])?").unwrap();
        let mut cells: BTreeMap<String, Vec<Cell>> = BTreeMap::new();
        let mut uses = BTreeMap::new();
        for instance in instances.captures_iter(text) {
            let name = instance[2].trim_start_matches('\\').to_owned();
            let mut connections = Vec::new();
            for port in ports.captures_iter(&instance[3]) {
                let mut signals = Vec::new();
                for net in nets.captures_iter(&port[2]) {
                    let name = net[1].trim_start_matches('\\').to_owned();
                    let hi = net.get(2).and_then(|v| v.as_str().parse().ok());
                    let lo = net.get(3).and_then(|v| v.as_str().parse().ok()).or(hi);
                    if let Some((lo, hi)) = lo.zip(hi) {
                        for bit in lo..=hi {
                            *uses.entry((name.clone(), Some(bit))).or_default() += 1;
                        }
                    } else {
                        *uses.entry((name.clone(), None)).or_default() += 1;
                    }
                    signals.push(Net { name, hi, lo });
                }
                connections.push((port[1].to_owned(), signals));
            }
            cells
                .entry(name.rsplit('/').next().unwrap().into())
                .or_default()
                .push(Cell {
                    kind: instance[1].to_owned(),
                    name,
                    ports: connections,
                });
        }
        Self { cells, uses }
    }

    fn unused_reason(&self, message: &str) -> Option<&'static str> {
        let format = Regex::new(
            r"^Dangling net '(.+)'\(source:'(.+)'\) in module '(.+)' has no destination$",
        )
        .unwrap();
        let m = format.captures(message)?;
        let indexed = Regex::new(r"^(.+)\[(\d+)\]$").unwrap();
        let index = indexed.captures(&m[1]);
        let base = index.as_ref().map_or(&m[1], |v| v.get(1).unwrap().as_str());
        let bit = index.as_ref().and_then(|v| v[2].parse::<u32>().ok());
        let mut reasons = Vec::new();
        for cell in self.cells.get(&m[2])? {
            let prefix = cell.name.rsplit_once('/').map_or("", |(p, _)| p);
            let target = if prefix.is_empty() {
                base.to_owned()
            } else {
                format!("{prefix}/{base}")
            };
            for (port, nets) in &cell.ports {
                if !nets.iter().any(|net| {
                    net.name == target
                        && bit.is_none_or(|b| {
                            net.lo
                                .zip(net.hi)
                                .is_some_and(|(lo, hi)| lo <= b && b <= hi)
                        })
                }) {
                    continue;
                }
                let stub = |stem: &str| {
                    base == stem
                        || base.strip_prefix(stem).is_some_and(|v| {
                            v.starts_with('_')
                                && v.len() > 1
                                && v[1..].chars().all(|c| c.is_ascii_digit())
                        })
                };
                let reason = match (cell.kind.as_str(), port.as_str()) {
                    ("MULT9X9" | "MULT18X18" | "MULTADDALU18X18", "SOA" | "SOB") if stub(port) => {
                        "dsp-unused-shift-output"
                    }
                    ("MULTALU36X18" | "MULTADDALU18X18" | "ALU54D", "CASO") if stub("CASO") => {
                        "dsp-unused-cascade-output"
                    }
                    (
                        "MULT9X9" | "MULT18X18" | "MULT36X36" | "MULTALU36X18" | "MULTADDALU18X18"
                        | "ALU54D",
                        "DOUT",
                    ) if stub("DOUT") => "dsp-unselected-result-bits",
                    (
                        "SDPB" | "SDPX9B" | "DPX9B" | "RAM16SDP4" | "RAM16S4" | "pROM" | "pROMX9",
                        "DO" | "DOA" | "DOB",
                    ) if stub(port) => "memory-physical-output-padding",
                    ("IODELAY", "DF") if base.ends_with("_DF") => {
                        "iodelay-unused-adjustment-status"
                    }
                    ("ALU", "SUM")
                        if cell
                            .ports
                            .iter()
                            .any(|(p, ns)| p == "COUT" && ns.iter().any(|n| self.consumed(n))) =>
                    {
                        "alu-carry-only-stage"
                    }
                    ("rPLL", "CLKOUTP" | "CLKOUTD" | "CLKOUTD3")
                        if ["unused_p", "unused_d", "unused_d3", "clkoutd3_unused"]
                            .contains(&base) =>
                    {
                        "pll-explicitly-unused-output"
                    }
                    ("DFF", "Q") if bit.is_none() && self.preserved_shadow(cell, base) => {
                        "preserved-register-unused-shadow"
                    }
                    _ => return None,
                };
                reasons.push(reason);
            }
        }
        // Ambiguous hierarchy is accepted only if every matched output has the
        // same independently justified primitive use. Unknown ports stay visible.
        let reason = *reasons.first()?;
        reasons.iter().all(|&r| r == reason).then_some(reason)
    }

    fn pll_reference_only(&self, message: &str) -> bool {
        if message != "Generic routing resource will be used to clock signal 'clk_d' by the specified constraint. And then it may lead to the excessive delay or skew" { return false; }
        let mut driver = 0;
        let mut sinks = 0;
        for cell in self.cells.values().flatten() {
            for (port, nets) in &cell.ports {
                if !nets.iter().any(|net| net.name == "clk_d") {
                    continue;
                }
                if cell.kind == "IBUF"
                    && cell.name == "clk_ibuf"
                    && port == "O"
                    && cell
                        .ports
                        .iter()
                        .any(|(p, ns)| p == "I" && ns.len() == 1 && ns[0].name == "clk")
                {
                    driver += 1;
                } else if cell.kind == "rPLL" && port == "CLKIN" {
                    sinks += 1;
                } else {
                    return false;
                }
            }
        }
        driver == 1 && sinks == 2
    }
}

fn source_reason(root: &Path, code: &str, message: &str) -> Option<String> {
    let location = Regex::new(r#"\("(.+)":(\d+)\)$"#)
        .unwrap()
        .captures(message)?;
    let line: usize = location[2].parse().ok()?;
    let locator = location[1].replace('\\', "/");
    let relative = locator
        .rsplit_once("/src/")
        .map(|(_, path)| format!("src/{path}"))?;
    let source = fs::canonicalize(root.join(relative)).ok()?;
    if !source.starts_with(fs::canonicalize(root).ok()?) {
        return None;
    }
    let source = fs::read_to_string(source).ok()?;
    let text = source.lines().nth(line.checked_sub(1)?)?;
    let (_, annotation) = text.split_once(&format!("gowin-lint: allow {code} "))?;
    let (scope, reason) = annotation.split_once("-- ")?;
    if code == "NL0001" {
        let instance = message
            .strip_prefix("Sweep user defined dangling instance \"")?
            .split_once('"')?
            .0;
        let patterns = scope.trim().strip_prefix("instances=")?;
        let range = Regex::new(r"\[(\d+):(\d+)\]").unwrap();
        let matches = patterns.split(',').any(|pattern| {
            if let Some(r) = range.captures(pattern) {
                let lo: u32 = r[1].parse().unwrap_or(u32::MAX);
                let hi: u32 = r[2].parse().unwrap_or(0);
                lo <= hi
                    && hi - lo < 64
                    && (lo..=hi).any(|i| {
                        let suffix = pattern.replace(&r[0], &format!("[{i}]"));
                        instance.ends_with(&format!("/{suffix}"))
                    })
            } else {
                instance.ends_with(&format!("/{pattern}"))
            }
        });
        if !matches {
            return None;
        }
    } else if !scope.trim().is_empty() {
        return None;
    }
    (!reason.trim().is_empty()).then(|| format!("source-local: {}", reason.trim()))
}

fn explicit_unused(root: &Path) -> Result<BTreeMap<(String, String), String>, GowinError> {
    let module = Regex::new(r"(?ms)^[ \t]*module\s+(\w+)\b.*?\bendmodule\b").unwrap();
    let marker = Regex::new(r"(?m)// gowin-lint: unused PA1001 ([^\r\n]+?) -- ([^\r\n]+)").unwrap();
    let mut result = BTreeMap::new();
    let mut directories = vec![root.join("src")];
    while let Some(directory) = directories.pop() {
        if !directory.is_dir() {
            continue;
        }
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            // Do not follow source-directory symlinks while collecting policy.
            if path.is_symlink() {
                continue;
            }
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|v| v == "v") {
                let source = fs::read_to_string(path)?;
                for m in module.captures_iter(&source) {
                    for annotation in marker.captures_iter(&m[0]) {
                        if annotation[2].trim().is_empty() {
                            continue;
                        }
                        for net in annotation[1].split(',') {
                            result.insert(
                                (m[1].to_owned(), net.trim().to_owned()),
                                format!("source-unused: {}", annotation[2].trim()),
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}

/// Read-only diagnostic audit, shared by builds, CLI consumers and frozen-artifact
/// inspection. Unknown diagnostics and incomplete capture are explicit outputs.
pub fn audit_gowin_warnings(directory: &Path, project: &str) -> Result<Value, GowinError> {
    digital_design_circuit::validate_verilog_identifier(project)
        .map_err(super::ProjectError::from)?;
    let mut inputs = BTreeMap::new();
    let mut read = |path: String| -> Result<Option<String>, GowinError> {
        let full = directory.join(&path);
        if !full.is_file() {
            return Ok(None);
        }
        let bytes = fs::read(full)?;
        inputs.insert(path, format!("{:x}", Sha256::digest(&bytes)));
        Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
    };
    let mapped = read(format!("impl/pnr/{project}.vo"))?.map(|v| MappedDesign::parse(&v));
    let synthesis = read(format!("impl/gwsynthesis/{project}.log"))?;
    let synthesis_command = read(format!("impl/gwsynthesis/{project}.prj"))?;
    let pnr = read(format!("impl/pnr/{project}.log"))?;
    let pnr_command = read("impl/pnr/cmd.do".into())?;
    let console = read("gowin-console.log".into())?;
    let complete = synthesis
        .as_ref()
        .is_some_and(|v| v.contains("GowinSynthesis finish"))
        && synthesis_command
            .as_ref()
            .is_some_and(|v| v.contains("type=\"print_all_synthesis_warning\" value=\"1\""))
        && pnr
            .as_ref()
            .is_some_and(|v| v.contains("Bitstream generation completed"))
        && mapped.as_ref().is_some_and(|v| !v.cells.is_empty())
        && pnr_command
            .as_ref()
            .is_some_and(|c| c.lines().any(|line| line.trim() == "-warning_all"));
    let diagnostic = Regex::new(r"^\s*WARN\s+\(([^)]+)\)\s*:\s*(.*)$").unwrap();
    let dangling =
        Regex::new(r"^Dangling net '(.+)'\(source:'.+'\) in module '(.+)' has no destination$")
            .unwrap();
    let explicit = explicit_unused(directory)?;
    let mut entries = Vec::new();
    let mut unknown = Vec::new();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (stage, log) in [("synthesis", synthesis), ("pnr", pnr), ("console", console)] {
        for (line, text) in log.as_deref().unwrap_or("").lines().enumerate() {
            if !text.contains("WARN") {
                continue;
            }
            if !seen.insert(text.trim().to_owned()) {
                continue;
            }
            let fields = diagnostic.captures(text);
            let mut reason = None;
            if let Some(m) = &fields {
                reason = source_reason(directory, &m[1], &m[2]);
                if let Some(mapped) = &mapped {
                    if m[1] == *"PA1001" {
                        reason = mapped.unused_reason(&m[2]).map(str::to_owned);
                    }
                    if m[1] == *"PR1014" && mapped.pll_reference_only(&m[2]) {
                        reason = Some("board-pll-reference-only".into());
                    }
                }
                if m[1] == *"PA1001" && reason.is_none() {
                    if let Some(net) = dangling.captures(&m[2]) {
                        reason = explicit
                            .get(&(net[2].to_owned(), net[1].to_owned()))
                            .cloned();
                    }
                }
            }
            if let Some(reason) = &reason {
                *reasons.entry(reason.clone()).or_default() += 1;
            } else {
                unknown.push(format!("{stage}: {text}"));
            }
            entries.push(json!({"stage":stage,"line":line+1,"text":text,"reason":reason}));
        }
    }
    if !complete {
        unknown.push("Warning capture incomplete: completed synthesis/PnR logs, all-warning command settings and a mapped .vo are required; no warning-clean claim is possible.".into());
    }
    let acknowledged: usize = reasons.values().sum();
    Ok(json!({"schema":"digitaldesign.gowin-warnings","version":1,
        "complete":complete,"total":entries.len(),"unacknowledged_count":unknown.len(),
        "acknowledged_count":acknowledged,"unacknowledged":unknown,
        "acknowledged_by_reason":reasons,"inputs_sha256":inputs,"entries":entries}))
}

pub(super) fn summary(mut report: Value) -> Value {
    report
        .as_object_mut()
        .expect("warning report")
        .remove("entries");
    report
}

pub(super) fn print_summary(report: &Value) {
    let warnings = report["unacknowledged"].as_array().expect("warning array");
    println!(
        "Gowin warning audit: {} unacknowledged; {} raw warnings; complete={}.",
        warnings.len(),
        report["total"],
        report["complete"]
    );
    for warning in warnings {
        eprintln!("Gowin warning: {}", warning.as_str().unwrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dangling(net: &str, source: &str) -> String {
        format!("Dangling net '{net}'(source:'{source}') in module 'test' has no destination")
    }

    #[test]
    fn unused_primitive_ports_require_matching_mapped_connection_and_stub() {
        let design = MappedDesign::parse("MULT9X9 \\p/dsp  (\n.DOUT({\\p/DOUT_0 [17:16], \\p/color [15:0]}),\n.SOA(\\p/SOA [8:0])\n);\n");
        assert_eq!(
            design.unused_reason(&dangling("DOUT_0[16]", "dsp")),
            Some("dsp-unselected-result-bits")
        );
        assert_eq!(
            design.unused_reason(&dangling("SOA[8]", "dsp")),
            Some("dsp-unused-shift-output")
        );
        for (net, source) in [
            ("DOUT_0[15]", "dsp"),
            ("color[15]", "dsp"),
            ("SOA[9]", "dsp"),
            ("SOA[8]", "new_dsp"),
        ] {
            assert!(design.unused_reason(&dangling(net, source)).is_none());
        }
        let wrong = MappedDesign::parse("DFF \\p/dsp  (\n.Q(\\p/SOA [8:0])\n);\n");
        assert!(wrong.unused_reason(&dangling("SOA[8]", "dsp")).is_none());
    }

    #[test]
    fn unused_alu_sum_requires_a_consumed_carry() {
        let carry = "ALU \\p/add  (\n.SUM(\\p/sum ),\n.COUT(\\p/carry )\n);\n";
        let design = MappedDesign::parse(carry);
        assert!(design.unused_reason(&dangling("sum", "add")).is_none());
        let design =
            MappedDesign::parse(&format!("{carry}ALU \\p/next  (\n.CIN(\\p/carry )\n);\n"));
        assert_eq!(
            design.unused_reason(&dangling("sum", "add")),
            Some("alu-carry-only-stage")
        );
        let different_bit = MappedDesign::parse("ALU \\p/add  (\n.SUM(\\p/sum ),\n.COUT(\\p/carry [0])\n);\nALU \\p/next  (\n.CIN(\\p/carry [1])\n);\n");
        assert!(different_bit
            .unused_reason(&dangling("sum", "add"))
            .is_none());
    }

    #[test]
    fn reference_clock_exception_rejects_added_fabric_consumers() {
        let warning = "Generic routing resource will be used to clock signal 'clk_d' by the specified constraint. And then it may lead to the excessive delay or skew";
        let clock = "IBUF clk_ibuf (\n.I(clk),\n.O(clk_d)\n);\nrPLL p (\n.CLKIN(clk_d)\n);\nrPLL q (\n.CLKIN(clk_d)\n);\n";
        assert!(MappedDesign::parse(clock).pll_reference_only(warning));
        let fabric = format!("{clock}DFF state (\n.CLK(clk_d)\n);\n");
        assert!(!MappedDesign::parse(&fabric).pll_reference_only(warning));
        assert!(!MappedDesign::parse(clock)
            .pll_reference_only(&warning.replace("clk_d", "other_clock")));
    }

    #[test]
    fn audit_keeps_new_codes_and_incomplete_logs_visible() {
        let root = std::env::temp_dir().join(format!("gowin-warnings-{}", std::process::id()));
        fs::create_dir_all(root.join("src/generated")).unwrap();
        fs::create_dir_all(root.join("impl/gwsynthesis")).unwrap();
        fs::write(
            root.join("src/generated/test.v"),
            "wire x; // gowin-lint: allow EX9999 -- Known unused diagnostic output.\n",
        )
        .unwrap();
        fs::write(root.join("impl/gwsynthesis/test.log"), "WARN  (EX9999) : diagnostic(\"old/src/generated/test.v\":1)\nWARN (EX9998) : new warning(\"old/src/generated/test.v\":1)\n").unwrap();
        fs::write(root.join("gowin-console.log"), "WARN (EX9998) : new warning(\"old/src/generated/test.v\":1)\nWARN (NEW01) : console only\n").unwrap();
        let report = audit_gowin_warnings(&root, "test").unwrap();
        assert_eq!(report["total"], 3);
        assert_eq!(report["complete"], false);
        let unknown = report["unacknowledged"].as_array().unwrap();
        assert_eq!(unknown.len(), 3);
        assert!(unknown[0].as_str().unwrap().contains("EX9998"));
        assert!(unknown[1].as_str().unwrap().contains("console only"));
        assert!(unknown[2].as_str().unwrap().contains("incomplete"));
        assert!(summary(report).get("entries").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_exceptions_require_exact_module_nets_and_bounded_instances() {
        let root =
            std::env::temp_dir().join(format!("gowin-warning-scopes-{}", std::process::id()));
        fs::create_dir_all(root.join("src/generated")).unwrap();
        fs::write(root.join("src/generated/test.v"), "module reader();\n// gowin-lint: unused PA1001 count[0],count[1] -- Optional low occupancy bits.\nRAM16SDP4 r(); // gowin-lint: allow NL0001 instances=reader/lane[1:4].a -- Known unused reader lanes.\nendmodule\n").unwrap();
        let explicit = explicit_unused(&root).unwrap();
        assert!(explicit.contains_key(&("reader".into(), "count[1]".into())));
        assert!(!explicit.contains_key(&("reader".into(), "count[2]".into())));
        assert!(!explicit.contains_key(&("other".into(), "count[1]".into())));
        let message = |i| {
            format!("Sweep user defined dangling instance \"top/reader/lane[{i}].a\"(\"old/src/generated/test.v\":3)")
        };
        assert!(source_reason(&root, "NL0001", &message(1)).is_some());
        assert!(source_reason(&root, "NL0001", &message(4)).is_some());
        assert!(source_reason(&root, "NL0001", &message(5)).is_none());
        assert!(source_reason(&root, "NL0002", &message(1)).is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
