//! Structural opportunities in oracle goldens, not audited counted/timed work.
use gpu_v2::texture::{ports::*, sim::oracle::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Write,
    path::Path,
};

#[derive(Default)]
struct Counts {
    quads: usize,
    pixels: usize,
    layers: usize,
    groups: usize,
    max_groups_per_pixel: usize,
    dual_mip_pixels: usize,
    naive_coefficient_products: usize,
    coefficient_products_fast_paths: usize,
    coefficient_products_repeated_inputs: usize,
    coefficient_products_unique_inputs: usize,
    distinct_weight_vectors: usize,
    useful_color_products: usize,
    issued_color_product_slots: usize,
    texel_references: usize,
    unique_texels_per_quad: usize,
    unique_tile_keys_per_quad: usize,
    consecutive_hint_keys: usize,
    constant_mip_pixels: usize,
    constant_only_pixels: usize,
    quads_with_constant_mip: usize,
}

#[derive(Default)]
pub struct WorkStudy {
    scenes: BTreeMap<String, Counts>,
}
impl WorkStudy {
    pub fn observe(&mut self, scene: &str, prepared: &PreparedQuad) {
        assert_eq!(
            prepared.config.coefficient_encoding,
            CoefficientEncoding::Unorm
        );
        assert_eq!(prepared.config.coefficient_fraction, 9);
        assert_eq!(prepared.config.coordinate_fraction, 8);
        let c = self.scenes.entry(scene.into()).or_default();
        let start_products = c.coefficient_products_fast_paths;
        c.quads += 1;
        c.pixels += prepared.pixels.len();
        let scale = prepared.config.coefficient_scale();
        let mut products = BTreeSet::new();
        let mut vectors = BTreeSet::new();
        let mut texels = BTreeSet::new();
        let mut tiles = BTreeSet::new();
        let mut last_hint = None;
        let mut has_constant_mip = false;
        for pixel in &prepared.pixels {
            if pixel.layers.iter().any(|layer| layer.n == 0) {
                c.constant_mip_pixels += 1;
                has_constant_mip = true;
                c.constant_only_pixels += usize::from(pixel.layers.len() == 1);
            }
            c.dual_mip_pixels += usize::from(pixel.layers.len() == 2);
            c.groups += pixel.groups.len();
            c.max_groups_per_pixel = c.max_groups_per_pixel.max(pixel.groups.len());
            c.issued_color_product_slots += 12 * pixel.groups.len();
            for group in &pixel.groups {
                tiles.insert(group.key);
                if last_hint != Some(group.key) {
                    c.consecutive_hint_keys += 1;
                    last_hint = Some(group.key);
                }
            }
            for layer in &pixel.layers {
                c.layers += 1;
                c.naive_coefficient_products += 3;
                vectors.insert((layer.parent, layer.fraction));
                // Full parent scale /256 can be strength-reduced. Zero operands
                // also need no generic multiply. All other split products have
                // identical semantics for identical integer operand pairs.
                let rows = [
                    layer.coefficients[0] + layer.coefficients[1],
                    layer.coefficients[2] + layer.coefficients[3],
                ];
                for (total, frac) in [
                    (layer.parent, layer.fraction[1]),
                    (rows[0], layer.fraction[0]),
                    (rows[1], layer.fraction[0]),
                ] {
                    if total > 0 && total < scale && frac > 0 {
                        c.coefficient_products_fast_paths += 1;
                        products.insert((total, frac));
                    }
                }
                for (tap, &w) in layer.taps.iter().zip(&layer.coefficients) {
                    if w != 0 {
                        c.texel_references += 1;
                        c.useful_color_products += 3;
                        // n=0's logical texel is replicated across virtual banks.
                        let logical = 1_u16 << layer.n;
                        texels.insert((layer.n, tap[0] % logical, tap[1] % logical));
                    }
                }
            }
        }
        c.coefficient_products_unique_inputs += products.len();
        // This is an opportunity bound. It requires operand/result reuse state
        // and does not promise that a smaller multiplier count meets throughput.
        let generic_now = c.coefficient_products_fast_paths - start_products;
        c.coefficient_products_repeated_inputs += generic_now - products.len();
        c.distinct_weight_vectors += vectors.len();
        c.unique_texels_per_quad += texels.len();
        c.unique_tile_keys_per_quad += tiles.len();
        c.quads_with_constant_mip += usize::from(has_constant_mip);
    }

    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        writeln!(file,"scene,quads,pixels,layers,groups,max_groups_per_pixel,dual_mip_pixels,naive_coefficient_products,coefficient_products_fast_paths,coefficient_products_repeated_inputs,coefficient_products_unique_inputs,distinct_weight_vectors,useful_color_products,issued_color_product_slots,texel_references,unique_texels_per_quad,unique_tile_keys_per_quad,consecutive_hint_keys,constant_mip_pixels,constant_only_pixels,quads_with_constant_mip")?;
        for (scene, c) in &self.scenes {
            write!(
                file,
                "{scene},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                c.quads,
                c.pixels,
                c.layers,
                c.groups,
                c.max_groups_per_pixel,
                c.dual_mip_pixels,
                c.naive_coefficient_products,
                c.coefficient_products_fast_paths,
                c.coefficient_products_repeated_inputs,
                c.coefficient_products_unique_inputs,
                c.distinct_weight_vectors,
                c.useful_color_products,
                c.issued_color_product_slots,
                c.texel_references,
                c.unique_texels_per_quad,
                c.unique_tile_keys_per_quad,
                c.consecutive_hint_keys
            )?;
            writeln!(
                file,
                ",{},{},{}",
                c.constant_mip_pixels, c.constant_only_pixels, c.quads_with_constant_mip
            )?;
        }
        Ok(())
    }
}

/// Exact nearest integer N/511 over the conserved UNORM9 RGB accumulator range.
/// The odd divisor has no ties. The carry needs only low[8] or an eight-bit sum
/// carry; no wide divider or reciprocal multiply is needed.
pub fn normalize511(n: u32) -> u32 {
    assert!(n <= 255 * 511);
    let hi = n >> 9;
    let lo = n & 511;
    hi + u32::from(hi + lo >= 256)
}

pub fn check_strength_reductions() {
    for n in 0..=255 * 511 {
        assert_eq!(normalize511(n), (n + 255) / 511);
    }
    for frac in 0..256_u32 {
        // Shared quad lambda is converted from eight-bit LOD only once.
        assert_eq!(
            2 * frac - u32::from(frac > 128),
            rne_div(u64::from(511 * frac), 256) as u32
        );
        assert_eq!(2 * frac - u32::from(frac != 0), 511 * frac / 256);
    }
    // Exact lower-mip coordinate floor from the fine-mip coordinate, including
    // negative coordinates and every possible discarded fractional position.
    for raw in -131_072_i64..=131_072 {
        let fine = raw.div_euclid(1024) - 128;
        let coarse = raw.div_euclid(2048) - 128;
        assert_eq!((fine - 128).div_euclid(2), coarse);
    }
}
