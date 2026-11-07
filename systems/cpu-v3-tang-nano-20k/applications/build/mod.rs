//! Application-owned recipes shared by the build script and host command.
use cpu_v3_boot_format::{parse_boot_project, BootProject};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[path = "../../examples/gpu_bench/application.rs"]
mod bench;
#[path = "../../examples/gpu_renderer_bloxorz/application.rs"]
mod bloxorz;
#[path = "../../examples/gpu_demo/application.rs"]
mod demo;
#[path = "../../examples/dualshock2/application.rs"]
mod ds2;
#[path = "../../examples/dualshock2_ui/application.rs"]
mod ds2_ui;
#[path = "../../examples/gpu_profiler_board/application.rs"]
mod profiler;
#[path = "../../examples/gpu_renderer/application.rs"]
mod renderer;
#[path = "../../examples/gpu_renderer_rtt/application.rs"]
mod rtt;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SourceSpecialization {
    Standard,
    ProfilerMemcpy,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CapturePolicy {
    Shared,
    Disabled,
    Diagnostic,
}
impl CapturePolicy {
    pub fn hook_enabled(self, setting: Option<&str>) -> bool {
        self != Self::Disabled && setting != Some("0")
    }
    pub fn requires_manifest(self, setting: Option<&str>) -> bool {
        self == Self::Diagnostic && self.hook_enabled(setting)
    }
}
pub struct ApplicationSource {
    pub entry: &'static str,
    pub ordinary_dirs: Vec<PathBuf>,
    pub capture: CapturePolicy,
    pub specialization: SourceSpecialization,
}
pub struct Prepared {
    pub project: BootProject,
    pub constants: String,
    pub extra_assets: Vec<String>,
    pub source: ApplicationSource,
}
#[derive(Clone, Copy)]
pub struct Assets {
    pub script: &'static str,
    pub directory: &'static str,
    pub constants: &'static str,
    pub repack: &'static str,
    pub names: ResourceNames,
    pub inputs: &'static [&'static str],
    pub production_surface: bool,
    pub manifest_sources: bool,
}
#[derive(Clone, Copy)]
pub enum ResourceNames {
    File(&'static str),
    Fixed(&'static str),
}
#[derive(Clone, Copy)]
pub struct Definition {
    pub name: &'static str,
    pub selection: Option<&'static str>,
    pub source: Option<&'static str>,
    pub gpu_api: bool,
    pub entry: &'static str,
    pub capture: CapturePolicy,
    pub ordinary_sources: &'static [&'static str],
    pub owner: &'static str,
    pub assets: Option<Assets>,
    pub options: &'static [BuildOption],
    pub inputs: fn(&mut Context<'_>),
    pub configure: fn(&mut Context<'_>, &mut Prepared, &Value),
}
pub const DEFAULT: Definition = Definition {
    name: "default",
    selection: None,
    source: None,
    gpu_api: false,
    entry: "main",
    capture: CapturePolicy::Shared,
    ordinary_sources: &[],
    owner: "applications/build/mod.rs",
    assets: None,
    options: &[],
    inputs: |_| {},
    configure: |_, _, _| {},
};
#[derive(Clone, Copy)]
pub struct BuildOption {
    pub flag: &'static str,
    pub variable: &'static str,
    pub values: OptionValues,
}
#[derive(Clone, Copy)]
pub enum OptionValues {
    Choices(&'static [&'static str]),
    Integer { minimum: u16, maximum: u16 },
    InputFile,
}
pub const COMMON_OPTIONS: &[BuildOption] = &[
    BuildOption {
        flag: "hook",
        variable: "GPU_PROFILER_HOOK",
        values: OptionValues::Choices(&["0", "1"]),
    },
    BuildOption {
        flag: "gpu-debug",
        variable: "RCC_GPU_DEBUG",
        values: OptionValues::Choices(&["0", "1"]),
    },
    BuildOption {
        flag: "manifest",
        variable: "GPU_PROFILER_MANIFEST",
        values: OptionValues::InputFile,
    },
];
pub const RENDERER_ASSETS: Assets = Assets {
    script: "examples/gpu_renderer/tools/assets.py",
    directory: "gpu-renderer-assets",
    constants: "renderer_assets.rs",
    repack: "renderer-repack.conf",
    names: ResourceNames::File("GPU_RENDERER_"),
    inputs: &[],
    production_surface: false,
    manifest_sources: false,
};

pub fn definitions() -> impl Iterator<Item = &'static Definition> {
    std::iter::once(&DEFAULT).chain(
        [
            bench::APPLICATIONS,
            demo::APPLICATIONS,
            ds2::APPLICATIONS,
            ds2_ui::APPLICATIONS,
            renderer::APPLICATIONS,
            bloxorz::APPLICATIONS,
            rtt::APPLICATIONS,
            profiler::APPLICATIONS,
        ]
        .into_iter()
        .flatten(),
    )
}
pub fn find(name: &str) -> Result<&'static Definition, String> {
    definitions()
        .find(|d| d.name == name || d.selection == Some(name))
        .ok_or_else(|| format!("unknown application {name}; use cpu-v3-app list"))
}

/// Preparation happens only inside the authoritative application build script.
pub struct Context<'a> {
    pub root: &'a Path,
    pub output: &'a Path,
    pub watch: &'a mut dyn FnMut(&Path),
}
impl Context<'_> {
    pub fn inputs(&mut self, files: &[&str]) {
        for name in files {
            (self.watch)(&self.root.join(name));
        }
    }
    pub fn production_surface(&self, manifest: &Value) {
        assert_eq!(manifest["profile"], "production");
        assert_eq!(manifest["surface"], "column-major-16x16-pad16");
    }
    pub fn copy_asset(&mut self, application: &mut Prepared, source: &str, destination: &str) {
        self.inputs(&[source]);
        std::fs::copy(self.root.join(source), self.output.join(destination)).unwrap();
        application.extra_assets.push(destination.into());
    }
}

pub fn prepare(root: &Path, output: &Path, name: &str, watch: &mut dyn FnMut(&Path)) -> Prepared {
    let definition = find(name).unwrap_or_else(|error| panic!("{error}"));
    if definition
        .capture
        .requires_manifest(std::env::var("GPU_PROFILER_HOOK").ok().as_deref())
    {
        assert!(
            std::env::var_os("GPU_PROFILER_MANIFEST").is_some_and(|value| !value.is_empty()),
            "{} requires GPU_PROFILER_MANIFEST with the capture hook enabled",
            definition.name
        );
    }
    let mut context = Context {
        root,
        output,
        watch,
    };
    context.inputs(&["boot-applications.conf", "tools/gpu_layout.py"]);
    for recipe in ["applications/build/mod.rs", definition.owner] {
        let path = root.join(recipe);
        assert!(
            path.is_file(),
            "missing application recipe {}",
            path.display()
        );
        (context.watch)(&path);
    }
    (definition.inputs)(&mut context);
    let mut project =
        parse_boot_project(&std::fs::read_to_string(root.join("boot-applications.conf")).unwrap())
            .expect("single application project");
    if let Some(source) = definition.source {
        project.application = source.into();
        project.gpu_api = definition.gpu_api;
        project.copies.clear();
        project.zeros.clear();
    }
    let mut application = Prepared {
        project,
        constants: String::new(),
        extra_assets: vec![],
        source: ApplicationSource {
            entry: definition.entry,
            capture: definition.capture,
            ordinary_dirs: definition
                .ordinary_sources
                .iter()
                .map(|path| root.join(path))
                .chain(std::iter::once(root.join("applications/rendering/rcc")))
                .collect(),
            specialization: SourceSpecialization::Standard,
        },
    };
    let manifest = if let Some(assets) = definition.assets {
        context.inputs(assets.inputs);
        context.inputs(&[assets.script]);
        let directory = output.join(assets.directory);
        let status = std::process::Command::new("python")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .arg(root.join(assets.script))
            .arg("--output")
            .arg(&directory)
            .status()
            .expect("application asset producer");
        assert!(status.success(), "application asset validation failed");
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(directory.join("assets.json")).unwrap()).unwrap();
        if assets.manifest_sources {
            for source in manifest["sources"].as_object().unwrap().keys() {
                (context.watch)(&root.join("../..").join(source));
            }
        }
        if assets.production_surface {
            context.production_surface(&manifest);
        }
        application.project.copies = manifest["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|resource| {
                let name = resource["file"].as_str().unwrap();
                let symbol = match assets.names {
                    ResourceNames::File(prefix) => {
                        format!("{prefix}{}", name.trim_end_matches(".bin").to_uppercase())
                    }
                    ResourceNames::Fixed(symbol) => symbol.into(),
                };
                (
                    symbol,
                    resource["address"].as_u64().unwrap() as usize,
                    directory.join(name),
                )
            })
            .collect();
        application.project.zeros.clear();
        application.constants = std::fs::read_to_string(directory.join(assets.constants)).unwrap();
        for name in ["assets.json", assets.constants] {
            application
                .extra_assets
                .push(format!("{}/{name}", assets.directory));
        }
        manifest
    } else {
        Value::Null
    };
    (definition.configure)(&mut context, &mut application, &manifest);
    if let Some(assets) = definition.assets {
        let mut repack = format!(
            "format 2\n{}application {}\n",
            if application.project.gpu_api {
                "gpu-api\n"
            } else {
                ""
            },
            application.project.application.display()
        );
        for (name, address, path) in &application.project.copies {
            let relative = path
                .strip_prefix(output)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            repack.push_str(&format!("copy {name} {address:#x} {relative}\n"));
            application.extra_assets.push(relative);
        }
        for (address, length) in &application.project.zeros {
            repack.push_str(&format!("zero {address:#x} {length}\n"));
        }
        std::fs::write(output.join(assets.repack), repack).unwrap();
        application.extra_assets.push(assets.repack.into());
    }
    for (_, _, path) in &application.project.copies {
        if !path.starts_with(output) {
            (context.watch)(&root.join(path));
        }
    }
    application.constants = application.project.constants() + &application.constants;
    application
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_policy_keeps_disabled_and_uninstrumented_diagnostic_modes() {
        for setting in [None, Some("0"), Some("1")] {
            assert!(!CapturePolicy::Disabled.hook_enabled(setting));
            assert!(!CapturePolicy::Disabled.requires_manifest(setting));
            assert!(!CapturePolicy::Shared.requires_manifest(setting));
            assert_eq!(
                CapturePolicy::Shared.hook_enabled(setting),
                setting != Some("0")
            );
            assert_eq!(
                CapturePolicy::Diagnostic.requires_manifest(setting),
                setting != Some("0")
            );
        }
    }
    #[test]
    fn application_names_aliases_and_options_are_unambiguous() {
        let mut names = std::collections::BTreeMap::new();
        let system = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for definition in definitions() {
            assert!(system.join(definition.owner).is_file());
            if let Some(source) = definition.source {
                assert!(system.join(source).is_file());
            }
            for name in std::iter::once(definition.name).chain(definition.selection) {
                if let Some(previous) = names.insert(name, definition.name) {
                    assert_eq!(previous, definition.name, "ambiguous application {name}");
                }
                assert_eq!(find(name).unwrap().name, definition.name);
            }
            let mut flags = std::collections::BTreeSet::new();
            let mut variables = std::collections::BTreeSet::new();
            for option in COMMON_OPTIONS.iter().chain(definition.options) {
                assert!(flags.insert(option.flag));
                assert!(variables.insert(option.variable));
            }
        }
    }
}
