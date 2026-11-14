//! Snapshot/audit protocol fixtures only; these reports are not physical fit evidence.
use super::*;
use crate::resources::components::BsramBlocks;
use crate::TangNano20K;

fn fixture(label: &str) -> (PathBuf, GeneratedGowinProject) {
    let root = std::env::temp_dir().join(format!("artifact-{label}-{}", std::process::id()));
    fs::create_dir_all(root.join("impl/pnr")).unwrap();
    fs::create_dir_all(root.join("impl/gwsynthesis")).unwrap();
    let mut resources = TargetResources::<TangNano20K>::new();
    resources
        .claim_module(
            "u_bank.Wrapper".into(),
            &TargetResourceRequest::new(BsramBlocks::new(1)),
        )
        .unwrap();
    let project = GeneratedGowinProject {
        project_name: "fixture".into(),
        top_module: "top".into(),
        logic_top_module: "Fixture".into(),
        target_name: TangNano20K::NAME,
        device: TangNano20K::DEVICE,
        resources: resources.report(),
        dsp_expectations: BTreeMap::new(),
        bsram_expectation: Some(ResourceCountExpectation::Claimed),
        files: BTreeMap::from([(
            PathBuf::from("fixture.v"),
            "module Fixture(); endmodule\n".into(),
        )]),
    };
    project.write_to(&root).unwrap();
    fs::write(
        root.join("impl/pnr/fixture.fs"),
        "// protocol fixture only\n10101010\n",
    )
    .unwrap();
    fs::write(root.join("impl/pnr/fixture.tr"),"<Numbers of Paths Analyzed>:1\n<Numbers of Endpoints Analyzed>:1\n<Numbers of Setup Violated Endpoints>:0\n<Numbers of Hold Violated Endpoints>:0\n").unwrap();
    fs::write(
        root.join("impl/pnr/fixture.rpt.txt"),
        "3. Resource Usage Summary\n  BSRAM | 1/46 | 3%\n  DSP | 0/32 | 0%\n  PLL | 0/2 | 0%\n",
    )
    .unwrap();
    fs::write(root.join("impl/gwsynthesis/fixture_syn_rsc.xml"),"<Module name=\"top\">\n<SubModule name=\"u_logic\">\n<SubModule name=\"u_bank\" Bsram=\"1\"/>\n</SubModule>\n</Module>\n").unwrap();
    build_manifest::write(&root, &project, &root.join("impl/pnr/fixture.fs")).unwrap();
    seal_fit(&root, &project).unwrap();
    (root, project)
}

#[test]
fn snapshot_roundtrip_retains_sources_and_complete_owned_resource_plan() {
    let (root, project) = fixture("roundtrip");
    let saved = load::<TangNano20K>(&root).unwrap();
    assert_eq!(saved.files, project.files);
    assert_eq!(saved.resources, project.resources);
    assert_eq!(saved.bsram_expectation, project.bsram_expectation);
    let before = fs::read(root.join(FILE)).unwrap();
    let report = inspect_gowin_artifact::<TangNano20K>(&root).unwrap();
    assert_eq!(report["project"], "fixture");
    assert_eq!(before, fs::read(root.join(FILE)).unwrap());
}

#[test]
fn existing_audit_rejects_each_changed_source_bitstream_and_fit_report() {
    let (root, _) = fixture("mutations");
    for name in [
        "fixture.v",
        FILE,
        build_manifest::FILE_NAME,
        "impl/pnr/fixture.fs",
        "impl/pnr/fixture.tr",
        "impl/pnr/fixture.rpt.txt",
        "impl/gwsynthesis/fixture_syn_rsc.xml",
    ] {
        let path = root.join(name);
        let original = fs::read(&path).unwrap();
        let mut changed = original.clone();
        changed.push(b' ');
        fs::write(&path, changed).unwrap();
        assert!(
            inspect_gowin_artifact::<TangNano20K>(&root).is_err(),
            "{name}"
        );
        fs::write(path, original).unwrap();
    }
    assert!(inspect_gowin_artifact::<TangNano20K>(&root).is_ok());
}

#[test]
fn frozen_identity_does_not_replace_timing_and_resource_audits() {
    let (root, project) = fixture("audits");
    let path = root.join("impl/pnr/fixture.tr");
    let good = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        good.replace("Setup Violated Endpoints>:0", "Setup Violated Endpoints>:1"),
    )
    .unwrap();
    seal_fit(&root, &project).unwrap();
    assert!(inspect_gowin_artifact::<TangNano20K>(&root).is_err());
    fs::write(path, good).unwrap();
    let path = root.join("impl/pnr/fixture.rpt.txt");
    fs::write(
        &path,
        fs::read_to_string(&path).unwrap().replace("1/46", "2/46"),
    )
    .unwrap();
    seal_fit(&root, &project).unwrap();
    assert!(inspect_gowin_artifact::<TangNano20K>(&root).is_err());
}

#[test]
fn descriptor_rejects_wrong_target_paths_and_incomplete_fit_closure() {
    let (root, project) = fixture("negative");
    let mut value: Value = serde_json::from_slice(&fs::read(root.join(FILE)).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("artifact_id");
    value["target"] = json!("another-board");
    value["artifact_id"] = json!(hash(&serde_json::to_vec(&value).unwrap()));
    fs::write(root.join(FILE), serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(load::<TangNano20K>(&root).is_err());
    assert!(source_path(&root, "../outside").is_err());
    let absolute = format!("{}:/outside", 'C');
    assert!(source_path(&root, &absolute).is_err());
    assert!(source_path(&root, "fixture.v:stream").is_err());
    write(&root, &project).unwrap();
    let mut seal: Value =
        serde_json::from_slice(&fs::read(root.join("gowin-fit.json")).unwrap()).unwrap();
    seal.as_object_mut().unwrap().remove("fit_id");
    seal["files"]
        .as_object_mut()
        .unwrap()
        .remove("impl/pnr/fixture.tr");
    seal["fit_id"] = json!(hash(&serde_json::to_vec(&seal).unwrap()));
    fs::write(
        root.join("gowin-fit.json"),
        serde_json::to_vec(&seal).unwrap(),
    )
    .unwrap();
    assert!(verify_fit(&root, &project).is_err());
}

#[test]
fn diagnostic_fit_binds_raw_logs_and_mapped_warning_evidence() {
    let (root, project) = fixture("diagnostic-fit");
    for name in warning_files("fixture") {
        fs::write(root.join(name), "diagnostic fixture\n").unwrap();
    }
    seal_fit(&root, &project).unwrap();
    let seal: Value =
        serde_json::from_slice(&fs::read(root.join("gowin-fit.json")).unwrap()).unwrap();
    assert_eq!(seal["version"], 2);
    verify_fit(&root, &project).unwrap();
    for name in warning_files("fixture") {
        let path = root.join(name);
        fs::write(&path, "changed diagnostic\n").unwrap();
        assert!(verify_fit(&root, &project).is_err());
        fs::write(&path, "diagnostic fixture\n").unwrap();
    }
}
