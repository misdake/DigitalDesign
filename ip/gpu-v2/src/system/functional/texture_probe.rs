//! Optional bounds at the functional sampler, before UV clamp and address wrap.
use std::collections::BTreeMap;

/// Key: encoded texture descriptor and physical meshlet byte address.
pub type TextureProbe = BTreeMap<(u32, u32), TextureUse>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureTap {
    /// Stored mip side exponent; the side is `1 << level`, not the LOD.
    pub level: u8,
    pub xy: [i128; 2],
    pub weight: u16,
    pub rgba: [u8; 4],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureSample {
    pub descriptor: u32,
    pub meshlet: u32,
    pub target: u32,
    pub xy: [u16; 2],
    pub uv: [i128; 2],
    pub lod: u8,
    pub uniform_color: u32,
    pub unlit: bool,
    pub shaded: [u8; 4],
    pub taps: Vec<TextureTap>,
    pub depth_passed: bool,
    /// Last full-opacity DRAW writer, surviving later full-opacity writes.
    pub visible: bool,
    /// A later partial-alpha DRAW covers this writer; keep it separate in analysis.
    pub overlaid: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureSampleProbe {
    pub minimum_lod: u8,
    pub maximum_samples: usize,
    pub samples: Vec<TextureSample>,
    owners: BTreeMap<(u32, [u16; 2]), usize>,
}
impl TextureSampleProbe {
    pub(super) fn new(minimum_lod: u8, maximum_samples: usize) -> Self {
        Self {
            minimum_lod,
            maximum_samples,
            samples: Vec::new(),
            owners: BTreeMap::new(),
        }
    }
    pub(super) fn begin(&mut self, sample: TextureSample) -> Result<usize, String> {
        if self.samples.len() >= self.maximum_samples {
            return Err("texture sample probe limit".into());
        }
        let index = self.samples.len();
        self.samples.push(sample);
        Ok(index)
    }
    pub(super) fn resolve(&mut self, target: u32, xy: [u16; 2], sample: Option<usize>, alpha: u8) {
        if let Some(index) = sample {
            self.samples[index].depth_passed = true;
        }
        let key = (target, xy);
        if alpha == 64 {
            if let Some(index) = self.owners.remove(&key) {
                self.samples[index].visible = false;
            }
            if let Some(index) = sample {
                self.samples[index].visible = true;
                self.owners.insert(key, index);
            }
        } else if alpha != 0 {
            if let Some(&index) = self.owners.get(&key) {
                self.samples[index].overlaid = true;
            }
        }
    }
    pub(super) fn clear(&mut self, target: Option<u32>) {
        let keys: Vec<_> = self
            .owners
            .keys()
            .filter(|(base, _)| target.is_none_or(|t| *base == t))
            .copied()
            .collect();
        for key in keys {
            let index = self.owners.remove(&key).unwrap();
            self.samples[index].visible = false;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureUse {
    pub samples: u64,
    pub uv_min: [i128; 2],
    pub uv_max: [i128; 2],
    pub outside_normalized: u64,
    pub quads: u64,
    pub raw_lod_q8_min: i32,
    pub raw_lod_q8_max: i32,
    /// Effective LOD in eighths; each texel read retains its real mip level.
    pub lod: [u64; 81],
    pub taps: [u64; 11],
    pub tap_min: [[i128; 2]; 11],
    pub tap_max: [[i128; 2]; 11],
}
impl Default for TextureUse {
    fn default() -> Self {
        Self {
            samples: 0,
            uv_min: [i128::MAX; 2],
            uv_max: [i128::MIN; 2],
            outside_normalized: 0,
            quads: 0,
            raw_lod_q8_min: i32::MAX,
            raw_lod_q8_max: i32::MIN,
            lod: [0; 81],
            taps: [0; 11],
            tap_min: [[i128::MAX; 2]; 11],
            tap_max: [[i128::MIN; 2]; 11],
        }
    }
}
impl TextureUse {
    pub(super) fn quad(&mut self, raw: i32) {
        self.quads += 1;
        self.raw_lod_q8_min = self.raw_lod_q8_min.min(raw);
        self.raw_lod_q8_max = self.raw_lod_q8_max.max(raw);
    }
    pub(super) fn sample(&mut self, uv: [i128; 2], lod: u8) {
        self.samples += 1;
        self.lod[usize::from(lod)] += 1;
        self.outside_normalized += u64::from(uv.iter().any(|&v| !(0..=65535).contains(&v)));
        for (axis, value) in uv.into_iter().enumerate() {
            self.uv_min[axis] = self.uv_min[axis].min(value);
            self.uv_max[axis] = self.uv_max[axis].max(value);
        }
    }
    pub(super) fn tap(&mut self, level: u8, xy: [i128; 2]) {
        let at = usize::from(level);
        self.taps[at] += 1;
        for (axis, value) in xy.into_iter().enumerate() {
            self.tap_min[at][axis] = self.tap_min[at][axis].min(value);
            self.tap_max[at][axis] = self.tap_max[at][axis].max(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> TextureSample {
        TextureSample {
            descriptor: 0,
            meshlet: 128,
            target: 256,
            xy: [3, 4],
            uv: [0, 0],
            lod: 48,
            uniform_color: u32::MAX,
            unlit: true,
            shaded: [255; 4],
            taps: Vec::new(),
            depth_passed: false,
            visible: false,
            overlaid: false,
        }
    }
    #[test]
    fn pixel_ownership_retires_occluded_samples_and_marks_partial_overlays() {
        let mut probe = TextureSampleProbe::new(48, 2);
        let a = probe.begin(sample()).unwrap();
        probe.resolve(256, [3, 4], Some(a), 64);
        assert!(probe.samples[a].visible && probe.samples[a].depth_passed);
        probe.resolve(256, [3, 4], None, 0);
        assert!(probe.samples[a].visible && !probe.samples[a].overlaid);
        probe.resolve(256, [3, 4], None, 32);
        assert!(probe.samples[a].visible && probe.samples[a].overlaid);
        let b = probe.begin(sample()).unwrap();
        probe.resolve(256, [3, 4], Some(b), 64);
        assert!(!probe.samples[a].visible && probe.samples[b].visible);
        assert!(!probe.samples[b].overlaid);
        assert!(probe.begin(sample()).is_err());
        probe.clear(Some(512));
        assert!(probe.samples[b].visible);
        probe.resolve(256, [3, 4], None, 64);
        assert!(!probe.samples[b].visible);
        probe.resolve(256, [3, 4], Some(b), 64);
        probe.clear(None);
        assert!(!probe.samples[b].visible);
    }
}
