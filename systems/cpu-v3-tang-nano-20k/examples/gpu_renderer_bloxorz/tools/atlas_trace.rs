// Included only in an observation copy; never linked into production GPU code.
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Mutex};
#[derive(Default)]
struct Counts {
    covered: u64,
    passed: u64,
    taps: u64,
    passed_taps: u64,
    outside: u64,
    passed_outside: u64,
    levels: BTreeMap<u8, u64>,
    footprint: BTreeMap<u8, u64>,
}
struct Active {
    key: String,
    rect: [usize; 4],
    source_size: usize,
    passed: bool,
    weight: u32,
}
static DATA: Mutex<(BTreeMap<String, Counts>, Option<Active>)> =
    Mutex::new((BTreeMap::new(), None));
static FOOTPRINT: Mutex<u8> = Mutex::new(0);
static EXPECTED_UV: Mutex<[i128; 2]> = Mutex::new([0; 2]);
pub fn set_expected_uv(uv: [i128; 2]) {
    *EXPECTED_UV.lock().unwrap() = uv;
}
pub fn set_footprint(lod: u8) {
    *FOOTPRINT.lock().unwrap() = lod;
}

fn rectangle(base: usize, uv: [i128; 2]) -> Option<([usize; 4], usize, String)> {
    if (0x760000..0x780000).contains(&base) {
        return Some((
            [0, 0, 128, 128],
            128,
            format!("carrier-{}", (base - 0x760000) / 32768),
        ));
    }
    let size = if base == 0x680000 {
        512
    } else if base == 0x740000 {
        256
    } else {
        return None;
    };
    let [x, y] = uv.map(|v| (((v & 65535) * size as i128) >> 16) as usize);
    if size == 512 {
        let (left, top, edge, name) = if x < 256 && y < 256 {
            (
                x / 128 * 128,
                y / 128 * 128,
                128,
                format!("floor-{}", (1 - y / 128) * 2 + x / 128),
            )
        } else if y < 256 {
            (256, 0, 256, "side-a".into())
        } else if x < 256 {
            (0, 256, 256, "top".into())
        } else {
            (256, 256, 256, "side-b".into())
        };
        return Some(([left, top, left + edge, top + edge], size, name));
    }
    for slot in 0..60 {
        let (a, b, e) = if slot < 32 {
            ((slot % 8) * 32, (slot / 8) * 32, 32)
        } else if slot < 48 {
            let c = (slot - 32) / 2;
            if slot % 2 == 0 {
                (c * 32, 128, 32)
            } else {
                (c * 8, 160, 8)
            }
        } else if slot < 56 {
            let v = (slot - 48) / 2;
            if slot % 2 == 0 {
                (v * 48, 176, 48)
            } else {
                (v * 32, 224, 32)
            }
        } else {
            (128 + (slot - 56) * 32, 224, 32)
        };
        if (a..a + e).contains(&x) && (b..b + e).contains(&y) {
            return Some(([a, b, a + e, b + e], size, format!("marker-{slot}")));
        }
    }
    None
}
pub fn begin(base: usize, n: u8, mip: bool, filter: u8, _uv: [i128; 2], lod: u8, passed: bool) {
    let mut data = DATA.lock().unwrap();
    data.1 = rectangle(base, *EXPECTED_UV.lock().unwrap()).map(|(rect, size, name)| {
        let key = format!("{base:#x}/{name}/n{n}/mip{mip}/filter{filter}/lod{lod}");
        let counts = data.0.entry(key.clone()).or_default();
        counts.covered += 1;
        counts.passed += u64::from(passed);
        if passed {
            *counts
                .footprint
                .entry(*FOOTPRINT.lock().unwrap())
                .or_default() += 1;
        }
        Active {
            key,
            rect,
            source_size: size,
            passed,
            weight: 0,
        }
    });
}
pub fn weight(value: u32) {
    if let Some(active) = DATA.lock().unwrap().1.as_mut() {
        active.weight = value;
    }
}
pub fn tap(level: u8, x: usize, y: usize) {
    let mut data = DATA.lock().unwrap();
    if let Some(active) = data.1.as_ref() {
        let scale = active.source_size as f64 / (1usize << level) as f64;
        let outside = (x as f64 * scale) < active.rect[0] as f64
            || (y as f64 * scale) < active.rect[1] as f64
            || ((x + 1) as f64 * scale) > active.rect[2] as f64
            || ((y + 1) as f64 * scale) > active.rect[3] as f64;
        let key = active.key.clone();
        let passed = active.passed;
        let nonzero = active.weight != 0;
        if nonzero {
            let counts = data.0.get_mut(&key).unwrap();
            counts.taps += 1;
            counts.passed_taps += u64::from(passed);
            counts.outside += u64::from(outside);
            counts.passed_outside += u64::from(outside && passed);
            *counts.levels.entry(level).or_default() += 1;
        }
    }
}
pub fn end() {
    DATA.lock().unwrap().1 = None;
}
pub fn finish(submission: usize, output: &std::path::Path) {
    let mut data = DATA.lock().unwrap();
    let rows:Vec<Value>=data.0.iter().map(|(key,c)|json!({"key":key,"covered_valid_fragments":c.covered,"depth_passing_fragments":c.passed,"nonzero_weighted_tap_reads":c.taps,"depth_passing_tap_reads":c.passed_taps,"tap_source_footprint_outside_own_slot":c.outside,"depth_passing_outside_taps":c.passed_outside,"tap_mip_exponent_counts":c.levels,"depth_passing_unclamped_base_only_LOD_q3":c.footprint})).collect();
    std::fs::write(output.join(format!("atlas-statistics-{submission}.json")),serde_json::to_vec_pretty(&json!({"submission":submission,"rows":rows,"counting":"valid covered fragments are shaded before ROP depth test; pass counts use actual current depth. Tap counts include every nonzero weighted read, not unique pixels or elapsed cycles. Outside includes mip source footprints overlapping another slot; no final-visibility claim."})).unwrap()).unwrap();
    data.0.clear();
    data.1 = None;
}
