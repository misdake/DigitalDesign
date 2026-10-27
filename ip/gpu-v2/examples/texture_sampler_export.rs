//! Export the complete serial sampler for repeatable whole-module PnR.
//! Runtime's overlapping controller is a separate, not yet complete RTL top.
use gpu_v2::texture::{ports::Slot, rtl::sampler, sim::staged::bound::serial};
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: texture_sampler_export OUTPUT_DIRECTORY SLOT_COUNT(1|16)".into());
    }
    let count: usize = args[1].parse()?;
    if ![1, 16].contains(&count) {
        return Err("slot count must be 1 or 16".into());
    }
    let slots: Vec<_> = (0..count)
        .map(|i| Slot {
            base_address: 0x0100_0000 + i as u32 * 0x0100_0000,
            has_full_mip: i % 3 != 1,
            max_size_log2: 10 - (i % 7) as u8,
            valid: true,
        })
        .collect();
    let config = serial::Config {
        nearest_bypass: false,
        short_alignment: true,
    };
    let plan = sampler::build_with_config(&slots, config)?;
    let dir = PathBuf::from(&args[0]);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("sampler.v"),
        format!("`define GPU_V2_GOWIN_DSP\n{}", plan.source),
    )?;
    let mut receipt = String::from("scope=complete serial sampler; not overlapping Runtime\nshort_alignment=true\nnearest_bypass=false\n");
    for (i, slot) in slots.iter().enumerate() {
        receipt.push_str(&format!(
            "slot{i}=base:{:08x},max_n:{},has_mip:{}\n",
            slot.base_address, slot.max_size_log2, slot.has_full_mip
        ));
    }
    std::fs::write(dir.join("configuration.txt"), receipt)?;
    println!(
        "Exported complete serial sampler with {count} slots to {}",
        dir.display()
    );
    Ok(())
}
