use crate::frontend::{
    compile_program_named, compile_program_named_with_libraries, ModuleSource, Program,
};
use crate::RccOptions;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

const OWNER: &str = r#"#![rcc_resource_library(helper)]
mod helper;
#[rcc_resource(cleanup=release)] pub struct Handle{word:u16}
static ROOT_STATE:u16=3;
fn release(h:Handle){}
pub fn open()->Handle{Handle{word:1}}
pub fn close(h:Handle){resource_disarm(h);}
pub fn facade()->u16{raw_set(11);helper_value()}
impl Handle {
 pub fn finish(self){raw_set(12);resource_disarm(self);}
 pub fn read(&mut self)->u16{helper_value()}
 fn private(&mut self)->u16{self.word}
}
"#;
const HELPER: &str = r#"static STATE:u16=7;
fn helper_value()->u16{STATE+ROOT_STATE}
pub fn raw_read()->u16{STATE}
pub fn raw_set(value:u16){unsafe{addr_of(&ROOT_STATE).write(0,value);}}
pub const RAW_CONSTANT:u16=4;
"#;
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    files: HashMap<String, PathBuf>,
    origins: HashMap<String, Option<PathBuf>>,
    approved: Vec<PathBuf>,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/rcc-resource-libraries")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&root).unwrap();
        let mut f = Self {
            root,
            files: HashMap::new(),
            origins: HashMap::new(),
            approved: vec![],
        };
        f.write("owner", "lib/owner.rs", OWNER);
        f.write("helper", "lib/helper.rs", HELPER);
        f.approved.push(f.files["owner"].clone());
        f
    }
    fn write(&mut self, name: &str, path: &str, source: &str) {
        let path = self.root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        self.files.insert(name.into(), path.clone());
        self.origins.insert(name.into(), Some(path));
    }
    fn load(&self, name: &str) -> Result<ModuleSource, String> {
        if name == "generated" {
            return Ok(ModuleSource {
                source: "pub const GENERATED:u16=5;".into(),
                origin: None,
            });
        }
        let path = self
            .files
            .get(name)
            .ok_or_else(|| format!("missing module {name}"))?;
        Ok(ModuleSource {
            source: std::fs::read_to_string(path).map_err(|e| e.to_string())?,
            origin: self.origins[name].clone(),
        })
    }
    fn compile(&self, source: &str) -> Result<Program, crate::frontend::CompileError> {
        compile_program_named_with_libraries(
            "application",
            source,
            &RccOptions::default(),
            &self.approved,
            &mut |n| self.load(n),
        )
    }
    fn reject(&self, source: &str, message: &str) {
        let e = match self.compile(source) {
            Ok(_) => panic!("accepted {source}"),
            Err(e) => e.to_string(),
        };
        assert!(e.contains(message), "expected {message}: {e}");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let root = std::fs::canonicalize(&self.root).unwrap();
        let base = std::fs::canonicalize(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/rcc-resource-libraries"),
        )
        .unwrap();
        assert_eq!(root.parent(), Some(base.as_path()));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn resource_library_facade_and_private_cross_helper_access_lower() {
    let mut f = Fixture::new();
    f.write(
        "second_helper",
        "lib/second_helper.rs",
        "fn second_value()->u16{helper_value()}",
    );
    f.write(
        "owner",
        "lib/owner.rs",
        &OWNER
            .replace("(helper)", "(helper,second_helper)")
            .replace("mod helper;", "mod helper;mod second_helper;")
            .replace("raw_set(11);helper_value()", "raw_set(11);second_value()"),
    );
    let p = f.compile("mod owner;mod generated;fn main(){let h=open();let n=facade()+GENERATED;close(h);halt(n);}").unwrap();
    assert!(p
        .funcs
        .iter()
        .find(|f| f.name == "facade")
        .unwrap()
        .blocks
        .iter()
        .any(|b| b
            .insts
            .iter()
            .any(|i| matches!(i,crate::Instr::Call{func,..} if *func=="second_value"))));
}

#[test]
fn resource_library_hides_private_state_and_even_public_raw_helpers() {
    let mut f = Fixture::new();
    f.write(
        "outsider",
        "lib/outsider.rs",
        "pub fn forwarded()->u16{raw_read()}",
    );
    f.write(
        "legacy",
        "lib/legacy.rs",
        "mod helper;pub fn legacy_read()->u16{raw_read()}",
    );
    for body in [
        "halt(raw_read());",
        "raw_set(9);",
        "halt(STATE);",
        "halt(ROOT_STATE);",
        "halt(RAW_CONSTANT);",
        "let f=raw_read;halt(f());",
        "unsafe{addr_of(&STATE).write(0,9);}",
        "use helper::raw_read;halt(raw_read());",
    ] {
        for modules in ["mod owner;mod helper;", "mod helper;mod owner;"] {
            f.reject(&format!("{modules}fn main(){{{body}}}"), "resource-library");
        }
    }
    for source in [
        "mod helper;mod owner;const LEAK:u16=RAW_CONSTANT;fn main(){}",
        "mod owner;mod helper;static LEAK:u16=STATE;fn main(){}",
        "mod helper;mod owner;struct Leak{data:Buf<u16,RAW_CONSTANT>}fn main(){}",
    ] {
        f.reject(source, "resource-library");
    }
    f.reject(
        "mod owner;mod outsider;fn main(){halt(forwarded());}",
        "resource-library",
    );
    f.reject(
        "mod owner;mod legacy;fn main(){halt(legacy_read());}",
        "resource-library",
    );
    f.reject(
        "mod owner;use helper::raw_read as renamed;fn main(){halt(renamed());}",
        "undefined function",
    );
    f.reject("mod owner;#[rcc_resource(cleanup=drop_fake)] struct Fake{word:u16}fn drop_fake(f:Fake){}fn main(){halt(raw_read());}", "resource-library");
    let native = "mod legacy;static ROOT_STATE:u16=3;fn main(){halt(legacy_read());}";
    compile_program_named(
        "native diagnostic",
        native,
        &RccOptions::default(),
        &mut |n| f.load(n).map(|m| m.source),
    )
    .unwrap();
}

#[test]
fn resource_library_approval_cannot_be_disabled_by_omitting_root_import() {
    let mut f = Fixture::new();
    f.write(
        "forwarder",
        "lib/forwarder.rs",
        "mod helper;pub fn forward()->u16{raw_read()}",
    );
    for source in [
        "mod helper;fn main(){halt(raw_read());}",
        "mod helper;fn main(){halt(STATE);}",
        "mod helper;fn main(){unsafe{addr_of(&STATE).write(0,99);}}",
        "mod helper;fn main(){raw_set(99);}",
        "mod forwarder;fn main(){halt(forward());}",
        "mod helper;#[rcc_resource(cleanup=drop_fake)]struct Fake{word:u16}fn drop_fake(f:Fake){}fn main(){halt(raw_read());}",
        "fn main(){}",
    ] {
        f.reject(source, "every host-approved resource-library root");
    }
    f.write("owner", "lib/owner.rs", "pub fn facade()->u16{0}");
    f.reject(
        "mod owner;mod helper;fn main(){}",
        "every host-approved resource-library root",
    );
}

#[test]
fn resource_library_cross_kind_global_names_cannot_override_access_decisions() {
    let declarations = [
        "static COLLISION:u16=37;",
        "const COLLISION:u16=37;",
        "fn COLLISION()->u16{37}",
    ];
    for protected in declarations {
        for outside in declarations {
            let mut f = Fixture::new();
            f.write(
                "helper",
                "lib/helper.rs",
                &format!("{protected}fn helper_value()->u16{{0}}pub fn raw_set(value:u16){{}}"),
            );
            f.write(
                "outsider",
                "application/outsider.rs",
                &format!("pub {outside}"),
            );
            for modules in [
                "mod helper;mod owner;mod outsider;",
                "mod outsider;mod owner;mod helper;",
            ] {
                f.reject(
                    &format!("{modules}fn main(){{}}"),
                    "resource-domain value name",
                );
            }
        }
    }
    let mut f = Fixture::new();
    f.write(
        "outsider",
        "application/outsider.rs",
        "pub const STATE:u16=0;",
    );
    for modules in [
        "mod helper;mod owner;mod outsider;",
        "mod outsider;mod owner;mod helper;",
    ] {
        f.reject(
            &format!("{modules}fn main(){{unsafe{{addr_of(&STATE).write(0,99);}}}}"),
            "resource-domain value name",
        );
    }
    f.compile("mod owner;fn local(STATE:u16)->u16{STATE}fn main(){let ROOT_STATE=2;let raw_read=3;let RAW_CONSTANT=4;halt(local(ROOT_STATE+raw_read+RAW_CONSTANT));}").unwrap();
}

#[test]
fn resource_library_requires_approved_actual_origins() {
    let mut f = Fixture::new();
    let main = "mod owner;fn main(){let mut h=open();let n=h.read();h.finish();halt(n);}";
    assert!(compile_program_named(
        "old string loader",
        main,
        &RccOptions::default(),
        &mut |n| f.load(n).map(|m| m.source)
    )
    .is_err());
    f.approved.clear();
    f.reject(main, "not host-approved");
    f.approved.push(f.files["owner"].clone());
    f.origins.insert("helper".into(), None);
    f.reject(main, "no host source origin");
    f.origins
        .insert("helper".into(), Some(PathBuf::from("helper.rs")));
    f.reject(main, "absolute paths");
    f.write("helper", "application/helper.rs", HELPER);
    f.reject(main, "does not match");
    f.write("helper", "lib/helper.rs", HELPER);
    f.origins
        .insert("owner".into(), Some(PathBuf::from("owner.rs")));
    f.reject(main, "absolute paths");
    f.origins.insert("owner".into(), None);
    f.reject(main, "no approved host origin");
}

#[test]
fn resource_library_methods_keep_owner_permissions_and_internal_namespace() {
    let mut f = Fixture::new();
    f.compile("mod owner;fn main(){let mut h=open();let n=h.read();h.finish();halt(n);}")
        .unwrap();
    f.reject(
        "mod owner;fn main(){let mut h=open();halt(h.private());}",
        "not callable",
    );
    for code in [
        "impl Handle{pub fn steal(self){resource_disarm(self);}}",
        "impl Handle{pub fn steal(&mut self)->u16{self.word}}",
    ] {
        // Membership in the approved library still cannot relocate an impl or
        // acquire the definition-file identity-operation privileges.
        f.write("helper", "lib/helper.rs", &format!("{HELPER}\n{code}"));
        f.reject(
            "mod owner;fn main(){}",
            "impl must be in its definition file",
        );
    }
    f.write("helper", "lib/helper.rs", HELPER);
    f.reject(
        "mod owner;impl Handle{pub fn steal(self){}}fn main(){}",
        "impl must be in its definition file",
    );
    f.reject(
        "mod owner;fn main(){let h=open();__rcc_method_0(h);}",
        "reserved resource method name",
    );
    f.reject(
        "mod owner;fn main(){let p=__rcc_method_0;}",
        "reserved resource method name",
    );
}

#[test]
fn resource_library_keeps_resource_identity_operations_definition_local() {
    for (code, diagnostic) in [
        ("fn forge()->Handle{Handle{word:1}}", "private"),
        ("fn inspect(h:&mut Handle)->u16{h.word}", "private"),
        (
            "fn forget(h:Handle){resource_disarm(h);}",
            "definition file",
        ),
    ] {
        let mut f = Fixture::new();
        f.write("helper", "lib/helper.rs", &format!("{HELPER}\n{code}"));
        f.reject("mod owner;fn main(){}", diagnostic);
    }
}

#[test]
fn resource_library_rejects_implicit_members_and_duplicate_claims() {
    let mut f = Fixture::new();
    f.write("extra", "lib/extra.rs", "pub fn unused()->u16{0}");
    f.write(
        "owner",
        "lib/owner.rs",
        &OWNER.replace("(helper)", "(helper,extra)"),
    );
    f.reject("mod owner;mod extra;fn main(){}", "direct mod");
    f.write(
        "owner",
        "lib/owner.rs",
        &OWNER.replace("(helper)", "(helper,helper)"),
    );
    f.reject("mod owner;fn main(){}", "duplicate");
    f.write("owner", "lib/owner.rs", OWNER);
    let second = "#![rcc_resource_library(helper)] mod helper;#[rcc_resource(cleanup=release_second)] struct Second{word:u16}fn release_second(s:Second){}";
    f.write("second", "lib/second.rs", second);
    f.approved.push(f.files["second"].clone());
    f.reject("mod owner;mod second;fn main(){}", "already claimed");
    f.write(
        "owner",
        "lib/owner.rs",
        &OWNER
            .replace("(helper)", "(second)")
            .replace("mod helper;", "mod helper;mod second;"),
    );
    f.reject("mod owner;fn main(){}", "resource root cannot");
}

#[test]
fn resource_library_marker_cannot_authorize_application_or_synthetic_root() {
    let f = Fixture::new();
    f.reject("#![rcc_resource_library(helper)] mod helper;#[rcc_resource(cleanup=drop_app)] struct App{word:u16}fn drop_app(a:App){}fn main(){}", "no approved host origin");
    f.reject(
        "mod owner;#[rcc_resource_library(helper)] struct Wrong{word:u16}fn main(){}",
        "file inner attribute",
    );
}

#[cfg(windows)]
#[test]
fn resource_library_windows_origin_keys_normalize_case_and_unc_prefix() {
    use super::library::key;
    let path = key(&std::env::temp_dir().join("RCC").join("owner.rs"));
    let extended = if let Some(tail) = path.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{tail}")
    } else {
        format!(r"\\?\{path}")
    };
    assert_eq!(key(std::path::Path::new(&extended.to_uppercase())), path);
    assert_eq!(
        key(std::path::Path::new(r"\\?\UNC\server\share\RCC\owner.rs")),
        key(std::path::Path::new(r"\\SERVER\SHARE\rcc\OWNER.RS"))
    );
}
