//! Host-owned capture profile. The wire ABI remains in gpu_capture_abi.rs.
use super::CaptureConfiguration;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
#[path = "../rcc/gpu_capture_abi.rs"]
#[allow(dead_code)]
pub(crate) mod abi;
#[path = "../rcc/gpu_layout_column16.rs"]
#[allow(dead_code)]
mod column;
#[path = "../rcc/gpu_layout.rs"]
#[allow(dead_code)]
pub(crate) mod row;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererLayoutProfile {
    Row,
    Column16,
}
impl RendererLayoutProfile {
    pub fn layout_abi(self) -> u16 {
        match self {
            Self::Row => abi::GPU_CAPTURE_LAYOUT_ROW_ABI,
            Self::Column16 => abi::GPU_CAPTURE_LAYOUT_COLUMN_ABI,
        }
    }
    pub fn plane_bytes(self) -> u32 {
        u32::from(match self {
            Self::Row => row::GPU_FRAMEBUFFER_PHYSICAL_SECTORS,
            Self::Column16 => column::GPU_FRAMEBUFFER_PHYSICAL_SECTORS,
        }) * 128
    }
    pub fn allocation_bytes(self) -> u32 {
        match self {
            Self::Row => self.plane_bytes(),
            Self::Column16 => u32::from(column::GPU_PLANE_SECTORS) * 128,
        }
    }
    pub fn layout_path(self, shared: &Path) -> PathBuf {
        shared.join(match self {
            Self::Row => "gpu_layout.rs",
            Self::Column16 => "gpu_layout_column16.rs",
        })
    }
    fn texture_layout(self) -> &'static str {
        match self {
            Self::Row => "pot-row-macro8x8",
            Self::Column16 => "pot-column-macro16x16-min512-cache16x8",
        }
    }
    pub fn description(self) -> Value {
        let mut v = json!({"abi":self.layout_abi(),"name":match self {Self::Row=>"rgb565-macro16x16-row-pitch25",Self::Column16=>"rgb565-macro16x16-column-pad16"},
            "gpu_profile":match self {Self::Row=>"production",Self::Column16=>"texture16x8-column16"},
            "width":400,"height":240,"logical_pixels":96000,"physical_bytes":self.plane_bytes(),"allocation_bytes":self.allocation_bytes(),
            "pixel_bytes":2,"macro_width":16,"macro_height":16,"macro_order":if self==Self::Row{"row"}else{"column"},
            "macro_pitch":if self==Self::Row{25}else{16},"texture_layout":self.texture_layout()});
        if self == Self::Column16 {
            let b = |sector: u16| u32::from(sector) * 128;
            let c = b(column::GPU_COMMAND_FIRST_SECTOR);
            let o = b(column::GPU_OBJECT_FIRST_SECTOR);
            let c1 = b(column::GPU_SLOT1_COMMAND_FIRST_SECTOR);
            let o1 = b(column::GPU_SLOT1_OBJECT_FIRST_SECTOR);
            v["memory_map"] = json!({"colorA":[b(column::GPU_COLOR_A_SECTOR),b(column::GPU_COLOR_B_SECTOR)],
                "colorB":[b(column::GPU_COLOR_B_SECTOR),b(column::GPU_DEPTH_SECTOR)],"depth":[b(column::GPU_DEPTH_SECTOR),c],
                "slot0CB":[c,o],"slot0obj":[o,b(column::GPU_ASSET_FIRST_SECTOR)],"assets":[b(column::GPU_ASSET_FIRST_SECTOR),b(column::GPU_ASSET_END_SECTOR)],
                "UI0":[b(column::GPU_UI_PAGE0_SECTOR),b(column::GPU_UI_PAGE1_SECTOR)],"UI1":[b(column::GPU_UI_PAGE1_SECTOR),c1],
                "slot1CB":[c1,o1],"slot1obj":[o1,0x800000]});
        }
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
    pub(super) profile: RendererLayoutProfile,
    resources: Vec<Resource>,
}
impl RendererCaptureConfiguration {
    pub fn without_capture(profile: RendererLayoutProfile) -> Self {
        Self {
            capture: None,
            profile,
            resources: vec![],
        }
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
        let version = match m.get("version") {
            None => 1,
            Some(v) => v.as_u64().ok_or("capture version must be an integer")?,
        };
        let profile = match version {
            1 if m.get("layout").is_none() => {
                if m.get("command_abi").is_some_and(|v| v.as_u64() != Some(1)) {
                    return Err("legacy row requires command ABI1".into());
                }
                RendererLayoutProfile::Row
            }
            2 => {
                if m["schema"] != "digitaldesign.gpu-capture-build" {
                    return Err("unsupported capture build schema".into());
                }
                if m["command_abi"].as_u64() != Some(u64::from(abi::GPU_CAPTURE_NATIVE_COMMAND_ABI))
                {
                    return Err(
                        "capture requires native command ABI1; RTT command ABI2 is unsupported"
                            .into(),
                    );
                }
                [RendererLayoutProfile::Row, RendererLayoutProfile::Column16]
                    .into_iter()
                    .find(|p| m["layout"] == p.description())
                    .ok_or("mixed or unsupported capture layout description")?
            }
            _ => return Err("unsupported capture manifest version/layout".into()),
        };
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
        if version == 2 {
            let entries = m["resources"]
                .as_array()
                .ok_or("v2 capture requires resource catalog")?;
            let (first, end) = match profile {
                RendererLayoutProfile::Row => {
                    (row::GPU_ASSET_FIRST_SECTOR, row::GPU_ASSET_END_SECTOR)
                }
                RendererLayoutProfile::Column16 => {
                    (column::GPU_ASSET_FIRST_SECTOR, column::GPU_ASSET_END_SECTOR)
                }
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
                if kind == "atlas" && (base % 65536 != 0 || bytes != 65536) {
                    return Err("UI atlas must retain its complete 64KiB ABI".into());
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
            if [
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
            profile,
            resources,
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
    if profile == RendererLayoutProfile::Row {
        return Ok(shared.to_path_buf());
    }
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
