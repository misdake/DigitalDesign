use super::{forward_slashes, GeneratedGowinProject, GowinError};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) const FILE_NAME: &str = "gowin-build.manifest";

fn fnv1a64_update(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
    }
    hash
}

fn file_fingerprint(path: &Path) -> Result<(u64, u64), GowinError> {
    let bytes = fs::read(path)?;
    Ok((
        bytes.len() as u64,
        fnv1a64_update(0xcbf2_9ce4_8422_2325, &bytes),
    ))
}

pub(super) fn source_fingerprint(project: &GeneratedGowinProject) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for (path, source) in &project.files {
        let path = forward_slashes(path);
        for bytes in [path.as_bytes(), source.as_bytes()] {
            hash = fnv1a64_update(hash, &(bytes.len() as u64).to_le_bytes());
            hash = fnv1a64_update(hash, bytes);
        }
    }
    hash
}

fn manifest_value<'a>(values: &'a BTreeMap<&str, &str>, name: &str) -> Result<&'a str, GowinError> {
    values
        .get(name)
        .copied()
        .ok_or_else(|| GowinError::InvalidBuildManifest(format!("missing `{name}` field")))
}

fn parse_u64(value: &str, name: &str) -> Result<u64, GowinError> {
    value.parse().map_err(|_| {
        GowinError::InvalidBuildManifest(format!("invalid decimal `{name}` value `{value}`"))
    })
}

pub(super) fn render(
    project: &GeneratedGowinProject,
    bitstream_relative: &Path,
    bitstream_bytes: u64,
    bitstream_fingerprint: u64,
) -> String {
    format!(
        "format=1\n\
target={}\n\
project={}\n\
top={}\n\
logic_top={}\n\
part_number={}\n\
device_name={}\n\
device_version={}\n\
project_device_id={}\n\
programmer_device={}\n\
programmer_cable={}\n\
source_fingerprint={:016x}\n\
bitstream_path={}\n\
bitstream_bytes={}\n\
bitstream_fingerprint={:016x}\n",
        project.target_name,
        project.project_name,
        project.top_module,
        project.logic_top_module,
        project.device.part_number,
        project.device.device_name,
        project.device.device_version,
        project.device.project_device_id,
        project.device.programmer_device,
        project.device.programmer_cable.index(),
        source_fingerprint(project),
        forward_slashes(bitstream_relative),
        bitstream_bytes,
        bitstream_fingerprint,
    )
}

pub(super) fn parse(text: &str) -> Result<BTreeMap<&str, &str>, GowinError> {
    let mut values = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let (name, value) = line.split_once('=').ok_or_else(|| {
            GowinError::InvalidBuildManifest(format!(
                "line {} is not a name=value field",
                index + 1
            ))
        })?;
        if name.is_empty() || value.is_empty() || values.insert(name, value).is_some() {
            return Err(GowinError::InvalidBuildManifest(format!(
                "invalid or duplicate field on line {}",
                index + 1
            )));
        }
    }
    Ok(values)
}

pub(super) fn write(
    directory: &Path,
    project: &GeneratedGowinProject,
    bitstream: &Path,
) -> Result<(), GowinError> {
    let bitstream_relative = PathBuf::from(format!("impl/pnr/{}.fs", project.project_name));
    let (bitstream_bytes, bitstream_fingerprint) = file_fingerprint(bitstream)?;
    fs::write(
        directory.join(FILE_NAME),
        render(
            project,
            &bitstream_relative,
            bitstream_bytes,
            bitstream_fingerprint,
        ),
    )?;
    Ok(())
}

pub(super) fn validate(
    directory: &Path,
    project: &GeneratedGowinProject,
) -> Result<PathBuf, GowinError> {
    let manifest_path = directory.join(FILE_NAME);
    let text = fs::read_to_string(&manifest_path).map_err(|error| {
        GowinError::InvalidBuildManifest(format!(
            "cannot read {}: {error}",
            manifest_path.display()
        ))
    })?;
    let values = parse(&text)?;
    let programmer_cable = project.device.programmer_cable.index().to_string();
    let expected = [
        ("format", "1"),
        ("target", project.target_name),
        ("project", project.project_name.as_str()),
        ("top", project.top_module.as_str()),
        ("logic_top", project.logic_top_module.as_str()),
        ("part_number", project.device.part_number),
        ("device_name", project.device.device_name),
        ("device_version", project.device.device_version),
        ("project_device_id", project.device.project_device_id),
        ("programmer_device", project.device.programmer_device),
        ("programmer_cable", programmer_cable.as_str()),
    ];
    for (name, expected) in expected {
        let actual = manifest_value(&values, name)?;
        if actual != expected {
            return Err(GowinError::InvalidBuildManifest(format!(
                "`{name}` is `{actual}`, expected `{expected}`"
            )));
        }
    }

    let actual_source = manifest_value(&values, "source_fingerprint")?;
    let expected_source = format!("{:016x}", source_fingerprint(project));
    if actual_source != expected_source {
        return Err(GowinError::InvalidBuildManifest(format!(
            "generated sources changed (manifest {actual_source}, current {expected_source}); rebuild before programming"
        )));
    }

    let expected_relative = PathBuf::from(format!("impl/pnr/{}.fs", project.project_name));
    let bitstream_relative = PathBuf::from(manifest_value(&values, "bitstream_path")?);
    if bitstream_relative != expected_relative {
        return Err(GowinError::InvalidBuildManifest(format!(
            "bitstream path is `{}`, expected `{}`",
            bitstream_relative.display(),
            expected_relative.display()
        )));
    }
    let bitstream = directory.join(&bitstream_relative);
    let (actual_bytes, actual_fingerprint) = file_fingerprint(&bitstream).map_err(|error| {
        GowinError::InvalidBuildManifest(format!(
            "cannot validate bitstream {}: {error}",
            bitstream.display()
        ))
    })?;
    let expected_bytes = parse_u64(
        manifest_value(&values, "bitstream_bytes")?,
        "bitstream_bytes",
    )?;
    let expected_fingerprint = manifest_value(&values, "bitstream_fingerprint")?;
    if actual_bytes != expected_bytes
        || format!("{actual_fingerprint:016x}") != expected_fingerprint
    {
        return Err(GowinError::InvalidBuildManifest(
            "bitstream bytes changed; rebuild before programming".to_string(),
        ));
    }
    Ok(bitstream)
}
