use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_scene_math;
use gpu_scene_math::*;
mod gpu_scene_objects;
use gpu_scene_objects::*;

static STATE: Buf<u16, 16> = Buf::new([0xbeef; 16]);
static MATERIAL: Buf<u16, 16> = Buf::new([0; 16]);
static TEXTURE: Buf<u16, 16> = Buf::new([0xaaaa; 16]);
static VIEW: Buf<u16, 16> = Buf::new([0; 16]);
static NATIVE: Buf<u16, 256> = Buf::new([0xaced; 256]);

fn main() {
    let state_desc: RenderStateDesc = RenderStateDesc {
        depth: DepthFunc::LessEqual,
        depth_write: true,
        blend: BlendMode::SrcOver,
        filter: TextureFilter::Bilinear,
        lighting: Lighting {
            direction: Vec3 {
                x: fix32::zero(),
                y: fix32::zero(),
                z: fix32::from_int(8),
            },
            ambient: fix32::from_words(49152, 0),
            directional: fix32::from_int(1),
        },
    };
    if !gpu_scene_state_init(STATE.as_array(), state_desc) {
        halt(1);
    }
    let tiny: Lighting = Lighting {
        direction: Vec3 {
            x: fix32::from_words(1, 0),
            y: fix32::zero(),
            z: fix32::zero(),
        },
        ambient: fix32::zero(),
        directional: fix32::zero(),
    };
    if !gpu_scene_state_lighting(STATE.as_array(), tiny)
        || gpu_scene_fix_read(STATE.as_array(), 0) != fix32::from_int(1)
    {
        halt(17);
    }
    let minimum: Lighting = Lighting {
        direction: Vec3 {
            x: fix32::from_words(0, 32768),
            y: fix32::zero(),
            z: fix32::zero(),
        },
        ambient: fix32::zero(),
        directional: fix32::zero(),
    };
    if !gpu_scene_state_lighting(STATE.as_array(), minimum)
        || gpu_scene_fix_read(STATE.as_array(), 0) != -fix32::from_int(1)
    {
        halt(18);
    }
    if !gpu_scene_state_lighting(STATE.as_array(), state_desc.lighting) {
        halt(19);
    }
    if STATE[10u16] != 27
        || STATE[11u16] != 1
        || gpu_scene_fix_read(STATE.as_array(), 4) != fix32::from_int(1)
    {
        halt(2);
    }
    let bad_light: Lighting = Lighting {
        direction: Vec3 {
            x: fix32::zero(),
            y: fix32::zero(),
            z: fix32::zero(),
        },
        ambient: fix32::zero(),
        directional: fix32::zero(),
    };
    if gpu_scene_state_lighting(STATE.as_array(), bad_light)
        || gpu_scene_fix_read(STATE.as_array(), 4) != fix32::from_int(1)
    {
        halt(3);
    }
    let mat: MaterialDesc = MaterialDesc {
        uniform_color: Rgba8 {
            r: 255,
            g: 255,
            b: 255,
            a: 0,
        },
        specular: Rgb8 { r: 1, g: 2, b: 3 },
        ambient_response: fix32::from_int(1),
        directional_response: fix32::from_int(1),
        shininess: Shininess::S4,
        unlit: false,
    };
    if !gpu_scene_material_init(MATERIAL.as_array(), mat) {
        halt(4);
    }
    let desc: TextureDesc = TextureDesc {
        format: TextureFormat::Rgb565,
        memory: GpuMemory {
            first_sector: 4096,
            sector_count: 36,
        },
        size_log2: 5,
        full_mip: true,
    };
    if !gpu_scene_texture_init(TEXTURE.as_array(), desc)
        || gpu_scene_view_width(TEXTURE.as_array()) != 32
    {
        halt(5);
    }
    gpu_scene_texture_view(VIEW.as_array(), TEXTURE.as_array());
    TEXTURE[0u16] = 8192;
    if VIEW[0u16] != 4096
        || VIEW[1u16] != 36
        || VIEW[2u16] != 5
        || VIEW[3u16] != 1
        || gpu_scene_view_height(VIEW.as_array()) != 32
    {
        halt(7);
    }
    let short: TextureDesc = TextureDesc {
        format: TextureFormat::Rgb565,
        memory: GpuMemory {
            first_sector: 4096,
            sector_count: 35,
        },
        size_log2: 5,
        full_mip: true,
    };
    if gpu_scene_texture_init(TEXTURE.as_array(), short) || TEXTURE[0u16] != 8192 {
        halt(8);
    }
    if !gpu_scene_render_prepare(
        STATE.as_array(),
        MATERIAL.as_array(),
        VIEW.as_array(),
        true,
        NATIVE.as_array(),
    ) || NATIVE[32u16] != 4096
        || NATIVE[33u16] != 53
        || NATIVE[34u16] != 0x0201
        || NATIVE[35u16] != 3
        || NATIVE[36u16] != 27
    {
        halt(9);
    }
    let plane: GpuMemory = GpuMemory {
        first_sector: 63488,
        sector_count: 2048,
    };
    if !gpu_scene_color_view(VIEW.as_array(), plane)
        || gpu_scene_view_width(VIEW.as_array()) != 400
        || gpu_scene_view_height(VIEW.as_array()) != 240
        || VIEW[1u16] != 2048
    {
        halt(10);
    }
    gpu_scene_material_unlit(MATERIAL.as_array(), true);
    if !gpu_scene_render_prepare(
        STATE.as_array(),
        MATERIAL.as_array(),
        VIEW.as_array(),
        true,
        NATIVE.as_array(),
    ) || NATIVE[33u16] != 169
        || NATIVE[36u16] != 59
    {
        halt(11);
    }
    gpu_scene_state_filter(STATE.as_array(), TextureFilter::Trilinear);
    if gpu_scene_render_prepare(
        STATE.as_array(),
        MATERIAL.as_array(),
        VIEW.as_array(),
        true,
        NATIVE.as_array(),
    ) || NATIVE[33u16] != 169
    {
        halt(12);
    }
    gpu_scene_state_depth(STATE.as_array(), DepthFunc::Always, false);
    gpu_scene_state_blend(STATE.as_array(), BlendMode::Replace);
    // No texture branch may inspect this deliberately invalid view pointer.
    let absent = unsafe { Ptr::from_addr(0xffff).as_u16_array() };
    if !gpu_scene_render_prepare(
        STATE.as_array(),
        MATERIAL.as_array(),
        absent,
        false,
        NATIVE.as_array(),
    ) || NATIVE[32u16] != 0
        || NATIVE[33u16] != 0
        || NATIVE[36u16] != 39
    {
        halt(13);
    }
    let mut at: u16 = 0;
    while at < 256 {
        if (at < 32 || at >= 48) && NATIVE[at] != 0xaced {
            halt(14);
        }
        if at >= 37 && at < 48 && NATIVE[at] != 0 {
            halt(15);
        }
        at += 1;
    }
    at = 12;
    while at < 16 {
        if STATE[at] != 0 || VIEW[at] != 0 {
            halt(16);
        }
        at += 1;
    }
    halt(0x600d);
}
