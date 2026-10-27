//! 阶段2独立Lighting整数gold；不读取RTL调度或DUT结果。
//! 输入每行：id normalX12 normalY12 normalZ12 pixelX pixelY。
use gpu_v2::lighting::{
    ports::{pixel_center_ndc, Light, Material, PixelInput, Projection},
    sim::{counted, oracle},
    LightingProfile, LightingQuantization,
};
use std::{env, fs, io::Write};

#[allow(dead_code)]
mod selected_h9 {
    pub type H9Entry = audited::Fixed<16, 0, false>;
    include!(concat!(env!("OUT_DIR"), "/lighting_h9.rs"));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if !(3..=4).contains(&args.len()) {
        return Err("INPUT.txt OUTPUT.txt [CONTEXT.txt]".into());
    }
    let input = fs::read_to_string(&args[1])?;
    let mut output = fs::File::create(&args[2])?;
    let kernel = counted::Config {
        rsqrt_q13: true,
        ..counted::Config::lit_queue_resource_profile(
            LightingProfile::Fast,
            LightingQuantization::CompensatedFloor,
        )
    };
    let config = oracle::Config::from_counted(kernel);
    let mut material = Material {
        unlit: false,
        specular_color: [0; 3],
        shininess_code: 8,
    };
    let mut light = Light {
        direction: [-11585, 0, -11585],
        ambient: 32,
        directional: 192,
    };
    let mut projection = Projection::default();
    if args.len() == 4 {
        let values = fs::read_to_string(&args[3])?
            .split_whitespace()
            .map(str::parse::<i32>)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(values.len(), 12, "Lxyz Ia Id rayXY K code specRGB");
        light.direction = [
            values[0].try_into()?,
            values[1].try_into()?,
            values[2].try_into()?,
        ];
        light.ambient = values[3].try_into()?;
        light.directional = values[4].try_into()?;
        projection.ray_scale = [values[5].try_into()?, values[6].try_into()?];
        projection.k = values[7].try_into()?;
        material.shininess_code = values[8].try_into()?;
        material.specular_color = [
            values[9].try_into()?,
            values[10].try_into()?,
            values[11].try_into()?,
        ];
    }
    let full = material.specular_color != [0; 3];
    let mut checked_highlights = 0;
    let mut count = 0;
    for line in input.lines() {
        let v: Vec<i32> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        if v.is_empty() {
            continue;
        }
        assert_eq!(v.len(), 6);
        assert!(count < 524288, "bounded golden pixel input");
        assert!(v[1..4].iter().all(|x| (-2048..=2047).contains(x)));
        assert!((0..400).contains(&v[4]) && (0..240).contains(&v[5]));
        let pixel = PixelInput {
            normal: [v[1] as i16 * 16, v[2] as i16 * 16, v[3] as i16 * 16],
            ndc: pixel_center_ndc(v[4] as u16, v[5] as u16, 400, 240)
                .map_err(|e| format!("{e:?}"))?,
        };
        let mut result = oracle::evaluate_output(pixel, material, light, projection, config)
            .map_err(|e| format!("{e:?}"))?;
        if full {
            // Keep the independent geometric oracle, then use the selected
            // direct-H9 table encoding rather than its older power profile.
            let trace = oracle::evaluate(pixel, material, light, projection, config)
                .map_err(|e| format!("{e:?}"))?;
            let stage = |key: &str| trace.stages.iter().find(|(n, _)| n == key).unwrap().1;
            let distance = 32768 - stage("x");
            let code = usize::from(material.shininess_code);
            let shift = selected_h9::H9_SHIFTS_RAW[code] + 6;
            let index = distance >> shift;
            let fraction = ((distance << 6) >> shift) & 63;
            let word = selected_h9::H9_RAW[code * 32 + (index as usize & 31)];
            let p = if stage("nl") <= 0 || index >= 32 {
                0
            } else {
                i128::from(word & 511) - ((i128::from(word >> 9) * fraction + 32) >> 6)
            };
            let product = i128::from(light.directional) * p;
            let q = product >> 8;
            let r = product & 255;
            result.h = (q + i128::from(r > 128 || r == 128 && q & 1 != 0)) as u16;
        }
        if count < 8 || count % 4096 == 0 || result.h > 0 && checked_highlights < 8 {
            let checked = counted::evaluate_with_config(
                pixel,
                material,
                light,
                projection,
                4096,
                counted::Config {
                    smooth_h9: full,
                    h9_tail_dsp: full,
                    ..kernel
                },
            )
            .map_err(|e| format!("{e:?}"))?;
            assert_eq!(result, checked.output);
            checked_highlights += usize::from(result.h > 0);
        }
        writeln!(output, "{} {} {}", v[0], result.g, result.h)?;
        count += 1;
    }
    eprintln!("Lighting golden: {count} S12F10 pixels, full={full}, checked highlights={checked_highlights}");
    Ok(())
}
