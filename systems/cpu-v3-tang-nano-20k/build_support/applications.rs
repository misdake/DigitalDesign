//! Compile one application from its application-owned preparation recipe.
use crate::{artifact, boot};
use cpu_v3::rcc_backend::CompilerOptions;
use cpu_v3_tools::applications::ApplicationSource;
use std::path::Path;

#[path = "application_compile.rs"]
mod compile;
use compile::compile_program;

enum ApplicationFrontend {
    Native,
    ExplicitGpu,
}
pub struct Application {
    pub project: boot::BootProject,
    pub constants: String,
    pub extra_assets: Vec<String>,
    source: ApplicationSource,
    pub inputs: artifact::Inputs,
}

pub fn prepare(root: &Path, output: &Path) -> Application {
    let mut inputs = artifact::Inputs::new(&root.join("../.."), output);
    println!("cargo:rerun-if-env-changed=CPU_V3_BOOT_EXAMPLE");
    let name = match std::env::var("CPU_V3_BOOT_EXAMPLE") {
        Ok(name) => name,
        Err(std::env::VarError::NotPresent) => "default".into(),
        Err(error) => panic!("invalid CPU_V3_BOOT_EXAMPLE: {error}"),
    };
    let prepared =
        cpu_v3_tools::applications::prepare(root, output, &name, &mut |path| inputs.watch(path));
    Application {
        project: prepared.project,
        constants: prepared.constants,
        extra_assets: prepared.extra_assets,
        source: prepared.source,
        inputs,
    }
}

impl Application {
    pub fn description(&self, root: &Path) -> serde_json::Value {
        serde_json::json!({
            "source":self.inputs.describe_path(&root.join(&self.project.application)),
            "entry":self.source.entry,
            "frontend":if self.project.gpu_api { "explicit-gpu" } else { "native" },
            "ordinary_sources":self.source.ordinary_dirs.iter().map(|p|self.inputs.describe_path(p)).collect::<Vec<_>>()
        })
    }

    pub fn compile(
        &mut self,
        root: &Path,
        _output: &Path,
        options: &CompilerOptions,
        reserved: &[boot::BootResource],
    ) -> Result<compile::CompiledApplication, cpu_v3_tools::diagnostics::BuildDiagnostic> {
        let result = compile_program(
            compile::CompileRequest {
                path: &root.join(&self.project.application),
                constants: &self.constants,
                shared: &root.join("rcc"),
                frontend: if self.project.gpu_api {
                    ApplicationFrontend::ExplicitGpu
                } else {
                    ApplicationFrontend::Native
                },
                policy: &self.source,
                reserved,
            },
            options,
            &mut self.inputs,
        )?;
        Ok(result)
    }
}
