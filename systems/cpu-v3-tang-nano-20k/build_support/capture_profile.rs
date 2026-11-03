//! Host-owned capture profile. The wire ABI remains in gpu_capture_abi.rs.
use super::CaptureConfiguration;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
#[path = "../rcc/gpu_capture_abi.rs"]
#[allow(dead_code)]
pub(crate) mod abi;
#[path = "../rcc/gpu_layout.rs"]
#[allow(dead_code)]
pub(crate) mod layout;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererLayoutProfile {
    Column16,
}
impl RendererLayoutProfile {
    pub fn layout_abi(self) -> u16 {
        abi::GPU_CAPTURE_LAYOUT_COLUMN_ABI
    }
    pub fn plane_bytes(self) -> u32 {
        u32::from(layout::GPU_FRAMEBUFFER_PHYSICAL_SECTORS) * 128
    }
    pub fn allocation_bytes(self) -> u32 {
        u32::from(layout::GPU_PLANE_SECTORS) * 128
    }
    pub fn layout_path(self, shared: &Path) -> PathBuf {
        shared.join("gpu_layout.rs")
    }
    fn texture_layout(self) -> &'static str {
        "pot-column-macro16x16-min512-cache16x8"
    }
    pub fn description(self) -> Value {
        let mut v = json!({"abi":self.layout_abi(),"name":"rgb565-macro16x16-column-pad16",
            "gpu_profile":"production","width":400,"height":240,"logical_pixels":96000,
            "physical_bytes":self.plane_bytes(),"allocation_bytes":self.allocation_bytes(),
            "pixel_bytes":2,"macro_width":16,"macro_height":16,"macro_order":"column",
            "macro_pitch":16,"texture_layout":self.texture_layout()});
        let b = |sector: u16| u32::from(sector) * 128;
        let c = b(layout::GPU_COMMAND_FIRST_SECTOR);
        let o = b(layout::GPU_OBJECT_FIRST_SECTOR);
        let c1 = b(layout::GPU_SLOT1_COMMAND_FIRST_SECTOR);
        let o1 = b(layout::GPU_SLOT1_OBJECT_FIRST_SECTOR);
        v["memory_map"] = json!({"colorA":[b(layout::GPU_COLOR_A_SECTOR),b(layout::GPU_COLOR_B_SECTOR)],
                "colorB":[b(layout::GPU_COLOR_B_SECTOR),b(layout::GPU_DEPTH_SECTOR)],"depth":[b(layout::GPU_DEPTH_SECTOR),c],
                "slot0CB":[c,o],"slot0obj":[o,b(layout::GPU_ASSET_FIRST_SECTOR)],"assets":[b(layout::GPU_ASSET_FIRST_SECTOR),b(layout::GPU_ASSET_END_SECTOR)],
                "UI0":[b(layout::GPU_UI_PAGE0_SECTOR),b(layout::GPU_UI_PAGE1_SECTOR)],"UI1":[b(layout::GPU_UI_PAGE1_SECTOR),c1],
                "slot1CB":[c1,o1],"slot1obj":[o1,0x800000]});
        v
    }
    pub fn recorded_description(self) -> Value {
        let mut v = self.description();
        let fields = v.as_object_mut().unwrap();
        fields.remove("memory_map");
        fields.remove("allocation_bytes");
        fields.insert("address_policy".into(), json!("record-ranges-v1"));
        v
    }
}
struct Resource {
    tag: u16,
    base: u32,
    bytes: u32,
}
/// One immutable selection drives layout origin, manifest and generated hook data.
/// Fields are private so callers cannot select a second conflicting layout.
pub struct RendererCaptureConfiguration {
    pub(super) capture: Option<CaptureConfiguration>,
    pub(super) hook_enabled: bool,
    pub(super) profile: RendererLayoutProfile,
    resources: Vec<Resource>,
    command_abi: u16,
}
impl RendererCaptureConfiguration {
    pub fn without_capture(profile: RendererLayoutProfile) -> Self {
        Self {
            capture: None,
            hook_enabled: false,
            profile,
            resources: vec![],
            command_abi: abi::GPU_CAPTURE_RTT400_COMMAND_ABI,
        }
    }
    pub fn with_default_hook(profile: RendererLayoutProfile) -> Self {
        let mut configuration = Self::without_capture(profile);
        configuration.hook_enabled = true;
        configuration
    }
    pub fn command_abi(&self) -> u16 {
        self.command_abi
    }
    pub fn profile(&self) -> RendererLayoutProfile {
        self.profile
    }
    pub fn from_manifest(gate_path: PathBuf, path: &Path) -> Result<Self, String> {
        let m: Value = serde_json::from_slice(
            &std::fs::read(path)
                .map_err(|e| format!("read capture manifest {}: {e}", path.display()))?,
        )
        .map_err(|e| e.to_string())?;
        let version = m["version"].as_u64().ok_or("capture manifest version")?;
        let profile = RendererLayoutProfile::Column16;
        if !matches!(version, 2 | 3) || m["schema"] != "digitaldesign.gpu-capture-build" {
            return Err("explicit v2/v3 column capture manifest required".into());
        }
        let command_abi = m["command_abi"].as_u64();
        if command_abi != Some(u64::from(abi::GPU_CAPTURE_RTT400_COMMAND_ABI))
            && command_abi != Some(u64::from(abi::GPU_CAPTURE_UNIFORM_RTT400_COMMAND_ABI))
        {
            return Err(
                "unsupported capture command ABI; production requires framebuffer-view support"
                    .into(),
            );
        }
        let description = if version == 3 {
            profile.recorded_description()
        } else {
            profile.description()
        };
        if m["layout"] != description {
            return Err("mixed or unsupported capture layout description".into());
        }
        if m["options"]
            .get("capture_allocation_guards")
            .is_some_and(|v| v != false)
        {
            return Err(
                "allocation guard capture is unsupported; only physical plane spans are exported"
                    .into(),
            );
        }
        let capture = CaptureConfiguration::parse_id(gate_path, &m)?;
        let mut resources = vec![];
        let mut spans = vec![];
        if matches!(version, 2 | 3) {
            let entries = m["resources"]
                .as_array()
                .ok_or("v2 capture requires resource catalog")?;
            let (first, end) = if version == 3 {
                (0u32, 65536u32)
            } else {
                (
                    u32::from(layout::GPU_ASSET_FIRST_SECTOR),
                    u32::from(layout::GPU_ASSET_END_SECTOR),
                )
            };
            for entry in entries {
                let base = entry["address"].as_u64().ok_or("resource address")?;
                let bytes = entry["bytes"].as_u64().ok_or("resource bytes")?;
                if bytes == 0
                    || base % 128 != 0
                    || bytes % 128 != 0
                    || base < u64::from(first) * 128
                    || base
                        .checked_add(bytes)
                        .is_none_or(|n| n > u64::from(end) * 128)
                {
                    return Err("resource escapes profile asset allocation/alignment".into());
                }
                let kind = entry["kind"].as_str().ok_or("v2 resource kind")?;
                let tag = match kind {
                    "mesh" => abi::GPU_CAPTURE_TAG_MESH,
                    "texture" => abi::GPU_CAPTURE_TAG_TEXTURE,
                    "atlas" => abi::GPU_CAPTURE_TAG_ATLAS,
                    "other" => 0,
                    _ => return Err("unsupported resource kind".into()),
                };
                if kind == "texture"
                    && (entry["layout"] != profile.texture_layout()
                        || (profile == RendererLayoutProfile::Column16
                            && (base % 256 != 0 || bytes < 512)))
                {
                    return Err("texture footprint/layout does not match capture profile".into());
                }
                if kind == "atlas"
                    && (base % 65536 != 0
                        || (version == 2 && bytes != 65536)
                        || (version == 3 && bytes < 256))
                {
                    return Err("UI atlas requires aligned glyph storage; v2 retains 64KiB".into());
                }
                spans.push((base, base + bytes));
                resources.push(Resource {
                    tag,
                    base: base as u32,
                    bytes: bytes as u32,
                });
            }
            spans.sort_unstable();
            if spans.windows(2).any(|p| p[0].1 > p[1].0) {
                return Err("overlapping capture resources".into());
            }
            if version == 2
                && [
                    abi::GPU_CAPTURE_TAG_MESH,
                    abi::GPU_CAPTURE_TAG_TEXTURE,
                    abi::GPU_CAPTURE_TAG_ATLAS,
                ]
                .into_iter()
                .any(|tag| !resources.iter().any(|r| r.tag == tag))
            {
                return Err("v2 catalog requires mesh, texture and atlas roots".into());
            }
        }
        Ok(Self {
            capture: Some(capture),
            hook_enabled: true,
            profile,
            resources,
            command_abi: command_abi.unwrap() as u16,
        })
    }
    pub(super) fn asset_check_source(&self) -> String {
        let mut s = String::from(
            "pub fn gpu_capture_manifest_asset(tag:u16,sector:u16,low:u16,high:u16)->bool{\n",
        );
        if self.resources.is_empty() {
            s.push_str("true\n}\n");
            return s;
        }
        for r in self.resources.iter().filter(|r| r.tag != 0) {
            s.push_str(&format!(
                "if tag=={} && sector=={} && low=={} && high=={}{{return true;}}\n",
                r.tag,
                r.base / 128,
                r.bytes & 65535,
                r.bytes >> 16
            ));
        }
        s.push_str("false\n}\n");
        s
    }
}

/// Materialize a host-selected approved source root; origins remain real files.
/// All files retain their exact source bytes except gpu_layout.rs, whose bytes
/// come from the selected system layout. This is not an ordinary module search dir.
pub fn materialize_renderer_sources(
    shared: &Path,
    generated: &Path,
    profile: RendererLayoutProfile,
    dependency: &mut dyn FnMut(&Path),
) -> Result<PathBuf, String> {
    let source = std::fs::canonicalize(shared).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(generated).map_err(|e| e.to_string())?;
    let generated = std::fs::canonicalize(generated).map_err(|e| e.to_string())?;
    if generated.starts_with(&source) || source.starts_with(&generated) {
        return Err("generated capture root must be separate from source root".into());
    }
    let mut origins = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(&source).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let input = if path.file_name().is_some_and(|n| n == "gpu_layout.rs") {
            profile.layout_path(&source)
        } else {
            path.clone()
        };
        dependency(&input);
        let bytes = std::fs::read(&input).map_err(|e| e.to_string())?;
        let dest = generated.join(path.file_name().unwrap());
        if std::fs::read(&dest).ok().as_deref() != Some(&bytes) {
            std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
        }
        origins.insert(
            path.file_name().unwrap().to_string_lossy().into_owned(),
            input,
        );
    }
    let bytes = serde_json::to_vec_pretty(&origins).map_err(|e| e.to_string())?;
    let map = generated.join("source-origins.json");
    if std::fs::read(&map).ok().as_deref() != Some(&bytes) {
        std::fs::write(map, bytes).map_err(|e| e.to_string())?;
    }
    Ok(generated)
}
