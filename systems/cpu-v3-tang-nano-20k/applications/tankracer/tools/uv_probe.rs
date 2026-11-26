//! Application-facing serialization of sampler observations, without GPU timing claims.
use gpu_v2::system::functional::TextureProbe;
use std::{error::Error, fs, path::Path};

pub fn write_samples(
    folder: &Path,
    frame: usize,
    probe: &gpu_v2::system::functional::TextureSampleProbe,
) -> Result<(), Box<dyn Error>> {
    let rows: Vec<_> = probe
        .samples
        .iter()
        .map(|s| {
            let taps: Vec<_> = s
                .taps
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "level":t.level,"xy":t.xy,"weight":t.weight,"rgba":t.rgba,
                    })
                })
                .collect();
            serde_json::json!({"descriptor":s.descriptor,"meshlet":s.meshlet,"target":s.target,
            "xy":s.xy,"uv_q16":s.uv,"lod_eighths":s.lod,"uniform_color":s.uniform_color,
            "unlit":s.unlit,"shaded":s.shaded,"depth_passed":s.depth_passed,
            "visible":s.visible,"overlaid":s.overlaid,"taps":taps})
        })
        .collect();
    fs::write(
        folder.join(format!("frame-{frame:03}.samples.json")),
        serde_json::to_vec(&serde_json::json!({
            "format":"tankracer.texture-samples.v1","frame":frame,"minimum_lod_eighths":probe.minimum_lod,
            "maximum_samples":probe.maximum_samples,"samples":rows,
            "scope":"Actual nonzero-weight sampler taps. Visible identifies the surviving full-opacity DRAW writer; later partial-alpha overlays are marked separately. CLEAR removes owners; UI/MEMCPY invalidate ownership observations conservatively.",
        }))?,
    )?;
    Ok(())
}

pub fn write(folder: &Path, frame: usize, probe: &TextureProbe) -> Result<(), Box<dyn Error>> {
    let rows: Vec<_> = probe.iter().map(|(&(word, meshlet), p)| {
        let levels: Vec<_> = (0..11).filter(|&i| p.taps[i] != 0).map(|i| serde_json::json!({
            "side":1<<i,"taps":p.taps[i],"min":p.tap_min[i],"max":p.tap_max[i],
        })).collect();
        serde_json::json!({"descriptor":word,"meshlet":meshlet,"base":(word&65535)*128,"size":1<<((word>>16)&15),
            "grid_log2":(word>>26)&3,"rgba4444":word&(1<<24)!=0,"clamp":word&(1<<25)!=0,
            "quads":p.quads,"raw_lod_q8_min":p.raw_lod_q8_min,"raw_lod_q8_max":p.raw_lod_q8_max,
            "samples":p.samples,"uv_q16_min":p.uv_min,"uv_q16_max":p.uv_max,
            "outside_normalized":p.outside_normalized,"lod_eighths_histogram":p.lod.as_slice(),"levels":levels})
    }).collect();
    fs::write(
        folder.join(format!("frame-{frame:03}.uv.json")),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format":"tankracer.uv-probe.v1","frame":frame,
            "scope":"All real shaded fragments before clamp; nonzero-weight taps before address wrap. Full functional replay, checked against physical colour/depth including padding. Not helper-lane bounds or GPU cycles.","textures":rows,
        }))?,
    )?;
    Ok(())
}
