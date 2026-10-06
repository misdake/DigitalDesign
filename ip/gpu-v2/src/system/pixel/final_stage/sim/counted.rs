//! Closed audited fixed-point implementation of the final-color kernel.
//!
//! Every product, add, compare, select and narrowing is one event in a sealed
//! `audited::Model`. No divider exists; the `/255` and `/256` roundings are the
//! bounded shift/add forms from [`super::super::math`]. The key is an input row
//! published unchanged, so it is carried by the same frame and stages.

use super::super::{math, Input, Output};
use audited::{Fixed, FrameReport, Model};

pub struct Report {
    pub frame: FrameReport,
    pub outputs: Vec<Output>,
}

fn fifo<T>(v: Vec<T>) -> Vec<i128>
where
    T: Into<i128>,
{
    v.into_iter().map(Into::into).collect()
}

/// Run the whole pixel batch through one bounded numerical frame.
pub fn run(pixels: &[Input]) -> Result<Report, String> {
    if pixels.is_empty() || pixels.len() > 4096 {
        return Err("final counted batch bounds".into());
    }
    for pixel in pixels {
        pixel.validate()?;
    }
    let mut model = Model::numerical();
    let err = |e: audited::Fault| format!("{e:?}");

    let keys = fifo(pixels.iter().map(|p| p.key).collect::<Vec<_>>());
    let tint = [
        fifo(pixels.iter().map(|p| p.tint[0]).collect::<Vec<_>>()),
        fifo(pixels.iter().map(|p| p.tint[1]).collect::<Vec<_>>()),
        fifo(pixels.iter().map(|p| p.tint[2]).collect::<Vec<_>>()),
    ];
    let texture = [
        fifo(pixels.iter().map(|p| p.texture[0]).collect::<Vec<_>>()),
        fifo(pixels.iter().map(|p| p.texture[1]).collect::<Vec<_>>()),
        fifo(pixels.iter().map(|p| p.texture[2]).collect::<Vec<_>>()),
    ];
    let specular = [
        fifo(pixels.iter().map(|p| p.specular[0]).collect::<Vec<_>>()),
        fifo(pixels.iter().map(|p| p.specular[1]).collect::<Vec<_>>()),
        fifo(pixels.iter().map(|p| p.specular[2]).collect::<Vec<_>>()),
    ];
    let g = fifo(pixels.iter().map(|p| p.g).collect::<Vec<_>>());
    let h = fifo(pixels.iter().map(|p| p.h).collect::<Vec<_>>());

    let key = model.input::<6, 0, false>("key", &keys).map_err(err)?;
    let tint_mem = [
        model
            .input::<8, 0, false>("tint_r", &tint[0])
            .map_err(err)?,
        model
            .input::<8, 0, false>("tint_g", &tint[1])
            .map_err(err)?,
        model
            .input::<8, 0, false>("tint_b", &tint[2])
            .map_err(err)?,
    ];
    let texture_mem = [
        model
            .input::<8, 0, false>("tex_r", &texture[0])
            .map_err(err)?,
        model
            .input::<8, 0, false>("tex_g", &texture[1])
            .map_err(err)?,
        model
            .input::<8, 0, false>("tex_b", &texture[2])
            .map_err(err)?,
    ];
    let specular_mem = [
        model
            .input::<8, 0, false>("spec_r", &specular[0])
            .map_err(err)?,
        model
            .input::<8, 0, false>("spec_g", &specular[1])
            .map_err(err)?,
        model
            .input::<8, 0, false>("spec_b", &specular[2])
            .map_err(err)?,
    ];
    let g_mem = model.input::<9, 0, false>("g", &g).map_err(err)?;
    let h_mem = model.input::<9, 0, false>("h", &h).map_err(err)?;

    let f = model
        .compute("final_stage", pixels.len() * 160 + 256)
        .map_err(err)?;
    let mut index = Fixed::<16, 0, false>::constant::<0>();
    let mut execute = || -> Result<(), audited::Fault> {
        for i in 0..pixels.len() {
            let k = f.read(key.indexed(index))?;
            f.publish(&format!("key.{i}"), k)?;
            for c in 0..3 {
                let tint_v = f.read(tint_mem[c].indexed(index))?;
                let texture_v = f.read(texture_mem[c].indexed(index))?;
                let specular_v = f.read(specular_mem[c].indexed(index))?;
                let g_v = f.read(g_mem.indexed(index))?;
                let h_v = f.read(h_mem.indexed(index))?;
                // base = RNE(tint * texture / 255): bounded multiply/add.
                let product = f.product::<16, 0, false>(tint_v, texture_v)?;
                let t = f.add::<16, 0, false>(product, Fixed::<16, 0, false>::constant::<128>())?;
                let eighth = f.slice::<8, 0, false, 8>(t)?;
                let corrected =
                    f.add::<16, 0, false>(t, f.resize_exact::<16, 0, false>(eighth)?)?;
                let base = f.slice::<8, 0, false, 8>(corrected)?;
                // color = RNE((base * g + specular * h) / 256), saturated.
                let bg = f.product::<17, 0, false>(base, g_v)?;
                let sh = f.product::<17, 0, false>(specular_v, h_v)?;
                let sum = f.add::<18, 0, false>(bg, sh)?;
                let lsb = f.slice::<1, 0, false, 8>(sum)?;
                let half = f.add::<18, 0, false>(sum, Fixed::<18, 0, false>::constant::<127>())?;
                let rounded_in =
                    f.add::<18, 0, false>(half, f.resize_exact::<18, 0, false>(lsb)?)?;
                let rounded = f.slice::<10, 0, false, 8>(rounded_in)?;
                let over = f.less(Fixed::<10, 0, false>::constant::<255>(), rounded)?;
                let saturated =
                    f.select(over, Fixed::<10, 0, false>::constant::<255>(), rounded)?;
                let out = f.resize_exact::<8, 0, false>(saturated)?;
                f.publish(&format!("rgb.{i}.{c}"), out)?;
            }
            // The loop index is a host addressing device, not pixel arithmetic;
            // the final increment is dead and is not emitted.
            if i + 1 < pixels.len() {
                index = f.add_same(index, Fixed::<16, 0, false>::constant::<1>())?;
            }
        }
        Ok(())
    };
    execute().map_err(err)?;
    let frame = f.finish();
    frame.audit().map_err(err)?;

    let outputs = (0..pixels.len())
        .map(|i| {
            let key = frame
                .outputs
                .iter()
                .find(|o| o.name == format!("key.{i}"))
                .map(|o| o.raw as u8)
                .ok_or_else(|| format!("missing key output {i}"))?;
            let mut rgb = [0u8; 3];
            for (c, slot) in rgb.iter_mut().enumerate() {
                let name = format!("rgb.{i}.{c}");
                let out = frame
                    .outputs
                    .iter()
                    .find(|o| o.name == name)
                    .ok_or_else(|| format!("missing rgb publication {name}"))?;
                *slot = out.raw as u8;
            }
            Ok(Output::new(key, rgb))
        })
        .collect::<Result<Vec<_>, String>>()?;
    for (pixel, out) in pixels.iter().zip(&outputs) {
        if out.rgb != math::reference_rgb(pixel) || out.key != pixel.key {
            return Err("chosen final ancestors differ from reference".into());
        }
    }
    Ok(Report { frame, outputs })
}
