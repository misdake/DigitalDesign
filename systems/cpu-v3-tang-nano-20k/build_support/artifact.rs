//! Exact inputs and host metadata for a single linked application build.
use rcc::frontend::ModuleSource;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Stabilize presentation order without changing linked locations or file IDs.
pub fn debug_text(program: &cpu_v3::rcc_backend::CpuV3Program) -> String {
    let mut debug = program.debug.clone();
    debug.globals.sort_by(|a, b| a.name.cmp(&b.name));
    debug.consts.sort();
    debug.types.sort_by(|a, b| a.name.cmp(&b.name));
    for function in &mut debug.functions {
        function
            .locals
            .sort_by(|a, b| (&a.name, &a.ty, a.scope).cmp(&(&b.name, &b.ty, b.scope)));
    }
    debug.render()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModuleInput {
    content: String,
    origin: Option<PathBuf>,
}

pub struct Inputs {
    root: PathBuf,
    output: PathBuf,
    watched: BTreeSet<PathBuf>,
    files: BTreeMap<PathBuf, String>,
    modules: BTreeMap<String, ModuleInput>,
    contents: BTreeMap<String, Vec<u8>>,
}
impl Inputs {
    pub fn new(root: &Path, output: &Path) -> Self {
        Self {
            root: root.canonicalize().unwrap(),
            output: output.canonicalize().unwrap(),
            watched: BTreeSet::new(),
            files: BTreeMap::new(),
            modules: BTreeMap::new(),
            contents: BTreeMap::new(),
        }
    }
    fn origin(&self, path: &Path) -> Value {
        for (kind, root) in [("generated", &self.output), ("repository", &self.root)] {
            if let Ok(relative) = path.strip_prefix(root) {
                return json!({"kind":kind,"path":relative.to_string_lossy().replace('\\',"/")});
            }
        }
        json!({"kind":"external","path":path.to_string_lossy()})
    }
    fn remember(&mut self, bytes: &[u8]) -> String {
        let id = hash(bytes);
        self.contents
            .entry(id.clone())
            .or_insert_with(|| bytes.to_vec());
        id
    }
    pub fn watch(&mut self, path: &Path) {
        // A missing fallback candidate must not make Cargo perpetually dirty.
        let mut existing = path;
        while !existing.exists() {
            existing = existing.parent().expect("input has no existing parent");
        }
        let canonical = existing.canonicalize().unwrap();
        if !self.watched.insert(canonical.clone()) {
            return;
        }
        println!("cargo:rerun-if-changed={}", existing.display());
        if canonical.is_file() {
            let bytes = std::fs::read(&canonical).unwrap();
            let id = self.remember(&bytes);
            self.files.insert(canonical, id);
        }
    }
    pub fn module(&mut self, name: &str, source: &ModuleSource) {
        let name = if source.namespace == rcc::frontend::ModuleNamespace::Flat {
            name.rsplit('/').next().unwrap_or(name)
        } else {
            name
        };
        if let Some(path) = &source.origin {
            self.watch(path);
            let origin = &self.files[&path.canonicalize().unwrap()];
            assert_eq!(
                *origin,
                hash(source.source.as_bytes()),
                "module text does not match origin: {name}"
            );
        }
        self.entry(name, source);
    }
    /// The selected entry retains its original file and the exact transformed text.
    pub fn entry(&mut self, name: &str, source: &ModuleSource) {
        let origin = source.origin.as_ref().map(|path| {
            self.watch(path);
            path.canonicalize().unwrap()
        });
        let id = self.remember(source.source.as_bytes());
        let record = ModuleInput {
            content: id,
            origin,
        };
        if let Some(previous) = self.modules.insert(name.to_owned(), record.clone()) {
            assert_eq!(
                previous, record,
                "module changed during compilation: {name}"
            );
        }
    }
    pub fn embedded_standard_library(&mut self) {
        for (name, source) in rcc::frontend::std_sources() {
            self.module(
                name,
                &ModuleSource {
                    namespace: rcc::frontend::ModuleNamespace::Scoped,
                    source: (*source).to_owned(),
                    origin: None,
                },
            );
        }
    }
    pub fn describe_path(&self, path: &Path) -> Value {
        self.origin(&path.canonicalize().unwrap())
    }
}

pub struct BuildRecord<'a> {
    pub output: &'a Path,
    pub inputs: &'a Inputs,
    pub program: &'a cpu_v3::rcc_backend::CpuV3Program,
    pub static_words: usize,
    pub application: Value,
    pub resources: &'a [cpu_v3_boot_format::BootResource],
    pub options: &'a cpu_v3::rcc_backend::CompilerOptions,
}

/// Existing exported bytes are registered first, so snapshots reuse them by hash.
pub fn write(record: BuildRecord<'_>, assets: &mut Vec<String>) {
    let BuildRecord {
        output,
        inputs,
        program,
        static_words,
        application,
        resources,
        options,
    } = record;
    let mut files = BTreeMap::new();
    let mut content_files = BTreeMap::new();
    for name in assets.iter() {
        let bytes = std::fs::read(output.join(name)).unwrap();
        let id = hash(&bytes);
        assert!(
            files
                .insert(name.clone(), json!({"bytes":bytes.len(),"sha256":id}))
                .is_none(),
            "duplicate export: {name}"
        );
        content_files.entry(id).or_insert_with(|| name.clone());
    }
    let mut snapshot = |bytes: &[u8]| {
        let id = hash(bytes);
        content_files
            .entry(id.clone())
            .or_insert_with(|| {
                let name = format!("application.inputs/{id}");
                std::fs::create_dir_all(output.join("application.inputs")).unwrap();
                std::fs::write(output.join(&name), bytes).unwrap();
                files.insert(name.clone(), json!({"bytes":bytes.len(),"sha256":id}));
                assets.push(name.clone());
                name
            })
            .clone()
    };
    for bytes in inputs.contents.values() {
        snapshot(bytes);
    }
    let resources = resources.iter().map(|resource| {
        if resource.zero_bytes != 0 {
            json!({"kind":"zero","address_bytes":resource.address,"bytes":resource.zero_bytes})
        } else {
            json!({"kind":"copy","address_bytes":resource.address,"bytes":resource.data.len(),"file":snapshot(&resource.data)})
        }
    }).collect::<Vec<_>>();
    let mut source_map = BTreeMap::new();
    for name in &program.debug.files {
        let module = inputs
            .modules
            .get(name)
            .unwrap_or_else(|| panic!("unrecorded compiler source: {name}"));
        let origin = module
            .origin
            .as_ref()
            .map(|path| json!({"location":inputs.origin(path),"input_sha256":inputs.files[path]}));
        source_map.insert(
            name.clone(),
            json!({"sha256":module.content,"file":content_files[&module.content],"origin":origin}),
        );
    }
    let declared = inputs
        .files
        .iter()
        .map(|(path, id)| {
            assert_eq!(
                hash(&std::fs::read(path).unwrap()),
                *id,
                "build input changed: {}",
                path.display()
            );
            json!({"origin":inputs.origin(path),"file":content_files[id]})
        })
        .collect::<Vec<_>>();
    let mut manifest = json!({
        "schema":"digitaldesign.application-build","version":1,"application":application,
        "target":{"isa":"cpu-v3","code_segment":cpu_v3_boot_format::SINGLE_BOOT_CODE_WORD >> 16,"data_segments":[0,1,2,3],
            "code_base_words":program.code_base,"static_words":static_words,"stack_top":options.stack_init,
            "heap_begin":options.heap_begin,"heap_words":options.heap_size,"gpu_debug":program.gpu_debug},
        "compiler_options":format!("{options:?}"),
        "code":"application.v3bin","package":"cpu-v3-boot.bin","debug":"application.dbg","listing":"application.asm",
        "files":files,"source_map":source_map,"declared_inputs":declared,"resources":resources
    });
    if !program.code_segments.is_empty() {
        manifest["code_segments"]=json!(program.code_segments.iter().map(|s|json!({
            "cseg":s.cseg,"code_base_words":s.code_base,"physical_bytes":(usize::from(s.cseg)<<17)+usize::from(s.code_base)*2,
            "bytes":s.words.len()*2,"code":format!("application.cseg{}.v3bin",s.cseg),
            "debug":format!("application.cseg{}.dbg",s.cseg),"listing":format!("application.cseg{}.asm",s.cseg),
        })).collect::<Vec<_>>());
    }
    manifest["artifact_id"] = json!(hash(&serde_json::to_vec(&manifest).unwrap()));
    std::fs::write(
        output.join("application.build.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    assets.push("application.build.json".into());
}
