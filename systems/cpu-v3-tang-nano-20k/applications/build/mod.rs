//! Application-owned recipes shared by the build script and host command.
use cpu_v3_boot_format::{parse_boot_project, BootProject};
use serde_json::Value;
use std::path::{Path, PathBuf};
pub mod asset_metadata;

// One explicit registration supplies both module loading and enumeration.
macro_rules! application_catalog {
    ($($name:ident => $path:literal),+ $(,)?) => {
        $(#[path = $path] mod $name;)+
        pub fn definitions() -> impl Iterator<Item = &'static Definition> {
            std::iter::once(&DEFAULT).chain([$($name::APPLICATIONS),+].into_iter().flatten())
        }
    };
}
application_catalog! {
    bench => "../../examples/gpu_bench/application.rs",
    core_demo => "../../examples/core_demo/application.rs",
    demo => "../../examples/gpu_demo/application.rs",
    ds2 => "../../examples/dualshock2/application.rs",
    ds2_ui => "../../examples/dualshock2_ui/application.rs",
    renderer => "../../examples/gpu_renderer/application.rs",
    bloxorz => "../../examples/gpu_renderer_bloxorz/application.rs",
    rtt => "../../examples/gpu_renderer_rtt/application.rs",
}

pub struct ApplicationSource {
    pub sector_relocations: Vec<asset_metadata::SectorRelocation>,
    pub entry: &'static str,
    pub ordinary_dirs: Vec<PathBuf>,
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
    ordinary_sources: &[],
    owner: "applications/build/mod.rs",
    assets: None,
    options: &[],
    inputs: |_| {},
    configure: |_, _, _| {},
};
mod options;
pub use options::{BuildOption, OptionValues, COMMON_OPTIONS};
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
    let mut option_constants = String::new();
    for option in COMMON_OPTIONS.iter().chain(definition.options) {
        println!("cargo:rerun-if-env-changed={}", option.variable);
        let value = std::env::var(option.variable).ok();
        option_constants.push_str(
            &option
                .constant_source(value.as_deref())
                .unwrap_or_else(|error| panic!("{error}")),
        );
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
        constants: option_constants,
        extra_assets: vec![],
        source: ApplicationSource {
            sector_relocations: vec![],
            entry: definition.entry,
            ordinary_dirs: definition
                .ordinary_sources
                .iter()
                .map(|path| root.join(path))
                .chain(std::iter::once(root.join("applications/rendering/rcc")))
                .collect(),
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
            .filter(|resource| resource["placement"] != "source")
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
        application.constants +=
            &std::fs::read_to_string(directory.join(assets.constants)).unwrap();
        for name in ["assets.json", assets.constants] {
            application
                .extra_assets
                .push(format!("{}/{name}", assets.directory));
        }
        for resource in manifest["resources"].as_array().unwrap() {
            if resource["placement"] == "source" {
                application.extra_assets.push(format!(
                    "{}/{}",
                    assets.directory,
                    resource["file"].as_str().unwrap()
                ));
            }
        }
        manifest
    } else {
        Value::Null
    };
    application.source.sector_relocations =
        asset_metadata::sector_relocations(&manifest).expect("producer sector relocation metadata");
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
