//! A captured explosion replayed on an orbitable inspection stage.
use crate::dsl_rt::*;
mod billboard;
mod boot_resources;
mod gamepad;
mod gpu_api;
mod gpu_board;
mod static_buffer;
mod stream;
mod time;
use crate::boot_resources::{
    SCENE_FLASH, SCENE_FRAME_COUNT, SCENE_GRASS, SCENE_GRASS_COUNT, SCENE_GRASS_FIRST, SCENE_MESH,
    SCENE_NTREE1, SCENE_NTREE2, SCENE_ROAD, SCENE_ROAD_COUNT, SCENE_ROAD_FIRST, SCENE_ROCK,
    SCENE_ROCK_COUNT, SCENE_ROCK_FIRST, SCENE_SMOKE, SCENE_SPRITE_STORAGE_BYTES, SCENE_TICK_MS,
};
use crate::gamepad::{
    gamepad_capture, gamepad_init, gamepad_take_frame, DS2_CROSS, DS2_LEFT, DS2_RIGHT, DS2_SQUARE,
    DS2_TRIANGLE,
};
use crate::gpu_api::{
    BillboardFacing, Camera, ColorBufferPtr, ColorLoad, DepthLoad, Display, Gpu, Material,
    MaterialPtr, Mesh, MeshPtr, Pass, RenderState, Rgb565, Texture, TextureView, Transform,
    TransformPtr,
};
use crate::gpu_board::{board_color_a, board_color_b, board_depth, board_gpu_config};
use crate::static_buffer::StaticBuffer;
use crate::stream::{sprite_writer, SpriteStream};
use crate::time::time_milliseconds;

#[static_buffer]
const SPRITES: StaticBuffer = StaticBuffer::new(SCENE_SPRITE_STORAGE_BYTES as usize, 128);

const LOOP_MS: u16 = SCENE_FRAME_COUNT * SCENE_TICK_MS + 800;
const SKY: Rgb565 = Rgb565::rgb(169, 182, 181);

struct Orbit {
    yaw: fix32,
    pitch: fix32,
    distance: fix32,
}

fn initial_orbit() -> Orbit {
    Orbit {
        yaw: fix!(-0.43633),
        pitch: fix!(0.38397),
        distance: fix!(3.125),
    }
}

fn clamp(value: fix32, lower: fix32, upper: fix32) -> fix32 {
    if value < lower {
        return lower;
    }
    if value > upper {
        return upper;
    }
    value
}

fn orbit_direction(orbit: &Orbit) -> Vec3 {
    let azimuth = fsincos(orbit.yaw);
    let elevation = fsincos(orbit.pitch);
    Vec3::new(
        azimuth.x() * elevation.y(),
        elevation.x(),
        azimuth.y() * elevation.y(),
    )
}

fn orbit_camera(camera: &mut Camera, orbit: &Orbit, direction: Vec3) {
    let target_y = fix!(0.525);
    let eye = Vec3::new(
        direction.x * orbit.distance,
        direction.y * orbit.distance + target_y,
        direction.z * orbit.distance,
    );
    camera.set_look_at_y_up(eye, Vec3::new(fix32::zero(), target_y, fix32::zero()));
}

fn clamp_view(texture: &Texture) -> TextureView {
    let mut view = texture.view();
    view.set_address_mode(TextureAddressMode::ClampToEdge);
    view
}

struct StageViews {
    grass: TextureView,
    road: TextureView,
    rock: TextureView,
}

struct LayerViews {
    crown: TextureView,
    trunk: TextureView,
    flash: TextureView,
    smoke: TextureView,
}

struct LayerDraw {
    transform: TransformPtr,
    mesh: MeshPtr,
    material: MaterialPtr,
    sprites: MeshPtr,
    views: LayerViews,
    layers: Array<u16>,
}

fn gray(level: u8, alpha: u8) -> Rgba8 {
    Rgba8 {
        r: level,
        g: level,
        b: level,
        a: alpha,
    }
}

fn draw_stage(
    pass: &mut Pass,
    transform: TransformPtr,
    mesh: MeshPtr,
    material: &mut Material,
    views: &StageViews,
) {
    let grass = mesh.slice(SCENE_GRASS_FIRST, SCENE_GRASS_COUNT);
    let road = mesh.slice(SCENE_ROAD_FIRST, SCENE_ROAD_COUNT);
    let rock = mesh.slice(SCENE_ROCK_FIRST, SCENE_ROCK_COUNT);
    material.set_uniform_color(gray(230, 255));
    pass.draw(transform, grass, material.ptr(), &views.grass);
    pass.draw(transform, road, material.ptr(), &views.road);
    material.set_uniform_color(gray(204, 255));
    pass.draw(transform, rock, material.ptr(), &views.rock);
}

fn draw_layer(pass: &mut Pass, draw: &LayerDraw, layer: u16) {
    let mesh = draw.mesh;
    let sprites = draw.sprites;
    let transform = draw.transform;
    let material = draw.material;
    let views = &draw.views;
    let layers = draw.layers;
    let entry = layers[layer * 2 + 1];
    let kind = entry >> 8;
    let level = entry as u8;
    let index = layers[layer * 2];
    let part: MeshPtr = if index & 32768 != 0 {
        mesh.slice(index & 32767, 1)
    } else {
        sprites.slice(index, 1)
    };
    if kind == 0 {
        material.set_uniform_color(gray(level, 255));
        pass.draw_billboard(
            transform,
            part,
            material,
            &views.crown,
            BillboardFacing::Vertical,
        );
    } else if kind == 1 {
        material.set_uniform_color(gray(level, 255));
        pass.draw_billboard(
            transform,
            part,
            material,
            &views.trunk,
            BillboardFacing::Vertical,
        );
    } else {
        material.set_uniform_color(gray(255, level));
        let texture: &TextureView = if kind == 3 {
            &views.smoke
        } else {
            &views.flash
        };
        pass.draw_billboard(
            transform,
            part,
            material,
            texture,
            BillboardFacing::CameraPlane,
        );
    }
}

fn draw_layers(pass: &mut Pass, draw: &LayerDraw, count: u16) {
    let mut at: u16 = 0;
    while at < count {
        draw_layer(pass, draw, at);
        at += 1;
    }
}

fn stage_desc() -> RenderStateDesc {
    RenderStateDesc::opaque(Lighting::directional(
        Vec3::unit_y(),
        fix32::one(),
        fix32::zero(),
    ))
    .with_filter(TextureFilter::Bilinear)
}

fn main() {
    let mut gpu = Gpu::init(board_gpu_config());
    let mut buffer = gpu.command_buffer();
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open();
    let mut camera = Camera::new(CameraDesc::new(
        Vec3::zero(),
        Vec3::zero(),
        Projection::from_focal_pixels(fix!(256.0), fix!(20.0)),
    ));
    let model = Transform::new(Pose::identity());
    let mesh = Mesh::from_native(SCENE_MESH);
    let mut writer = sprite_writer(GpuMemory::from_buffer(SPRITES));
    let mut sprite_owner = Mesh::from_native(writer.mesh_desc());
    let grass = Texture::from_native(SCENE_GRASS);
    let road = Texture::from_native(SCENE_ROAD);
    let rock = Texture::from_native(SCENE_ROCK);
    let crown = Texture::from_native(SCENE_NTREE1);
    let trunk = Texture::from_native(SCENE_NTREE2);
    let flash = Texture::from_native(SCENE_FLASH);
    let smoke = Texture::from_native(SCENE_SMOKE);
    let grass_view = clamp_view(&grass);
    let road_view = clamp_view(&road);
    let rock_view = clamp_view(&rock);
    let crown_view = clamp_view(&crown);
    let trunk_view = clamp_view(&trunk);
    let flash_view = clamp_view(&flash);
    let smoke_view = clamp_view(&smoke);
    let stage_views = StageViews {
        grass: grass_view,
        road: road_view,
        rock: rock_view,
    };
    let stage_state = RenderState::new(stage_desc());
    let layer_state = RenderState::new(
        stage_desc()
            .with_depth(DepthFunc::LessEqual, false)
            .with_blend(BlendMode::SrcOver),
    );
    let mut material = Material::new(
        MaterialDesc::lit(Rgba8::white(), Rgb8::black(), Shininess::S4).with_unlit(true),
    );
    let mut stream = SpriteStream::new();
    let layer_draw = LayerDraw {
        transform: model.ptr(),
        mesh: mesh.ptr(),
        material: material.ptr(),
        sprites: sprite_owner.ptr(),
        views: LayerViews {
            crown: crown_view,
            trunk: trunk_view,
            flash: flash_view,
            smoke: smoke_view,
        },
        layers: stream.plan(),
    };
    gamepad_init();
    let mut orbit = initial_orbit();
    let mut direction = orbit_direction(&orbit);
    orbit_camera(&mut camera, &orbit, direction);
    let mut paused = false;
    let mut previous = time_milliseconds();
    let mut age: u16 = 0;
    let mut odd = false;
    let mut first = true;
    loop {
        gamepad_capture();
        let input = gamepad_take_frame();
        let now = time_milliseconds();
        let mut elapsed = now - previous;
        previous = now;
        if elapsed > 120 {
            elapsed = 120;
        }
        if !paused {
            age += elapsed;
            if age >= LOOP_MS {
                age -= LOOP_MS;
            }
        }
        let last = SCENE_FRAME_COUNT - 1;
        let mut frame = age / SCENE_TICK_MS;
        if frame > last {
            frame = last;
        }
        if input.pressed & DS2_CROSS != 0 {
            age = 0;
            frame = 0;
            paused = false;
        } else if input.pressed & DS2_LEFT != 0 && frame > 0 {
            frame -= 1;
            age = frame * SCENE_TICK_MS;
            paused = true;
        } else if input.pressed & DS2_RIGHT != 0 && frame < last {
            frame += 1;
            age = frame * SCENE_TICK_MS;
            paused = true;
        } else if input.pressed & DS2_SQUARE != 0 {
            paused = !paused;
        }
        if input.pressed & DS2_TRIANGLE != 0 {
            orbit = initial_orbit();
            direction = orbit_direction(&orbit);
            orbit_camera(&mut camera, &orbit, direction);
        } else if input.left_x != fix32::zero()
            || input.left_y != fix32::zero()
            || input.right_y != fix32::zero()
        {
            let dt = fix32::from_int(elapsed as i16) * fix!(0.001);
            orbit.yaw = orbit.yaw + input.left_x * dt * fix!(1.8);
            if orbit.yaw > fix!(3.14159) {
                orbit.yaw = orbit.yaw - fix!(6.28319);
            }
            if orbit.yaw < fix!(-3.14159) {
                orbit.yaw = orbit.yaw + fix!(6.28319);
            }
            orbit.pitch = clamp(orbit.pitch - input.left_y * dt, fix!(0.08), fix!(1.16));
            orbit.distance = clamp(orbit.distance + input.right_y * dt, fix!(2.4), fix!(5.5));
            direction = orbit_direction(&orbit);
            orbit_camera(&mut camera, &orbit, direction);
        }

        buffer.reset();
        let batch_count = stream.prepare(&mut writer, &mut sprite_owner, frame, direction);
        if first {
            buffer.invalidate_textures();
            first = false;
        }
        let color: ColorBufferPtr = if odd { color_a.ptr() } else { color_b.ptr() };
        {
            let mut pass = buffer.pass(
                color,
                depth.ptr(),
                ColorLoad::clear(SKY),
                DepthLoad::clear(65535),
            );
            pass.set_camera(camera.ptr());
            pass.set_state(stage_state.ptr());
            draw_stage(
                &mut pass,
                model.ptr(),
                layer_draw.mesh,
                &mut material,
                &stage_views,
            );
            pass.set_state(layer_state.ptr());
            draw_layers(&mut pass, &layer_draw, batch_count);
        }
        gpu.submit(buffer);
        buffer = gpu.wait();
        display.set_next_swap(color);
        display.wait_vblank();
        odd = !odd;
    }
}
