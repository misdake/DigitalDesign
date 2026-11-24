//! A captured explosion replayed on an orbitable inspection stage.
use crate::dsl_rt::*;
mod boot_resources;
mod gamepad;
mod gpu_api;
mod gpu_board;
mod stream;
mod time;
use stream::*;
mod static_buffer;
use static_buffer::StaticBuffer;

#[static_buffer]
const SPRITES: StaticBuffer = StaticBuffer::new(SCENE_SPRITE_STORAGE_BYTES as usize, 128);
use boot_resources::*;
use gamepad::*;
use gpu_api::*;
use gpu_board::*;
use time::*;

const BUTTON_LEFT: u16 = 1 << 7;
const BUTTON_RIGHT: u16 = 1 << 5;
const BUTTON_TRIANGLE: u16 = 1 << 12;
const BUTTON_CROSS: u16 = 1 << 14;
const BUTTON_SQUARE: u16 = 1 << 15;
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
    grass: TextureViewPtr,
    road: TextureViewPtr,
    rock: TextureViewPtr,
}

#[derive(Copy, Clone)]
struct LayerViews {
    crown: TextureViewPtr,
    trunk: TextureViewPtr,
    flash: TextureViewPtr,
    smoke: TextureViewPtr,
}

struct LayerDraw {
    mesh: MeshPtr,
    sprites: MeshPtr,
    model: ModelPtr,
    material: MaterialPtr,
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
    mesh: MeshPtr,
    model: ModelPtr,
    material: &mut Material,
    views: &StageViews,
) {
    let grass = mesh.slice(SCENE_GRASS_FIRST, SCENE_GRASS_COUNT);
    let road = mesh.slice(SCENE_ROAD_FIRST, SCENE_ROAD_COUNT);
    let rock = mesh.slice(SCENE_ROCK_FIRST, SCENE_ROCK_COUNT);
    material.set_uniform_color(gray(230, 255));
    pass.draw(grass, model, material.ptr(), views.grass);
    pass.draw(road, model, material.ptr(), views.road);
    material.set_uniform_color(gray(204, 255));
    pass.draw(rock, model, material.ptr(), views.rock);
}

fn draw_layer(pass: &mut Pass, draw: &LayerDraw, layer: u16) {
    let mesh = draw.mesh;
    let sprites = draw.sprites;
    let model = draw.model;
    let material = draw.material;
    let views = draw.views;
    let layers = draw.layers;
    let entry = layers[layer * 2 + 1];
    let kind = entry >> 8;
    let level = entry as u8;
    let index = layers[layer * 2];
    let mut part = sprites;
    if index & 32768 != 0 {
        part = mesh.slice(index & 32767, 1);
    } else {
        part = sprites.slice(index, 1);
    }
    if kind == 0 {
        material.set_uniform_color(gray(level, 255));
        pass.draw_billboard(
            part,
            model,
            material,
            views.crown,
            BillboardFacing::Vertical,
        );
    } else if kind == 1 {
        material.set_uniform_color(gray(level, 255));
        pass.draw_billboard(
            part,
            model,
            material,
            views.trunk,
            BillboardFacing::Vertical,
        );
    } else {
        material.set_uniform_color(gray(255, level));
        let mut texture = views.flash;
        if kind == 3 {
            texture = views.smoke;
        }
        pass.draw_billboard(part, model, material, texture, BillboardFacing::CameraPlane);
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
    let model = Model::new(Transform::identity());
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
        grass: grass_view.ptr(),
        road: road_view.ptr(),
        rock: rock_view.ptr(),
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
        mesh: mesh.ptr(),
        sprites: sprite_owner.ptr(),
        model: model.ptr(),
        material: material.ptr(),
        views: LayerViews {
            crown: crown_view.ptr(),
            trunk: trunk_view.ptr(),
            flash: flash_view.ptr(),
            smoke: smoke_view.ptr(),
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
        if input.pressed & BUTTON_CROSS != 0 {
            age = 0;
            frame = 0;
            paused = false;
        } else if input.pressed & BUTTON_LEFT != 0 && frame > 0 {
            frame -= 1;
            age = frame * SCENE_TICK_MS;
            paused = true;
        } else if input.pressed & BUTTON_RIGHT != 0 && frame < last {
            frame += 1;
            age = frame * SCENE_TICK_MS;
            paused = true;
        } else if input.pressed & BUTTON_SQUARE != 0 {
            paused = !paused;
        }
        if input.pressed & BUTTON_TRIANGLE != 0 {
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
        let mut color = color_b.ptr();
        if odd {
            color = color_a.ptr();
        }
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
                layer_draw.mesh,
                model.ptr(),
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
