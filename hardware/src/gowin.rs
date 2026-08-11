use crate::{
    write_generated_files, HardwareBackend, HardwareTarget, Module, ProjectError, ResourceError,
    ResourceLease, ResourceReport, Supports, TargetComponent, TargetResources, VerilogProject,
};
use digital_design_code::validate_verilog_identifier;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GowinBackend;

impl HardwareBackend for GowinBackend {
    const NAME: &'static str = "gowin";
}

pub trait GowinTarget: HardwareTarget<Backend = GowinBackend> {
    const DEVICE: GowinDeviceInfo;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GowinDeviceInfo {
    pub device_name: &'static str,
    pub device_version: &'static str,
    pub part_number: &'static str,
    pub project_device_id: &'static str,
    pub programmer_device: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GowinPortDirection {
    Input,
    Output,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GowinPin {
    pub location: u16,
    pub io_type: &'static str,
    pub pull_mode: Option<&'static str>,
    pub drive: Option<u8>,
    pub active_low: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GowinClockPin {
    pub pin: GowinPin,
    pub frequency_hz: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GowinBoundPort {
    board_port: String,
    logic_port: String,
    direction: GowinPortDirection,
    pins: Vec<GowinPin>,
}

/// Physical board IO bound to the typed ports of a generated logic module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GowinBoardBinding<T: GowinTarget> {
    top_module: String,
    clock_port: String,
    logic_clock_port: String,
    clock: GowinClockPin,
    ports: Vec<GowinBoundPort>,
    target: PhantomData<T>,
}

impl<T: GowinTarget> GowinBoardBinding<T> {
    pub fn new(
        top_module: impl Into<String>,
        clock_port: impl Into<String>,
        logic_clock_port: impl Into<String>,
        clock: GowinClockPin,
    ) -> Self {
        Self {
            top_module: top_module.into(),
            clock_port: clock_port.into(),
            logic_clock_port: logic_clock_port.into(),
            clock,
            ports: Vec::new(),
            target: PhantomData,
        }
    }

    pub fn bind_port(
        mut self,
        direction: GowinPortDirection,
        board_port: impl Into<String>,
        logic_port: impl Into<String>,
        pins: impl IntoIterator<Item = GowinPin>,
    ) -> Self {
        self.ports.push(GowinBoundPort {
            board_port: board_port.into(),
            logic_port: logic_port.into(),
            direction,
            pins: pins.into_iter().collect(),
        });
        self
    }

    pub fn top_module(&self) -> &str {
        &self.top_module
    }

    fn render(
        &self,
        logic_top: &str,
        logic_ports: &[(String, String, usize)],
    ) -> Result<BTreeMap<PathBuf, String>, GowinError> {
        self.validate(logic_top, logic_ports)?;
        Ok(BTreeMap::from([
            (
                PathBuf::from("src/generated/board_top.v"),
                self.render_wrapper(logic_top),
            ),
            (PathBuf::from("src/generated/board.cst"), self.render_cst()),
            (PathBuf::from("src/generated/board.sdc"), self.render_sdc()),
        ]))
    }

    fn validate(
        &self,
        logic_top: &str,
        logic_ports: &[(String, String, usize)],
    ) -> Result<(), GowinError> {
        for name in [
            self.top_module.as_str(),
            self.clock_port.as_str(),
            self.logic_clock_port.as_str(),
            logic_top,
        ] {
            validate_verilog_identifier(name).map_err(ProjectError::from)?;
        }
        if self.clock.frequency_hz == 0 {
            return Err(GowinError::InvalidBoardBinding(
                "board clock frequency must be non-zero".to_string(),
            ));
        }
        let mut board_ports = std::collections::HashSet::new();
        let mut bound_logic_ports = std::collections::HashSet::new();
        let contract = logic_ports
            .iter()
            .map(|(name, direction, width)| (name.as_str(), (direction.as_str(), *width)))
            .collect::<std::collections::HashMap<_, _>>();
        match contract.get(self.logic_clock_port.as_str()) {
            Some(&("input", 1)) => {}
            Some(&(direction, width)) => {
                return Err(GowinError::InvalidBoardBinding(format!(
                    "logic clock port `{}` must be a scalar input, found {direction} width {width}",
                    self.logic_clock_port
                )));
            }
            None => {
                return Err(GowinError::InvalidBoardBinding(format!(
                    "logic module `{logic_top}` has no clock port `{}`",
                    self.logic_clock_port
                )));
            }
        }
        for port in &self.ports {
            validate_verilog_identifier(&port.board_port).map_err(ProjectError::from)?;
            validate_verilog_identifier(&port.logic_port).map_err(ProjectError::from)?;
            if port.pins.is_empty() {
                return Err(GowinError::InvalidBoardBinding(format!(
                    "board port `{}` has no pins",
                    port.board_port
                )));
            }
            if !board_ports.insert(&port.board_port) {
                return Err(GowinError::InvalidBoardBinding(format!(
                    "duplicate board port `{}`",
                    port.board_port
                )));
            }
            if !bound_logic_ports.insert(&port.logic_port) {
                return Err(GowinError::InvalidBoardBinding(format!(
                    "duplicate logic port `{}`",
                    port.logic_port
                )));
            }
            let expected_direction = match port.direction {
                GowinPortDirection::Input => "input",
                GowinPortDirection::Output => "output",
            };
            match contract.get(port.logic_port.as_str()) {
                Some(&(direction, width))
                    if direction == expected_direction && width == port.pins.len() => {}
                Some(&(direction, width)) => {
                    return Err(GowinError::InvalidBoardBinding(format!(
                        "logic port `{}` must be {expected_direction} width {}, found {direction} width {width}",
                        port.logic_port,
                        port.pins.len()
                    )));
                }
                None => {
                    return Err(GowinError::InvalidBoardBinding(format!(
                        "logic module `{logic_top}` has no port `{}`",
                        port.logic_port
                    )));
                }
            }
        }
        for (name, _, _) in logic_ports {
            if name != &self.logic_clock_port && !bound_logic_ports.contains(name) {
                return Err(GowinError::InvalidBoardBinding(format!(
                    "logic port `{name}` is not bound to a board resource"
                )));
            }
        }
        Ok(())
    }

    fn render_wrapper(&self, logic_top: &str) -> String {
        let mut output = format!("module {}(\n", self.top_module);
        let mut declarations = vec![format!("    input wire {}", self.clock_port)];
        declarations.extend(self.ports.iter().map(|port| {
            let direction = match port.direction {
                GowinPortDirection::Input => "input",
                GowinPortDirection::Output => "output",
            };
            let width = verilog_width(port.pins.len());
            format!("    {direction} wire {width}{}", port.board_port)
        }));
        output.push_str(&declarations.join(",\n"));
        output.push_str("\n);\n\n");

        for port in &self.ports {
            let needs_adapter = port.pins.iter().any(|pin| pin.active_low);
            if needs_adapter {
                output.push_str(&format!(
                    "wire {}bound_{};\n",
                    verilog_width(port.pins.len()),
                    port.board_port
                ));
                for (index, pin) in port.pins.iter().enumerate() {
                    let board = verilog_bit(&port.board_port, port.pins.len(), index);
                    let bound = verilog_bit(
                        &format!("bound_{}", port.board_port),
                        port.pins.len(),
                        index,
                    );
                    let (left, right) = match port.direction {
                        GowinPortDirection::Input => (bound, board),
                        GowinPortDirection::Output => (board, bound),
                    };
                    let invert = if pin.active_low { "~" } else { "" };
                    output.push_str(&format!("assign {left} = {invert}{right};\n"));
                }
            }
        }

        output.push_str(&format!("\n{logic_top} u_logic(\n"));
        let mut connections = vec![format!(
            "    .{}({})",
            self.logic_clock_port, self.clock_port
        )];
        connections.extend(self.ports.iter().map(|port| {
            let signal = if port.pins.iter().any(|pin| pin.active_low) {
                format!("bound_{}", port.board_port)
            } else {
                port.board_port.clone()
            };
            format!("    .{}({signal})", port.logic_port)
        }));
        output.push_str(&connections.join(",\n"));
        output.push_str("\n);\n\nendmodule\n");
        output
    }

    fn render_cst(&self) -> String {
        let mut output = String::new();
        render_pin_constraint(&mut output, &self.clock_port, self.clock.pin);
        for port in &self.ports {
            for (index, &pin) in port.pins.iter().enumerate() {
                render_pin_constraint(
                    &mut output,
                    &verilog_bit(&port.board_port, port.pins.len(), index),
                    pin,
                );
            }
        }
        output
    }

    fn render_sdc(&self) -> String {
        let period = 1_000_000_000.0 / self.clock.frequency_hz as f64;
        format!(
            "create_clock -name {} -period {period:.6} -waveform {{0 {:.6}}} [get_ports {{{}}}]\n",
            self.clock_port,
            period / 2.0,
            self.clock_port
        )
    }
}

fn verilog_width(width: usize) -> String {
    if width == 1 {
        String::new()
    } else {
        format!("[{}:0] ", width - 1)
    }
}

fn verilog_bit(name: &str, width: usize, index: usize) -> String {
    if width == 1 {
        name.to_string()
    } else {
        format!("{name}[{index}]")
    }
}

fn render_pin_constraint(output: &mut String, port: &str, pin: GowinPin) {
    output.push_str(&format!("IO_LOC \"{port}\" {};\n", pin.location));
    output.push_str(&format!("IO_PORT \"{port}\" IO_TYPE={}", pin.io_type));
    if let Some(pull_mode) = pin.pull_mode {
        output.push_str(&format!(" PULL_MODE={pull_mode}"));
    }
    if let Some(drive) = pin.drive {
        output.push_str(&format!(" DRIVE={drive}"));
    }
    output.push_str(";\n");
}

pub struct GowinProject<T: GowinTarget> {
    project_name: String,
    top_module: String,
    project_sources: BTreeMap<PathBuf, String>,
    resources: TargetResources<T>,
    board_binding: Option<GowinBoardBinding<T>>,
}

impl<T: GowinTarget> GowinProject<T> {
    pub fn new(project_name: impl Into<String>, top_module: impl Into<String>) -> Self {
        Self {
            project_name: project_name.into(),
            top_module: top_module.into(),
            project_sources: BTreeMap::new(),
            resources: TargetResources::new(),
            board_binding: None,
        }
    }

    pub fn resources(&self) -> &TargetResources<T> {
        &self.resources
    }

    pub fn resources_mut(&mut self) -> &mut TargetResources<T> {
        &mut self.resources
    }

    pub fn take<C>(&mut self, component: C) -> Result<ResourceLease<T, C>, ResourceError>
    where
        C: TargetComponent,
        T: Supports<C>,
    {
        self.resources.take(component)
    }

    pub fn take_named<C>(
        &mut self,
        label: impl Into<String>,
        component: C,
    ) -> Result<ResourceLease<T, C>, ResourceError>
    where
        C: TargetComponent,
        T: Supports<C>,
    {
        self.resources.take_named(label, component)
    }

    pub fn add_source_file(
        mut self,
        relative_path: impl Into<PathBuf>,
        content: impl Into<String>,
    ) -> Self {
        let path = relative_path.into();
        assert!(
            path.starts_with("src"),
            "Gowin source files must be placed below src/"
        );
        self.project_sources.insert(path, content.into());
        self
    }

    pub fn with_board_binding(mut self, binding: GowinBoardBinding<T>) -> Self {
        self.board_binding = Some(binding);
        self
    }

    pub fn generate<M: Module>(&self) -> Result<GeneratedGowinProject, GowinError> {
        self.resources.ensure_valid()?;
        if T::DEVICE.project_device_id.is_empty() {
            return Err(GowinError::UnverifiedDeviceConfiguration {
                target: T::NAME,
                part_number: T::DEVICE.part_number,
            });
        }
        validate_verilog_identifier(&self.project_name).map_err(ProjectError::from)?;
        validate_verilog_identifier(&self.top_module).map_err(ProjectError::from)?;
        let verilog = VerilogProject::generate::<M>()?;
        let logic_ports = verilog.top_port_contract()?;
        let mut files = self.project_sources.clone();
        if let Some(binding) = &self.board_binding {
            if binding.top_module() != self.top_module {
                return Err(GowinError::InvalidBoardBinding(format!(
                    "binding top module `{}` does not match project top module `{}`",
                    binding.top_module(),
                    self.top_module
                )));
            }
            for (path, content) in binding.render(&verilog.top_module, &logic_ports)? {
                if files.insert(path.clone(), content).is_some() {
                    return Err(GowinError::DuplicateSourcePath(path));
                }
            }
        }
        for path in files.keys() {
            validate_project_path(path)?;
        }
        for (path, content) in &verilog.files {
            let generated_path = Path::new("src/generated").join(path);
            validate_project_path(&generated_path)?;
            if files
                .insert(generated_path.clone(), content.clone())
                .is_some()
            {
                return Err(GowinError::DuplicateSourcePath(generated_path));
            }
        }
        let resource_report = self.resources.report();
        files.insert(
            PathBuf::from("resource-report.txt"),
            render_resource_report(T::DEVICE, &resource_report),
        );
        let source_paths = files
            .keys()
            .filter(|path| file_type(path).is_some())
            .cloned()
            .collect::<Vec<_>>();
        files.insert(
            PathBuf::from(format!("{}.gprj", self.project_name)),
            render_gprj(T::DEVICE, &source_paths),
        );
        files.insert(
            PathBuf::from("build.tcl"),
            render_build_tcl(
                T::DEVICE,
                &self.project_name,
                &self.top_module,
                &source_paths,
            )?,
        );
        Ok(GeneratedGowinProject {
            project_name: self.project_name.clone(),
            top_module: self.top_module.clone(),
            logic_top_module: verilog.top_module,
            target_name: T::NAME,
            device: T::DEVICE,
            resources: resource_report,
            files,
        })
    }

    pub fn export<M: Module>(
        &self,
        directory: impl AsRef<Path>,
    ) -> Result<GeneratedGowinProject, GowinError> {
        let generated = self.generate::<M>()?;
        generated.write_to(directory)?;
        Ok(generated)
    }
}

#[derive(Clone, Debug)]
pub struct GeneratedGowinProject {
    pub project_name: String,
    pub top_module: String,
    pub logic_top_module: String,
    pub target_name: &'static str,
    pub device: GowinDeviceInfo,
    pub resources: ResourceReport,
    pub files: BTreeMap<PathBuf, String>,
}

impl GeneratedGowinProject {
    pub fn write_to(&self, directory: impl AsRef<Path>) -> Result<(), GowinError> {
        write_generated_files(directory.as_ref(), &self.files)?;
        Ok(())
    }
}

fn file_type(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("v") => Some("file.verilog"),
        Some("cst") => Some("file.cst"),
        Some("sdc") => Some("file.sdc"),
        _ => None,
    }
}

fn render_resource_report(device: GowinDeviceInfo, report: &ResourceReport) -> String {
    let mut output = format!(
        "Target: {}\nDevice: {}\n\nReserved resources (planning values, not PnR results):\n",
        report.target, device.part_number
    );
    for (&kind, &capacity) in &report.capacity {
        let claimed = report.claimed.get(&kind).copied().unwrap_or(0);
        output.push_str(&format!(
            "- {kind}: claimed {claimed}, remaining {}, capacity {capacity}\n",
            capacity - claimed
        ));
    }
    output.push_str("\nComponent allocations:\n");
    for allocation in &report.allocations {
        let resources = allocation
            .resources
            .iter()
            .map(|resource| format!("{} {}", resource.amount, resource.kind))
            .collect::<Vec<_>>()
            .join(", ");
        output.push_str(&format!(
            "- {} [{}]: {}\n",
            allocation.label, allocation.component, resources
        ));
    }
    output
}

fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn xml_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn validate_project_path(path: &Path) -> Result<(), GowinError> {
    if path.is_absolute()
        || !path.starts_with("src")
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(GowinError::UnsafeProjectPath(path.to_path_buf()));
    }
    for component in path.iter() {
        let component = component.to_string_lossy();
        if component.contains(['{', '}', '\n', '\r', '\0']) {
            return Err(GowinError::UnsafeProjectPath(path.to_path_buf()));
        }
    }
    Ok(())
}

fn render_gprj(device: GowinDeviceInfo, paths: &[PathBuf]) -> String {
    let files = paths
        .iter()
        .filter_map(|path| {
            file_type(path).map(|file_type| {
                format!(
                    "        <File path=\"{}\" type=\"{file_type}\" enable=\"1\"/>",
                    xml_attribute(&forward_slashes(path))
                )
            })
        })
        .collect::<Vec<_>>()
        .join("\n");
    let GowinDeviceInfo {
        device_name,
        part_number,
        project_device_id,
        ..
    } = device;
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE gowin-fpga-project>\n\
<Project>\n\
    <Template>FPGA</Template>\n\
    <Version>5</Version>\n\
    <Device name=\"{device_name}\" pn=\"{part_number}\">{project_device_id}</Device>\n\
    <FileList>\n{files}\n    </FileList>\n\
</Project>\n"
    )
}

fn tcl_add_file(path: &Path) -> String {
    let components = path
        .iter()
        .map(|component| format!("{{{}}}", component.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    format!("add_file [file join $here {components}]")
}

fn render_build_tcl(
    device: GowinDeviceInfo,
    project_name: &str,
    top_module: &str,
    paths: &[PathBuf],
) -> Result<String, GowinError> {
    validate_verilog_identifier(project_name).map_err(ProjectError::from)?;
    validate_verilog_identifier(top_module).map_err(ProjectError::from)?;
    for path in paths {
        validate_project_path(path)?;
    }
    let files = paths
        .iter()
        .filter(|path| file_type(path).is_some())
        .map(|path| tcl_add_file(path))
        .collect::<Vec<_>>()
        .join("\n");
    let GowinDeviceInfo {
        device_name,
        device_version,
        part_number,
        ..
    } = device;
    Ok(format!(
        "set here [file normalize [file dirname [info script]]]\n\
cd $here\n\
set_device -name {device_name} -device_version {device_version} {part_number}\n\
{files}\n\
set_option -synthesis_tool gowinsynthesis\n\
set_option -top_module {top_module}\n\
set_option -output_base_name {project_name}\n\
set_option -verilog_std v2001\n\
set_option -print_all_synthesis_warning 1\n\
set_option -gen_text_timing_rpt 1\n\
run all\n"
    ))
}

#[derive(Clone, Debug)]
pub struct GowinToolchain {
    pub home: PathBuf,
}

impl Default for GowinToolchain {
    fn default() -> Self {
        Self {
            home: PathBuf::from(r"D:\DevTools\Gowin\Gowin_V1.9.11.03_Education_x64"),
        }
    }
}

#[derive(Debug)]
pub struct GowinBuildResult {
    pub status: ExitStatus,
    pub bitstream: PathBuf,
    pub synthesis_log: PathBuf,
    pub pnr_report: PathBuf,
    pub timing_report: PathBuf,
    pub warnings: Vec<String>,
    pub device: GowinDeviceInfo,
}

impl GowinToolchain {
    pub fn build(
        &self,
        project_directory: impl AsRef<Path>,
        project: &GeneratedGowinProject,
    ) -> Result<GowinBuildResult, GowinError> {
        let directory = project_directory.as_ref();
        let project_name = &project.project_name;
        let executable = self.home.join("IDE/bin/gw_sh.exe");
        if !executable.is_file() {
            return Err(GowinError::MissingTool(executable));
        }
        let build_tcl = fs::canonicalize(directory.join("build.tcl"))?;
        let status = Command::new(executable)
            .arg(&build_tcl)
            .current_dir(directory)
            .status()?;
        if !status.success() {
            return Err(GowinError::BuildFailed(status));
        }
        let result = GowinBuildResult {
            status,
            bitstream: directory.join(format!("impl/pnr/{project_name}.fs")),
            synthesis_log: directory.join(format!("impl/gwsynthesis/{project_name}.log")),
            pnr_report: directory.join(format!("impl/pnr/{project_name}.rpt.txt")),
            timing_report: directory.join(format!("impl/pnr/{project_name}.tr")),
            warnings: collect_warnings(directory, project_name)?,
            device: project.device,
        };
        if !result.bitstream.is_file() {
            return Err(GowinError::MissingBuildArtifact(result.bitstream));
        }
        audit_timing(&result.timing_report)?;
        Ok(result)
    }

    pub fn program_sram(
        &self,
        build: &GowinBuildResult,
        cable_index: u8,
    ) -> Result<ExitStatus, GowinError> {
        let executable = self.home.join("Programmer/bin/programmer_cli.exe");
        if !executable.is_file() {
            return Err(GowinError::MissingTool(executable));
        }
        // Gowin Programmer 1.9.11.03 parses a relative --fsFile as if no data
        // file was supplied, despite accepting the same path elsewhere.
        let bitstream = if build.bitstream.is_absolute() {
            build.bitstream.clone()
        } else {
            std::env::current_dir()?.join(&build.bitstream)
        };
        let status = Command::new(executable)
            .args([
                "--device",
                build.device.programmer_device,
                "--operation_index",
                "2",
                "--fsFile",
            ])
            .arg(bitstream)
            .args(["--cable-index", &cable_index.to_string()])
            .status()?;
        if !status.success() {
            return Err(GowinError::ProgramFailed(status));
        }
        Ok(status)
    }
}

fn collect_warnings(directory: &Path, project_name: &str) -> Result<Vec<String>, GowinError> {
    let logs = [
        directory.join(format!("impl/gwsynthesis/{project_name}.log")),
        directory.join(format!("impl/pnr/{project_name}.log")),
    ];
    let mut warnings = Vec::new();
    for log in logs {
        if !log.is_file() {
            continue;
        }
        warnings.extend(
            fs::read_to_string(log)?
                .lines()
                .filter(|line| line.contains("WARN"))
                .map(str::to_string),
        );
    }
    Ok(warnings)
}

fn audit_timing(report: &Path) -> Result<(), GowinError> {
    if !report.is_file() {
        return Err(GowinError::MissingBuildArtifact(report.to_path_buf()));
    }
    let report_text = fs::read_to_string(report)?;
    let setup_ok = report_text.contains("<Numbers of Setup Violated Endpoints>:0");
    let hold_ok = report_text.contains("<Numbers of Hold Violated Endpoints>:0");
    let paths_analyzed = timing_count(&report_text, "<Numbers of Paths Analyzed>:");
    let endpoints_analyzed = timing_count(&report_text, "<Numbers of Endpoints Analyzed>:");
    if !setup_ok
        || !hold_ok
        || paths_analyzed.is_none_or(|count| count == 0)
        || endpoints_analyzed.is_none_or(|count| count == 0)
    {
        return Err(GowinError::TimingAuditFailed {
            report: report.to_path_buf(),
            setup_ok,
            hold_ok,
            paths_analyzed,
            endpoints_analyzed,
        });
    }
    Ok(())
}

fn timing_count(report: &str, marker: &str) -> Option<usize> {
    report
        .lines()
        .find_map(|line| line.trim().strip_prefix(marker))
        .and_then(|value| value.trim().parse().ok())
}

#[derive(Debug)]
pub enum GowinError {
    Project(ProjectError),
    Resource(ResourceError),
    Io(std::io::Error),
    MissingTool(PathBuf),
    BuildFailed(ExitStatus),
    ProgramFailed(ExitStatus),
    MissingBuildArtifact(PathBuf),
    UnsafeProjectPath(PathBuf),
    DuplicateSourcePath(PathBuf),
    InvalidBoardBinding(String),
    UnverifiedDeviceConfiguration {
        target: &'static str,
        part_number: &'static str,
    },
    TimingAuditFailed {
        report: PathBuf,
        setup_ok: bool,
        hold_ok: bool,
        paths_analyzed: Option<usize>,
        endpoints_analyzed: Option<usize>,
    },
}

impl Display for GowinError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Project(error) => Display::fmt(error, formatter),
            Self::Resource(error) => Display::fmt(error, formatter),
            Self::Io(error) => Display::fmt(error, formatter),
            Self::MissingTool(path) => {
                write!(formatter, "Gowin tool not found: {}", path.display())
            }
            Self::BuildFailed(status) => write!(formatter, "Gowin build failed with {status}"),
            Self::ProgramFailed(status) => {
                write!(formatter, "Gowin SRAM programming failed with {status}")
            }
            Self::MissingBuildArtifact(path) => {
                write!(formatter, "Gowin did not produce {}", path.display())
            }
            Self::UnsafeProjectPath(path) => {
                write!(formatter, "unsafe Gowin project path: {}", path.display())
            }
            Self::DuplicateSourcePath(path) => {
                write!(formatter, "duplicate Gowin source path: {}", path.display())
            }
            Self::InvalidBoardBinding(message) => {
                write!(formatter, "invalid Gowin board binding: {message}")
            }
            Self::UnverifiedDeviceConfiguration {
                target,
                part_number,
            } => write!(
                formatter,
                "Gowin export for target `{target}` ({part_number}) is disabled until its IDE project device ID has been verified"
            ),
            Self::TimingAuditFailed {
                report,
                setup_ok,
                hold_ok,
                paths_analyzed,
                endpoints_analyzed,
            } => write!(
                formatter,
                "Gowin timing audit failed for {} (setup_ok={setup_ok}, hold_ok={hold_ok}, paths_analyzed={paths_analyzed:?}, endpoints_analyzed={endpoints_analyzed:?})",
                report.display(),
            ),
        }
    }
}

impl std::error::Error for GowinError {}

impl From<ProjectError> for GowinError {
    fn from(value: ProjectError) -> Self {
        Self::Project(value)
    }
}

impl From<ResourceError> for GowinError {
    fn from(value: ResourceError) -> Self {
        Self::Resource(value)
    }
}

impl From<std::io::Error> for GowinError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::examples::basic_adder::{
        basic_adder_gowin_project, BasicAdder, BasicAdderTangNano20K,
    };
    use crate::resources::components::UserLeds;
    use crate::{GowinTarget, HardwareTarget, ResourceKind, TangConsole138KC128M, TangNano20K};

    #[test]
    fn tang_nano_project_contains_all_required_files() {
        let project = basic_adder_gowin_project()
            .generate::<BasicAdderTangNano20K>()
            .unwrap();
        assert!(project.files.contains_key(Path::new("build.tcl")));
        assert!(project.files.contains_key(Path::new("basic_adder.gprj")));
        assert!(project.files.contains_key(Path::new("resource-report.txt")));
        assert!(project.files.contains_key(Path::new(
            "src/generated/examples/basic_adder/basic_adder_board6750000.v"
        )));
        assert!(project
            .files
            .contains_key(Path::new("src/generated/board_top.v")));
        assert!(project
            .files
            .contains_key(Path::new("src/generated/board.cst")));
        assert!(project
            .files
            .contains_key(Path::new("src/generated/board.sdc")));
        let wrapper = &project.files[Path::new("src/generated/board_top.v")];
        assert!(wrapper.contains("input wire [1:0] buttons"));
        assert!(wrapper.contains("assign leds[0] = ~bound_leds[0]"));
        assert!(wrapper.contains("BasicAdderBoard6750000 u_logic"));
        let cst = &project.files[Path::new("src/generated/board.cst")];
        assert!(cst.contains("IO_LOC \"clk\" 4;"));
        assert!(cst.contains("IO_LOC \"buttons[0]\" 88;"));
        assert!(cst.contains("IO_LOC \"leds[5]\" 20;"));
        let sdc = &project.files[Path::new("src/generated/board.sdc")];
        assert!(sdc.contains("-period 37.037037"));
        let tcl = &project.files[Path::new("build.tcl")];
        assert!(tcl.contains("set_option -top_module tang_nano_20k_top"));
        assert!(tcl.contains("set_option -verilog_std v2001"));
        assert!(tcl.contains("add_file [file join $here {src} {generated} {board.sdc}]"));
        let gprj = &project.files[Path::new("basic_adder.gprj")];
        assert!(gprj.contains(project.device.part_number));
        assert!(gprj.contains("src/generated/board.sdc"));
        assert_eq!(project.target_name, "tang-nano-20k");
        assert_eq!(project.resources.claimed[&ResourceKind::UserLed], 6);
        assert_eq!(project.resources.claimed[&ResourceKind::UserButton], 2);
    }

    #[test]
    fn board_binding_rejects_the_wrong_logic_contract() {
        assert!(matches!(
            basic_adder_gowin_project().generate::<BasicAdder>(),
            Err(GowinError::InvalidBoardBinding(message))
                if message.contains("has no port `buttons`")
        ));
    }

    #[test]
    fn tang_console_138k_has_distinct_device_and_memory_resources() {
        let inventory = TangConsole138KC128M::inventory();
        assert_eq!(inventory.capacity(ResourceKind::Lut4), 138_240);
        assert_eq!(inventory.capacity(ResourceKind::Bsram18K), 340);
        assert_eq!(inventory.capacity(ResourceKind::Multiplier18x18), 298);
        assert_eq!(
            inventory.capacity(ResourceKind::Ddr3Bit),
            8_192 * 1_024 * 1_024
        );
        assert_eq!(inventory.capacity(ResourceKind::SdrSdramBit), 0);
        assert_eq!(inventory.capacity(ResourceKind::UserLed), 3);
        assert_eq!(inventory.capacity(ResourceKind::UserButton), 2);
        assert_eq!(
            <TangConsole138KC128M as GowinTarget>::DEVICE.part_number,
            "GW5AST-LV138PG484AC1/I0"
        );
    }

    #[test]
    fn generation_stops_after_a_resource_failure() {
        let mut project = GowinProject::<TangNano20K>::new("failed", "failed_top");
        let error = project.take(UserLeds::<7>).unwrap_err();
        let generation = match project.generate::<BasicAdder>() {
            Ok(_) => panic!("generation continued after a failed allocation"),
            Err(error) => error,
        };
        assert!(matches!(error, ResourceError::CapacityExceeded { .. }));
        assert!(matches!(
            generation,
            GowinError::Resource(ResourceError::AllocatorFailed { .. })
        ));
    }

    #[test]
    fn tang_console_export_uses_verified_ide_identity() {
        let project = GowinProject::<TangConsole138KC128M>::new("console", "console_top");
        let generated = project.generate::<BasicAdder>().unwrap();
        let gprj = &generated.files[Path::new("console.gprj")];
        assert!(gprj.contains("gw5ast138c-007"));
    }

    #[test]
    fn tcl_paths_quote_spaces_and_xml_paths_are_escaped() {
        let paths = vec![PathBuf::from("src/board files/a&b.v")];
        let device = <TangNano20K as GowinTarget>::DEVICE;
        let tcl = render_build_tcl(device, "safe_project", "SafeTop", &paths).unwrap();
        assert!(tcl.contains("{board files}"));
        let gprj = render_gprj(device, &paths);
        assert!(gprj.contains("a&amp;b.v"));
    }

    #[test]
    fn timing_audit_requires_analyzed_paths() {
        let directory =
            std::env::temp_dir().join(format!("digital-design-timing-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let report = directory.join("empty.tr");
        fs::write(
            &report,
            "<Numbers of Paths Analyzed>:0\n<Numbers of Endpoints Analyzed>:0\n<Numbers of Setup Violated Endpoints>:0\n<Numbers of Hold Violated Endpoints>:0\n",
        )
        .unwrap();
        assert!(matches!(
            audit_timing(&report),
            Err(GowinError::TimingAuditFailed {
                paths_analyzed: Some(0),
                ..
            })
        ));
        fs::remove_file(report).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
