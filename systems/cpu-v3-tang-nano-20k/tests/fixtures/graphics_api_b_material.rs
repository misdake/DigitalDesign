use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_scene_math;
use gpu_scene_math::*;
mod gpu_scene_objects;
use gpu_scene_objects::*;

static SLOTS: Buf<u16, 48> = Buf::new([0xbeef; 48]);
static STATE: Buf<u16, 16> = Buf::new([0; 16]);
static NATIVE: Buf<u16, 256> = Buf::new([0xaced; 256]);

fn main() {
    let material = unsafe { SLOTS.as_ptr().add(16).as_u16_array() };
    let desc: MaterialDesc = MaterialDesc {
        uniform_color: Rgba8 {
            r: 18,
            g: 52,
            b: 86,
            a: 0,
        },
        specular: Rgb8 {
            r: 154,
            g: 188,
            b: 222,
        },
        ambient_response: fix32::from_words(128, 0),
        directional_response: fix32::from_words(32768, 0),
        shininess: Shininess::S28,
        unlit: false,
    };
    if !gpu_scene_material_init(material, desc) {
        halt(1);
    }
    if material[0u16] != 0x3412
        || material[1u16] != 0x0056
        || material[2u16] != 0xbc9a
        || material[3u16] != 0x00de
        || material[4u16] != 128
        || material[5u16] != 0
        || material[6u16] != 32768
        || material[7u16] != 0
        || material[8u16] != 11
        || material[9u16] != 0
    {
        halt(2);
    }
    let mut at: u16 = 10;
    while at < 16 {
        if material[at] != 0 || SLOTS[at] != 0xbeef || SLOTS[at + 32] != 0xbeef {
            halt(3);
        }
        at += 1;
    }
    let captured: MaterialDesc = gpu_scene_material_desc(material);
    gpu_scene_material_color(
        material,
        Rgba8 {
            r: 255,
            g: 0,
            b: 1,
            a: 255,
        },
    );
    gpu_scene_material_specular(material, Rgb8 { r: 1, g: 2, b: 3 }, Shininess::S64);
    gpu_scene_material_unlit(material, true);
    if captured.uniform_color.r != 18
        || captured.uniform_color.a != 0
        || captured.specular.b != 222
        || captured.shininess as u16 != 11
        || captured.unlit
        || material[0u16] != 255
        || material[1u16] != 0xff01
        || material[2u16] != 0x0201
        || material[3u16] != 3
        || material[8u16] != 16
        || material[9u16] != 1
    {
        halt(4);
    }
    if gpu_scene_shininess_nearest(13) as u16 != 6
        || gpu_scene_shininess_nearest(0) as u16 != 0
        || gpu_scene_shininess_nearest(65535) as u16 != 16
        || gpu_scene_shininess_exponent(Shininess::S56) != 56
    {
        halt(5);
    }
    gpu_scene_fix_write(
        STATE.as_array(),
        GPU_STATE_AMBIENT,
        fix32::from_words(49152, 0),
    );
    gpu_scene_fix_write(
        STATE.as_array(),
        GPU_STATE_DIRECTIONAL,
        fix32::from_words(49152, 0),
    );
    if !gpu_scene_light_response(STATE.as_array(), material, NATIVE.as_array()) {
        halt(6);
    }
    // Ka=1/512, Ia=3/4: product*256=3/8 rounds to zero. Pre-quantizing Ka
    // would produce one instead. Kd=1/2, Id=3/4 yields exactly 96.
    if NATIVE[8u16] != 0 || NATIVE[9u16] != 96 {
        halt(7);
    }
    gpu_scene_fix_write(STATE.as_array(), GPU_STATE_AMBIENT, fix32::from_int(600));
    if gpu_scene_light_response(STATE.as_array(), material, NATIVE.as_array())
        || NATIVE[8u16] != 0
        || NATIVE[9u16] != 96
    {
        halt(8);
    }
    #[cfg(gpu_debug)]
    {
        if gpu_scene_material_response(material, -fix32::from_int(1), fix32::zero())
            || material[4u16] != 128
            || material[6u16] != 32768
        {
            halt(9);
        }
    }
    at = 0;
    while at < 256 {
        if at != 8 && at != 9 && NATIVE[at] != 0xaced {
            halt(10);
        }
        at += 1;
    }
    halt(0x600d);
}
