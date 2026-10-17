//! Export current VLIW integration profiles with one fixed comparison fixture.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::pixel::{direct_lod, quad_ingress},
    texture::ports::Slot,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 3 {
        return Err("OUTPUT direct-lod|backend|full|frontend [portable]".into());
    }
    let portable = args.iter().skip(3).any(|s| s == "portable");
    let backend = if portable {
        quad_ingress::rtl::Backend::Portable
    } else {
        quad_ingress::rtl::Backend::Native
    };
    let slots = [Slot {
        base_address: 0x1000,
        max_size_log2: 6,
        has_full_mip: true,
        valid: true,
    }];
    let surface = MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x140000,
        width: 400,
        height: 240,
    };
    let source = match args[2].as_str() {
        "direct-lod" => direct_lod::verilog_selected(&slots, surface, backend)?,
        "backend" => {
            gpu_v2::system::pixel::vliw_backend::verilog_selected(&slots, surface, backend)?
        }
        "full" => gpu_v2::system::vliw::verilog(&slots, surface, backend)?,
        "frontend" => gpu_v2::frontend::vliw::verilog(portable),
        _ => return Err("unknown VLIW profile".into()),
    };
    let path = std::path::Path::new(&args[1]);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, source)?;
    println!("Exported {} to {}", args[2], path.display());
    Ok(())
}
