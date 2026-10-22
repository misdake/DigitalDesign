//! Explicit 32-bit command stream; all input storage is the existing scratchpad.
use crate::{framebuffer::ports::MaterializedSurface, texture::ports::Slot};
#[path = "current_state.rs"]
mod current_state;
#[path = "current_texture.rs"]
mod current_texture;
#[path = "simulation_checks.rs"]
mod simulation_checks;
pub const MODULE: &str = "gpu_v2_vliw_command";
pub fn verilog(
    surface: MaterializedSurface,
    backend: super::pixel::quad_ingress::rtl::Backend,
) -> Result<String, String> {
    let mut source = "`define GPU_V2_COMMAND_PORTS\n".to_owned();
    source.push_str(include_str!("vliw_command.v"));
    // Generic numerical exporters also serve the fixed-resource leaf profiles.
    // Specialize their selected closure completely; no Slot table reaches RTL.
    let numerical = super::vliw_sdram::compute_verilog(
        &[Slot {
            base_address: 0,
            max_size_log2: 0,
            has_full_mip: false,
            valid: true,
        }],
        surface,
        backend,
    )?;
    source.push_str(&current_state::lower(current_texture::lower(numerical)?)?);
    source.push_str(include_str!("command_processor.v"));
    source.push_str(include_str!("command_dma.v"));
    let router = super::vliw_memory::RTL
        .split("module gpu_vliw_memory #")
        .next()
        .ok_or("memory router")?;
    source.push_str(router);
    simulation_checks::lower(source)
}

/// Current square RGB565 texture. Address and field ranges are driver contracts;
/// installation performs no descriptor validation or implicit invalidation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureDescriptor {
    pub base_sector: u16,
    pub size_log2: u8,
    pub full_mip: bool,
    pub filter: u8,
}
impl TextureDescriptor {
    pub const fn word(self) -> u32 {
        self.base_sector as u32
            | ((self.size_log2 as u32 & 15) << 16)
            | ((self.full_mip as u32) << 20)
            | ((self.filter as u32 & 3) << 21)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command(pub u32);
impl Command {
    pub const NOP: Self = Self(0);
    pub fn dma_local(
        source: u32,
        destination_qword: u16,
        bytes: u32,
    ) -> Result<Self, &'static str> {
        let size = match bytes {
            32 => 0,
            64 => 1,
            128 => 2,
            _ => return Err("copy size"),
        };
        let end = source.checked_add(bytes).ok_or("source overflow")?;
        let dst = destination_qword as u32 / 4;
        if !source.is_multiple_of(bytes)
            || end > 0x800000
            || !destination_qword.is_multiple_of(4)
            || !((dst >= 66 && dst + bytes / 32 <= 80)
                || ((80..=92).contains(&dst) && dst.is_multiple_of(4) && bytes == 128))
        {
            return Err("copy range");
        }
        Ok(Self(
            (1 << 28) | ((source / 32) << 10) | (dst << 3) | (size << 1),
        ))
    }
    pub fn dma_meshlet(record: u32, slot: u8, payload_bytes: u32) -> Result<Self, &'static str> {
        if slot > 1
            || !record.is_multiple_of(128)
            || payload_bytes == 0
            || payload_bytes > 1024
            || !payload_bytes.is_multiple_of(128)
            || record
                .checked_add(128 + payload_bytes)
                .is_none_or(|end| end > 0x800000)
        {
            return Err("meshlet DMA range");
        }
        Ok(Self(
            (2 << 28) | ((record / 128) << 12) | ((payload_bytes / 128 - 1) << 1) | slot as u32,
        ))
    }
    pub fn draw_meshlet(slot: u8) -> Result<Self, &'static str> {
        if slot > 1 {
            return Err("input slot");
        }
        Ok(Self((3 << 28) | slot as u32))
    }
    pub const fn wait(dma: bool, all: bool) -> Self {
        Self((4 << 28) | (dma as u32) | ((all as u32) << 1))
    }
    pub fn set_global(qword: u16) -> Result<Self, &'static str> {
        Self::set(5, qword, 64)
    }
    pub fn set_render(qword: u16) -> Result<Self, &'static str> {
        Self::set(6, qword, 32)
    }
    fn set(op: u32, qword: u16, bytes: u32) -> Result<Self, &'static str> {
        let dst = qword as u32 / 4;
        if !qword.is_multiple_of(4) || dst < 66 || dst + bytes / 32 > 80 {
            return Err("state slot");
        }
        Ok(Self((op << 28) | dst))
    }
    pub const FLUSH: Self = Self(7 << 28);
    /// Only after WAIT(all); independent of SET_RENDER and framebuffer FLUSH.
    pub const TEX_INVALIDATE: Self = Self(9 << 28);
    /// Clear the installed color (false) or depth (true) plane. No maintenance.
    pub const fn clear(depth: bool, value: u16) -> Self {
        Self((8 << 28) | ((depth as u32) << 16) | value as u32)
    }
    pub fn set_target(qword: u16) -> Result<Self, &'static str> {
        Self::set(10, qword, 32)
    }
    pub const END: Self = Self(15 << 28);
    pub const fn encode(self) -> [u8; 4] {
        self.0.to_le_bytes()
    }
}
/// Driver-validated two-SEND descriptor. All addresses here are bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceSubmission {
    pub count_minus_one: u16,
    pub base_sector: u16,
}
impl DeviceSubmission {
    pub fn new(base_bytes: u32, valid_bytes: u32) -> Result<Self, &'static str> {
        if !base_bytes.is_multiple_of(128)
            || valid_bytes == 0
            || valid_bytes > 65536
            || !valid_bytes.is_multiple_of(4)
            || base_bytes
                .checked_add(valid_bytes.div_ceil(128) * 128)
                .is_none_or(|end| end > 0x800000)
        {
            return Err("command buffer extent/alignment");
        }
        Ok(Self {
            count_minus_one: (valid_bytes / 4 - 1) as u16,
            base_sector: (base_bytes / 128) as u16,
        })
    }
}
/// Runtime plane addresses. Cache maintenance is an explicit driver operation.
pub fn target_params(surface: MaterializedSurface) -> Result<[u8; 32], String> {
    surface.validate()?;
    if surface.width != 400 || surface.height != 240 {
        return Err("Command target requires400x240".into());
    }
    if !surface.color_base_bytes.is_multiple_of(65536)
        || !surface.depth_base_bytes.is_multiple_of(65536)
    {
        return Err("Command target requires64KiB alignment".into());
    }
    if u64::from(surface.color_base_bytes) + 192000 > 0x800000
        || u64::from(surface.depth_base_bytes) + 192000 > 0x800000
    {
        return Err("Command target exceeds8MiB".into());
    }
    let word =
        (surface.color_base_bytes as u64 >> 9) | ((surface.depth_base_bytes as u64 >> 9) << 14);
    let mut bytes = [0; 32];
    bytes[..8].copy_from_slice(&word.to_le_bytes());
    Ok(bytes)
}
/// Instance128 words22/23: RGB modulation and an explicit per-mesh alpha.
/// None retains untinted source color and opaque alpha; only Mesh supplies alpha.
pub fn mesh_color_words(rgba: Option<[u8; 4]>) -> [u32; 2] {
    [rgba.map_or(u32::MAX, u32::from_le_bytes), 0]
}
#[derive(Clone, Debug, Default)]
pub struct CommandBuffer {
    pub commands: Vec<Command>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandImage {
    pub valid_bytes: u32,
    pub storage: Vec<u8>,
}
impl CommandBuffer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, command: Command) {
        self.commands.push(command)
    }
    pub fn image(&self) -> Result<CommandImage, &'static str> {
        if self.commands.is_empty()
            || self.commands.len() > 16384
            || self.commands.last() != Some(&Command::END)
        {
            return Err("command extent/END");
        }
        let mut storage: Vec<u8> = self.commands.iter().flat_map(|c| c.encode()).collect();
        let valid_bytes = storage.len() as u32;
        storage.resize(storage.len().div_ceil(128) * 128, 0);
        Ok(CommandImage {
            valid_bytes,
            storage,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_submission_boundaries() {
        assert_eq!(
            DeviceSubmission::new(0x40000, 372).unwrap(),
            DeviceSubmission {
                count_minus_one: 92,
                base_sector: 0x800
            }
        );
        assert_eq!(
            DeviceSubmission::new(0x7f0000, 65536)
                .unwrap()
                .count_minus_one,
            16383
        );
        assert!(DeviceSubmission::new(0x7fff80, 4).is_ok());
        for (base, bytes) in [
            (0, 0),
            (0, 3),
            (1, 4),
            (0, 65540),
            (0x7fff80, 132),
            (u32::MAX, 4),
        ] {
            assert!(DeviceSubmission::new(base, bytes).is_err());
        }
    }
    #[test]
    fn compact_encoding_and_ranges() {
        assert_eq!(Command::TEX_INVALIDATE.0, 9 << 28);
        assert_eq!(
            TextureDescriptor {
                base_sector: 0x1234,
                size_log2: 6,
                full_mip: true,
                filter: 2
            }
            .word(),
            0x00561234
        );
        assert_eq!(
            Command::dma_local(0x50080, 272, 128).unwrap().0,
            0x10000000 | ((0x50080 / 32) << 10) | (68 << 3) | 4
        );
        assert!(Command::dma_local(0x50020, 272, 128).is_err());
        assert!(Command::dma_local(0x50020, 272, 64).is_err());
        assert!(Command::dma_local(0x50020, 272, 32).is_ok());
        assert!(Command::dma_local(0x50040, 272, 64).is_ok());
        assert!(Command::dma_local(0, 448, 128).is_err());
        assert!(Command::dma_local(0, 324, 128).is_err());
        assert!(Command::dma_meshlet(0x7fff80, 0, 128).is_err());
        assert_eq!(
            Command::dma_meshlet(0x240000, 1, 1024).unwrap().0,
            0x20000000 | ((0x240000 / 128) << 12) | 15
        );
        let mut cb = CommandBuffer::new();
        cb.push(Command::NOP);
        cb.push(Command::END);
        let image = cb.image().unwrap();
        assert_eq!(image.valid_bytes, 8);
        assert_eq!(image.storage.len(), 128);
    }
    #[test]
    fn plane_clear_encoding() {
        assert_eq!(Command::clear(false, 0x5a3c).0, 0x80005a3c);
        assert_eq!(Command::clear(true, 0xffff).0, 0x8001ffff);
    }
    #[test]
    fn runtime_target_uses_the_physical_window() {
        let mut surface = MaterializedSurface {
            color_base_bytes: 0x120000,
            depth_base_bytes: 0x180000,
            width: 400,
            height: 240,
        };
        assert!(target_params(surface).is_ok());
        surface.depth_base_bytes = 0x800000;
        assert!(surface.validate().is_ok()); // The generic API has a wider address domain.
        assert!(target_params(surface).is_err());
    }
    #[test]
    fn dependency_closed_no_extra_store() {
        let s = verilog(
            MaterializedSurface {
                color_base_bytes: 0x100000,
                depth_base_bytes: 0x140000,
                width: 400,
                height: 240,
            },
            super::super::pixel::quad_ingress::rtl::Backend::Portable,
        )
        .unwrap();
        assert!(!s.contains("module gpu_command_store"));
        assert!(!s.contains("module gpu_command_mesh("));
        assert!(!s.contains("module gpu_vliw_memory #"));
        assert!(!s.contains("module gpu_vliw_dma"));
        assert!(s.contains("module gpu_command_dma("));
        for legacy in [
            "slot_base_f",
            "slot_full_f",
            "slot_max_f",
            "slot_valid_f",
            "draw_config",
            "draw_bank",
            "loaded_bank",
            "COLOR_BASE",
            "DEPTH_BASE",
            "draw_rebind",
            "reg[31:0]far_z",
        ] {
            assert!(
                !s.contains(legacy),
                "old texture configuration survived: {legacy}"
            );
        }
        assert!(s.contains("if(reset||texture_invalidate)begin state_initialized<=0;"));
        assert!(s.contains("cfg_max_mip=texture_mip?{1'b0,texture_n}:5'd0"));
        assert!(s.contains("output[34:0]out_packet"));
        let cache = s
            .split("module gpu_v2_texture_runtime_cache_unique #")
            .nth(1)
            .unwrap()
            .split("endmodule")
            .next()
            .unwrap();
        assert!(cache.contains("head_tag={6'd0,head_sector[15:4]}"));
        assert!(!cache.contains("hk_row"));
        assert!(!cache.contains("layer_off_f"));
        assert!(!cache.contains("wire msx="));
        let light = s
            .split("module gpu_v2_lighting(")
            .nth(1)
            .unwrap()
            .split("endmodule")
            .next()
            .unwrap();
        assert!(!light.contains("configured"));
        assert!(!light.contains("context_valid"));
        assert!(!light.contains("context_ready"));
        assert!(!s.contains("L_CONTEXT"));
    }
}
