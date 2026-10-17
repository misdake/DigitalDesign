//! 阶段2独立Lighting整数gold；不读取RTL调度或DUT结果。
//! 输入每行：id normalX12 normalY12 normalZ12 pixelX pixelY。
use gpu_v2::lighting::{
    ports::{pixel_center_ndc, Light, Material, PixelInput, Projection},
    sim::{counted, oracle},
    LightingProfile, LightingQuantization,
};
use std::{env, fs, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 3 {
        return Err("INPUT.txt OUTPUT.txt".into());
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
    let material = Material {
        unlit: false,
        specular_color: [0; 3],
        shininess_code: 8,
    };
    let light = Light {
        direction: [-11585, 0, -11585],
        ambient: 32,
        directional: 192,
    };
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
        assert!(count < 65536, "有界gold输入上限");
        assert!(v[1..4].iter().all(|x| (-2048..=2047).contains(x)));
        assert!((0..400).contains(&v[4]) && (0..240).contains(&v[5]));
        let pixel = PixelInput {
            normal: [v[1] as i16 * 16, v[2] as i16 * 16, v[3] as i16 * 16],
            ndc: pixel_center_ndc(v[4] as u16, v[5] as u16, 400, 240)
                .map_err(|e| format!("{e:?}"))?,
        };
        let result = oracle::evaluate_output(pixel, material, light, Projection::default(), config)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            result.h, 0,
            "本用例为真实法线的diffuse模式，不使用旧power作为H9金值"
        );
        if count < 8 {
            let checked = counted::evaluate_with_config(
                pixel,
                material,
                light,
                Projection::default(),
                4096,
                kernel,
            )
            .map_err(|e| format!("{e:?}"))?;
            assert_eq!(result, checked.output);
        }
        writeln!(output, "{} {} {}", v[0], result.g, result.h)?;
        count += 1;
    }
    eprintln!("Lighting独立整数gold：{count}像素，真实S12F10法线，diffuse/CompensatedFloor/q13");
    Ok(())
}
