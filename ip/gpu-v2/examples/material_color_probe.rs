//! Bounded arithmetic study, not a hardware fit or a selected context change.
use gpu_v2::material_color::{
    gain_table, prepare_gray_intensities, prepare_specular_intensity, uniform_product,
};
use std::{fs, path::PathBuf};

#[derive(Default)]
struct Error {
    count: u64,
    changed: u64,
    sum: u64,
    max: u16,
    witness: [u16; 5],
}
impl Error {
    fn push(&mut self, a: u16, b: u16, witness: [u16; 5]) {
        let d = a.abs_diff(b);
        self.count += 1;
        self.changed += u64::from(d != 0);
        self.sum += u64::from(d);
        if d > self.max {
            self.max = d;
            self.witness = witness;
        }
    }
    fn json(&self) -> String {
        format!(
            "{{\"samples\":{},\"changed\":{},\"max_rgb8\":{},\"mean_rgb8\":{},\"witness\":{:?}}}",
            self.count,
            self.changed,
            self.max,
            self.sum as f64 / self.count as f64,
            self.witness
        )
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(std::env::args().nth(1).ok_or("output folder required")?);
    fs::create_dir_all(&output)?;
    let mut diffuse = Error::default();
    let mut specular = Error::default();
    let mut gray_light = Error::default();
    // Fixed code-domain budgets: no open-ended simulation or image scan.
    const MAX_DIFFUSE: u64 = 256 * 256 * 512;
    const MAX_SPECULAR: u64 = 256 * 257 * 257;
    for tint in 0..=255_u8 {
        let table = gain_table([tint; 3]);
        for base in 0..=255_u8 {
            for g in 0..512_u16 {
                let old =
                    ((u32::from(uniform_product(base, tint)) * u32::from(g)) >> 8).min(255) as u16;
                let new =
                    ((u32::from(base) * u32::from(table[usize::from(g)][0])) >> 8).min(255) as u16;
                diffuse.push(old, new, [u16::from(base), u16::from(tint), g, 0, 0]);
            }
        }
    }
    for s in 0..=255_u8 {
        for id in 0..=256_u16 {
            let ks = prepare_specular_intensity([s; 3], id)?[0];
            for p in 0..=256_u16 {
                let product = u32::from(id) * u32::from(p);
                let q = product >> 8;
                let r = product & 255;
                let h = q + u32::from(r > 128 || (r == 128 && q & 1 != 0));
                let old = ((u32::from(s) * h) >> 8) as u16;
                let new = ((u32::from(ks) * u32::from(p)) >> 8) as u16;
                specular.push(old, new, [u16::from(s), id, p, 0, 0]);
            }
        }
    }
    assert_eq!(diffuse.count, MAX_DIFFUSE);
    assert_eq!(specular.count, MAX_SPECULAR);
    let rne256 = |n: u32| {
        let q = n >> 8;
        let r = n & 255;
        q + u32::from(r > 128 || (r == 128 && q & 1 != 0))
    };
    const BASES: [u8; 6] = [0, 16, 64, 127, 192, 255];
    const INTENSITIES: [u16; 9] = [0, 32, 64, 96, 128, 160, 192, 224, 256];
    const MAX_GRAY: u64 = 256 * 6 * 9 * 9 * 257;
    for gray in 0..=255_u8 {
        for ia in INTENSITIES {
            for id in INTENSITIES {
                let [new_ia, new_id] = prepare_gray_intensities(gray, ia, id)?;
                for nl in 0..=256_u16 {
                    let old_g = (u32::from(ia) + rne256(u32::from(id) * u32::from(nl))).min(511);
                    let new_g =
                        (u32::from(new_ia) + rne256(u32::from(new_id) * u32::from(nl))).min(511);
                    for base in BASES {
                        let old =
                            ((u32::from(uniform_product(base, gray)) * old_g) >> 8).min(255) as u16;
                        let new = ((u32::from(base) * new_g) >> 8).min(255) as u16;
                        gray_light.push(old, new, [u16::from(gray), ia, id, nl, u16::from(base)]);
                    }
                }
            }
        }
    }
    assert_eq!(gray_light.count, MAX_GRAY);
    let report=format!("{{\n\"scope\":\"finite CPU-side material arithmetic; no Lighting/RTL adoption\",\n\"gain_table_vs_current_tint_position\":{},\n\"move_Id_from_h_into_Ks\":{},\n\"gray_tint_into_Ia_Id_bounded_grid\":{}\n}}\n",diffuse.json(),specular.json(),gray_light.json());
    fs::write(output.join("material-arithmetic.json"), &report)?;
    println!("{report}");
    Ok(())
}
