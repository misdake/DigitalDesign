use crate::{IoBinding, Module, ModuleIo};
use digital_design_code::{
    build_circuit, render_verilog_module, validate_verilog_identifier, ExportGateReg,
    VerilogConnection, VerilogInstance, VerilogModule, VerilogPort,
};
use std::any::TypeId;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    Render(digital_design_code::VerilogRenderError),
    DependencyCycle(String),
    DuplicateModuleName(String),
    DuplicateOutputPath(PathBuf),
    InvalidHandwrittenVerilog(String),
    UnsafeOutputPath(PathBuf),
    UnmanagedFileConflict(PathBuf),
}

impl Display for ProjectError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Render(error) => Display::fmt(error, formatter),
            Self::DependencyCycle(name) => {
                write!(formatter, "Verilog dependency cycle at `{name}`")
            }
            Self::DuplicateModuleName(name) => {
                write!(formatter, "duplicate Verilog module `{name}`")
            }
            Self::DuplicateOutputPath(path) => {
                write!(formatter, "multiple modules export to `{}`", path.display())
            }
            Self::InvalidHandwrittenVerilog(message) => formatter.write_str(message),
            Self::UnsafeOutputPath(path) => {
                write!(
                    formatter,
                    "unsafe generated output path `{}`",
                    path.display()
                )
            }
            Self::UnmanagedFileConflict(path) => write!(
                formatter,
                "refusing to overwrite unmanaged file `{}`",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ProjectError {}

impl From<std::io::Error> for ProjectError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<digital_design_code::VerilogRenderError> for ProjectError {
    fn from(value: digital_design_code::VerilogRenderError) -> Self {
        Self::Render(value)
    }
}

#[derive(Clone)]
struct ModuleDescriptor {
    type_id: TypeId,
    rust_name: &'static str,
    module_name: String,
    relative_path: PathBuf,
    build: fn() -> RawModule,
}

struct RecordedInstance {
    descriptor: ModuleDescriptor,
    instance_name: String,
    bindings: Vec<IoBinding>,
}

struct RawModule {
    descriptor: ModuleDescriptor,
    source: Option<&'static str>,
    content: ExportGateReg,
    inputs: Vec<VerilogPort>,
    outputs: Vec<VerilogPort>,
    instances: Vec<RecordedInstance>,
    base_clocked: bool,
}

thread_local! {
    static RECORDED_INSTANCES: RefCell<Option<Vec<RecordedInstance>>> = const { RefCell::new(None) };
}

struct RecordingGuard {
    active: bool,
}

impl RecordingGuard {
    fn start() -> Self {
        RECORDED_INSTANCES.with(|instances| {
            assert!(
                instances.borrow().is_none(),
                "nested Verilog module generation is not supported"
            );
            *instances.borrow_mut() = Some(Vec::new());
        });
        Self { active: true }
    }

    fn finish(mut self) -> Vec<RecordedInstance> {
        self.active = false;
        RECORDED_INSTANCES.with(|recorded| recorded.borrow_mut().take().unwrap())
    }
}

impl Drop for RecordingGuard {
    fn drop(&mut self) {
        if self.active {
            RECORDED_INSTANCES.with(|recorded| {
                recorded.borrow_mut().take();
            });
        }
    }
}

fn split_type_name(type_name: &str) -> Vec<&str> {
    type_name
        .split('<')
        .next()
        .unwrap_or(type_name)
        .split("::")
        .collect()
}

fn descriptor<M: Module>() -> ModuleDescriptor {
    let rust_name = std::any::type_name::<M>();
    let parts = split_type_name(rust_name);
    let module_name = M::verilog_name();
    let default_module_name = parts.last().unwrap();
    let relative_path = if parts.len() > 2 {
        let mut path = PathBuf::new();
        for part in &parts[1..parts.len() - 2] {
            path.push(part);
        }
        let file_stem = if module_name == *default_module_name {
            parts[parts.len() - 2].to_string()
        } else {
            to_snake_case(&module_name)
        };
        path.push(format!("{file_stem}.v"));
        path
    } else {
        PathBuf::from(format!("{}.v", to_snake_case(&module_name)))
    };
    ModuleDescriptor {
        type_id: TypeId::of::<M>(),
        rust_name,
        module_name,
        relative_path,
        build: build_raw::<M>,
    }
}

fn to_snake_case(value: &str) -> String {
    let mut result = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() && index != 0 {
            result.push('_');
        }
        result.push(character.to_ascii_lowercase());
    }
    result
}

fn ports(bindings: Vec<IoBinding>) -> Vec<VerilogPort> {
    bindings
        .into_iter()
        .map(|binding| VerilogPort::bus(binding.name, binding.wires))
        .collect()
}

fn build_raw<M: Module>() -> RawModule {
    let recording = RecordingGuard::start();

    let source = M::verilog_source();
    let (circuit, (input, output)) = build_circuit(|| {
        let input = M::Input::allocate();
        let output = if source.is_some() {
            M::Output::allocate()
        } else {
            M::build_verilog(&input)
        };
        (input, output)
    });
    let instances = recording.finish();
    let content = circuit.export_gate_reg();
    let base_clocked = M::USES_MAIN_CLOCK || !content.regs.is_empty();

    RawModule {
        descriptor: descriptor::<M>(),
        source,
        content,
        inputs: ports(input.bindings()),
        outputs: ports(output.bindings()),
        instances,
        base_clocked,
    }
}

pub(crate) fn record_instance<M: Module>(input: &M::Input) -> M::Output {
    let output = M::Output::allocate();
    RECORDED_INSTANCES.with(|instances| {
        let mut instances = instances.borrow_mut();
        let recorded = instances.as_mut().unwrap_or_else(|| {
            panic!(
                "`{}::verilog` called outside VerilogProject",
                std::any::type_name::<M>()
            )
        });
        let descriptor = descriptor::<M>();
        let instance_name = format!(
            "u_{}_{}",
            to_snake_case(&descriptor.module_name),
            recorded.len()
        );
        let mut bindings = input.bindings();
        bindings.extend(output.bindings());
        recorded.push(RecordedInstance {
            descriptor,
            instance_name,
            bindings,
        });
    });
    output
}

#[derive(Clone, Debug)]
pub struct GeneratedVerilogProject {
    pub top_module: String,
    pub files: BTreeMap<PathBuf, String>,
}

impl GeneratedVerilogProject {
    pub fn write_to(&self, directory: impl AsRef<Path>) -> Result<(), ProjectError> {
        write_generated_files(directory.as_ref(), &self.files)
    }

    pub(crate) fn top_port_contract(&self) -> Result<Vec<(String, String, usize)>, ProjectError> {
        for source in self.files.values() {
            if find_module_port_list(source, &self.top_module).is_ok() {
                return parse_header_ports(source, &self.top_module);
            }
        }
        Err(ProjectError::InvalidHandwrittenVerilog(format!(
            "generated project does not contain top module `{}`",
            self.top_module
        )))
    }
}

pub struct VerilogProject;

impl VerilogProject {
    pub fn generate<M: Module>() -> Result<GeneratedVerilogProject, ProjectError> {
        let root = descriptor::<M>();
        let top_module = root.module_name.clone();
        let mut resolver = Resolver::default();
        resolver.visit(root)?;
        Ok(GeneratedVerilogProject {
            top_module,
            files: resolver.files,
        })
    }

    pub fn export<M: Module>(directory: impl AsRef<Path>) -> Result<(), ProjectError> {
        Self::generate::<M>()?.write_to(directory)
    }
}

#[derive(Default)]
struct Resolver {
    visiting: HashSet<TypeId>,
    completed: HashMap<TypeId, bool>,
    module_names: HashSet<String>,
    paths: HashSet<PathBuf>,
    files: BTreeMap<PathBuf, String>,
}

impl Resolver {
    fn visit(&mut self, descriptor: ModuleDescriptor) -> Result<bool, ProjectError> {
        if let Some(clocked) = self.completed.get(&descriptor.type_id) {
            return Ok(*clocked);
        }
        if !self.visiting.insert(descriptor.type_id) {
            return Err(ProjectError::DependencyCycle(
                descriptor.rust_name.to_string(),
            ));
        }

        let raw = (descriptor.build)();
        let mut child_clocking = HashMap::new();
        for instance in &raw.instances {
            let clocked = self.visit(instance.descriptor.clone())?;
            child_clocking.insert(instance.descriptor.type_id, clocked);
        }
        let clocked = raw.base_clocked || child_clocking.values().any(|clocked| *clocked);

        if !self.module_names.insert(raw.descriptor.module_name.clone()) {
            return Err(ProjectError::DuplicateModuleName(
                raw.descriptor.module_name,
            ));
        }
        if !self.paths.insert(raw.descriptor.relative_path.clone()) {
            return Err(ProjectError::DuplicateOutputPath(
                raw.descriptor.relative_path,
            ));
        }

        let source = if let Some(source) = raw.source {
            validate_handwritten(&raw, source, clocked)?;
            source.to_string()
        } else {
            let instances = raw
                .instances
                .iter()
                .map(|instance| {
                    let mut connections = instance
                        .bindings
                        .iter()
                        .map(|binding| {
                            (
                                binding.name.to_string(),
                                VerilogConnection::Wires(binding.wires.clone()),
                            )
                        })
                        .collect::<Vec<_>>();
                    if child_clocking[&instance.descriptor.type_id] {
                        connections.push((
                            "clk".to_string(),
                            VerilogConnection::Signal("clk".to_string()),
                        ));
                    }
                    VerilogInstance {
                        module_name: instance.descriptor.module_name.clone(),
                        instance_name: instance.instance_name.clone(),
                        connections,
                    }
                })
                .collect();
            render_verilog_module(
                &VerilogModule {
                    module_name: raw.descriptor.module_name.clone(),
                    clock: clocked.then(|| "clk".to_string()),
                    inputs: raw.inputs,
                    outputs: raw.outputs,
                    instances,
                },
                &raw.content,
            )?
        };
        self.files.insert(raw.descriptor.relative_path, source);
        self.visiting.remove(&descriptor.type_id);
        self.completed.insert(descriptor.type_id, clocked);
        Ok(clocked)
    }
}

fn expected_port_tokens(port: &VerilogPort, direction: &str) -> (String, String, usize) {
    (port.name.clone(), direction.to_string(), port.wires.len())
}

fn parse_header_ports(
    source: &str,
    module_name: &str,
) -> Result<Vec<(String, String, usize)>, ProjectError> {
    let source = strip_verilog_comments(source)?;
    let header = find_module_port_list(&source, module_name)?;
    let mut direction: Option<String> = None;
    let mut width = 1;
    split_top_level(header, ',')
        .into_iter()
        .map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return Err(ProjectError::InvalidHandwrittenVerilog(
                    "empty module port declaration".to_string(),
                ));
            }
            let mut declaration = entry.to_string();
            if let Some(rest) = strip_word_prefix(&declaration, "input") {
                direction = Some("input".to_string());
                width = 1;
                declaration = rest.to_string();
            } else if let Some(rest) = strip_word_prefix(&declaration, "output") {
                direction = Some("output".to_string());
                width = 1;
                declaration = rest.to_string();
            }
            let current_direction = direction.clone().ok_or_else(|| {
                ProjectError::InvalidHandwrittenVerilog(format!(
                    "port declaration `{entry}` needs an explicit initial direction"
                ))
            })?;
            if let Some(open) = declaration.find('[') {
                let close = declaration[open + 1..]
                    .find(']')
                    .map(|offset| open + 1 + offset)
                    .ok_or_else(|| {
                        ProjectError::InvalidHandwrittenVerilog(format!(
                            "unterminated range in `{entry}`"
                        ))
                    })?;
                width = parse_port_width(&declaration[open + 1..close])?;
                declaration.replace_range(open..=close, " ");
            }
            let before_default = declaration.split('=').next().unwrap().trim();
            let name = before_default
                .split_whitespace()
                .last()
                .ok_or_else(|| {
                    ProjectError::InvalidHandwrittenVerilog(format!(
                        "missing port name in `{entry}`"
                    ))
                })?
                .trim()
                .to_string();
            validate_verilog_identifier(&name).map_err(ProjectError::Render)?;
            Ok((name, current_direction, width))
        })
        .collect()
}

fn strip_verilog_comments(source: &str) -> Result<String, ProjectError> {
    let bytes = source.as_bytes();
    let mut output = bytes.to_vec();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            let start = index;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            output[start..index].fill(b' ');
        } else if bytes[index..].starts_with(b"/*") {
            let start = index;
            index += 2;
            while index + 1 < bytes.len() && !bytes[index..].starts_with(b"*/") {
                index += 1;
            }
            if index + 1 >= bytes.len() {
                return Err(ProjectError::InvalidHandwrittenVerilog(
                    "unterminated block comment".to_string(),
                ));
            }
            index += 2;
            for byte in &mut output[start..index] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
        } else {
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| {
        ProjectError::InvalidHandwrittenVerilog("Verilog source must be UTF-8".to_string())
    })
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn skip_whitespace(bytes: &[u8], cursor: &mut usize) {
    while *cursor < bytes.len() && bytes[*cursor].is_ascii_whitespace() {
        *cursor += 1;
    }
}

fn identifier_at(source: &str, cursor: &mut usize) -> Option<String> {
    let bytes = source.as_bytes();
    skip_whitespace(bytes, cursor);
    let start = *cursor;
    while *cursor < bytes.len() && is_identifier_byte(bytes[*cursor]) {
        *cursor += 1;
    }
    (start != *cursor).then(|| source[start..*cursor].to_string())
}

fn balanced_close(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_module_port_list<'a>(source: &'a str, module_name: &str) -> Result<&'a str, ProjectError> {
    let bytes = source.as_bytes();
    let mut search = 0;
    while search + 6 <= bytes.len() {
        let Some(offset) = source[search..].find("module") else {
            break;
        };
        let start = search + offset;
        let before_ok = start == 0 || !is_identifier_byte(bytes[start - 1]);
        let after = start + 6;
        let after_ok = after == bytes.len() || !is_identifier_byte(bytes[after]);
        search = after;
        if !before_ok || !after_ok {
            continue;
        }
        let mut cursor = after;
        if identifier_at(source, &mut cursor).as_deref() != Some(module_name) {
            continue;
        }
        skip_whitespace(bytes, &mut cursor);
        if bytes.get(cursor) == Some(&b'#') {
            cursor += 1;
            skip_whitespace(bytes, &mut cursor);
            if bytes.get(cursor) != Some(&b'(') {
                return Err(ProjectError::InvalidHandwrittenVerilog(
                    "expected parameter list after `#`".to_string(),
                ));
            }
            cursor = balanced_close(source, cursor).ok_or_else(|| {
                ProjectError::InvalidHandwrittenVerilog(
                    "unterminated module parameter list".to_string(),
                )
            })? + 1;
            skip_whitespace(bytes, &mut cursor);
        }
        if bytes.get(cursor) != Some(&b'(') {
            return Err(ProjectError::InvalidHandwrittenVerilog(
                "missing module port list".to_string(),
            ));
        }
        let close = balanced_close(source, cursor).ok_or_else(|| {
            ProjectError::InvalidHandwrittenVerilog("unterminated module port list".to_string())
        })?;
        return Ok(&source[cursor + 1..close]);
    }
    Err(ProjectError::InvalidHandwrittenVerilog(format!(
        "hand-written Verilog does not declare `module {module_name}`"
    )))
}

fn split_top_level(source: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depths = [0usize; 3];
    for (index, character) in source.char_indices() {
        match character {
            '(' => depths[0] += 1,
            ')' => depths[0] = depths[0].saturating_sub(1),
            '[' => depths[1] += 1,
            ']' => depths[1] = depths[1].saturating_sub(1),
            '{' => depths[2] += 1,
            '}' => depths[2] = depths[2].saturating_sub(1),
            value if value == separator && depths == [0, 0, 0] => {
                parts.push(&source[start..index]);
                start = index + value.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&source[start..]);
    parts
}

fn strip_word_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let value = value.trim_start();
    let rest = value.strip_prefix(prefix)?;
    rest.chars()
        .next()
        .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
        .then_some(rest.trim_start())
}

fn parse_port_width(range: &str) -> Result<usize, ProjectError> {
    let (high, low) = range.split_once(':').ok_or_else(|| {
        ProjectError::InvalidHandwrittenVerilog(format!("invalid range `[{range}]`"))
    })?;
    let high = high.trim().parse::<usize>().map_err(|_| {
        ProjectError::InvalidHandwrittenVerilog(format!("non-constant range `[{range}]`"))
    })?;
    let low = low.trim().parse::<usize>().map_err(|_| {
        ProjectError::InvalidHandwrittenVerilog(format!("non-constant range `[{range}]`"))
    })?;
    Ok(high.abs_diff(low) + 1)
}

fn validate_handwritten(raw: &RawModule, source: &str, clocked: bool) -> Result<(), ProjectError> {
    let mut expected = raw
        .inputs
        .iter()
        .map(|port| expected_port_tokens(port, "input"))
        .chain(
            raw.outputs
                .iter()
                .map(|port| expected_port_tokens(port, "output")),
        )
        .collect::<Vec<_>>();
    if clocked {
        expected.push(("clk".to_string(), "input".to_string(), 1));
    }
    let mut actual = parse_header_ports(source, &raw.descriptor.module_name)?;
    expected.sort();
    actual.sort();
    if actual.windows(2).any(|ports| ports[0].0 == ports[1].0) {
        return Err(ProjectError::InvalidHandwrittenVerilog(format!(
            "duplicate port in `{}`",
            raw.descriptor.module_name
        )));
    }
    if actual != expected {
        return Err(ProjectError::InvalidHandwrittenVerilog(format!(
            "signature mismatch for `{}`: expected {:?}, found {:?}",
            raw.descriptor.module_name, expected, actual
        )));
    }
    Ok(())
}

pub(crate) fn write_generated_files(
    directory: &Path,
    files: &BTreeMap<PathBuf, String>,
) -> Result<(), ProjectError> {
    fs::create_dir_all(directory)?;
    let manifest_path = directory.join(".digital-design-generated");
    let old_manifest = fs::read_to_string(&manifest_path).unwrap_or_default();
    let old_files = old_manifest
        .lines()
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect::<HashSet<_>>();
    for relative in files.keys().chain(&old_files) {
        validate_output_path(relative)?;
    }
    for relative in files.keys() {
        let target = directory.join(relative);
        if target.exists() && (!old_files.contains(relative) || !target.is_file()) {
            return Err(ProjectError::UnmanagedFileConflict(target));
        }
    }

    let staging = create_transaction_directory(directory, "stage")?;
    let backup = match create_transaction_directory(directory, "backup") {
        Ok(backup) => backup,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    let prepare_result = (|| -> Result<(), ProjectError> {
        for (relative, content) in files {
            let staged = staging.join(relative);
            if let Some(parent) = staged.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(staged, content)?;
        }
        let manifest = files
            .keys()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(staging.join("manifest"), format!("{manifest}\n"))?;
        Ok(())
    })();
    if let Err(error) = prepare_result {
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_dir_all(&backup);
        return Err(error);
    }

    let mut backed_up = Vec::new();
    let mut installed = Vec::new();
    let mut manifest_backed_up = false;
    let mut manifest_installed = false;
    let commit_result = (|| -> Result<(), ProjectError> {
        for relative in &old_files {
            let target = directory.join(relative);
            if target.is_file() {
                let saved = backup.join(relative);
                if let Some(parent) = saved.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(&target, saved)?;
                backed_up.push(relative.clone());
            }
        }
        if manifest_path.is_file() {
            fs::rename(&manifest_path, backup.join("manifest"))?;
            manifest_backed_up = true;
        }
        for relative in files.keys() {
            let target = directory.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(staging.join(relative), &target)?;
            installed.push(relative.clone());
        }
        fs::rename(staging.join("manifest"), &manifest_path)?;
        manifest_installed = true;
        Ok(())
    })();

    if let Err(error) = commit_result {
        if manifest_installed {
            let _ = fs::remove_file(&manifest_path);
        }
        for relative in installed.iter().rev() {
            let _ = fs::remove_file(directory.join(relative));
        }
        for relative in backed_up.iter().rev() {
            let saved = backup.join(relative);
            let target = directory.join(relative);
            if let Some(parent) = target.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::rename(saved, target);
        }
        if manifest_backed_up {
            let _ = fs::rename(backup.join("manifest"), &manifest_path);
        }
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_dir_all(&backup);
        return Err(error);
    }

    fs::remove_dir_all(staging)?;
    fs::remove_dir_all(backup)?;
    Ok(())
}

fn validate_output_path(relative: &Path) -> Result<(), ProjectError> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(ProjectError::UnsafeOutputPath(relative.to_path_buf()));
    }
    let first = relative
        .components()
        .next()
        .and_then(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .unwrap_or_default();
    if first == ".digital-design-generated"
        || first.starts_with(".digital-design-stage-")
        || first.starts_with(".digital-design-backup-")
    {
        return Err(ProjectError::UnsafeOutputPath(relative.to_path_buf()));
    }
    Ok(())
}

fn create_transaction_directory(directory: &Path, label: &str) -> Result<PathBuf, ProjectError> {
    for nonce in 0..1000u32 {
        let candidate = directory.join(format!(
            ".digital-design-{label}-{}-{nonce}",
            std::process::id()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(ProjectError::Io(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a generated-file transaction directory",
    )))
}

#[cfg(test)]
mod file_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "digital-design-hardware-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn export_refuses_to_replace_an_unmanaged_file() {
        let directory = temporary_directory("conflict");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("module.v"), "user source").unwrap();
        let files = BTreeMap::from([(PathBuf::from("module.v"), "generated".to_string())]);

        let error = write_generated_files(&directory, &files).unwrap_err();
        assert!(matches!(error, ProjectError::UnmanagedFileConflict(_)));
        assert_eq!(
            fs::read_to_string(directory.join("module.v")).unwrap(),
            "user source"
        );

        fs::remove_file(directory.join("module.v")).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn export_rejects_unsafe_paths_from_a_tampered_manifest() {
        let directory = temporary_directory("unsafe-manifest");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(".digital-design-generated"),
            "../outside.v\n",
        )
        .unwrap();

        let error = write_generated_files(&directory, &BTreeMap::new()).unwrap_err();
        assert!(matches!(error, ProjectError::UnsafeOutputPath(_)));

        fs::remove_file(directory.join(".digital-design-generated")).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
