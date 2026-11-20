//! A captured explosion replayed on an orbitable inspection stage.
use crate::dsl_rt::*;
mod boot_resources;
mod gamepad;
mod gpu_api;
mod gpu_board;
mod time;
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

fn scene_texture(sector: u16, sectors: u16, size: u16, format: TextureFormat) -> Texture {
    let memory = unsafe { GpuMemory::from_sectors(sector, sectors) };
    Texture::from_native(TextureDesc::from_memory(
        memory,
        TextureLayout::square(size, format, Mipmaps::BaseOnly),
    ))
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

struct LayerViews {
    crown: TextureViewPtr,
    trunk: TextureViewPtr,
    flash: TextureViewPtr,
    smoke: TextureViewPtr,
}

struct LayerFrame {
    direction: Vec3,
    index: u16,
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
    mesh: &Mesh,
    model: ModelPtr,
    material: &mut Material,
    views: &StageViews,
) {
    let grass = mesh.subset(SCENE_GRASS_FIRST, SCENE_GRASS_COUNT);
    let road = mesh.subset(SCENE_ROAD_FIRST, SCENE_ROAD_COUNT);
    let rock = mesh.subset(SCENE_ROCK_FIRST, SCENE_ROCK_COUNT);
    material.set_uniform_color(gray(230, 255));
    pass.draw(grass.ptr(), model, material.ptr(), views.grass);
    pass.draw(road.ptr(), model, material.ptr(), views.road);
    material.set_uniform_color(gray(204, 255));
    pass.draw(rock.ptr(), model, material.ptr(), views.rock);
}

fn q8_8(word: u16) -> fix32 {
    fix32::from_words(word << 8, ((word as i16) >> 8) as u16)
}

fn layer_depth(layer: u16, direction: &Vec3) -> i16 {
    let at = layer * 4;
    let depth = fdot(
        vec3::new(
            q8_8(SCENE_LAYERS[at + 1]),
            q8_8(SCENE_LAYERS[at + 2]),
            q8_8(SCENE_LAYERS[at + 3]),
        ),
        vec3::new(direction.x, direction.y, direction.z),
    );
    (depth * fix!(256.0)).to_int()
}

fn layer_index(slot: u16, first_effect: u16) -> u16 {
    if slot < 2 {
        return slot;
    }
    first_effect + (slot - 2)
}

fn sort_layer_order(mut order: Array<u16>, depths: Array<i16>, count: u16) {
    // Use Knuth's gaps, growing 1, 4, 13, ... until the next gap would exceed one third of the list.
    let mut gap: u16 = 1;
    while gap * 3 + 1 <= count / 3 {
        gap = gap * 3 + 1;
    }
    loop {
        let mut i = gap;
        while i < count {
            let slot = order[i];
            let depth = depths[slot];
            let mut at = i;
            while at >= gap {
                let previous = order[at - gap];
                let previous_depth = depths[previous];
                // Use the original slot to preserve input order at equal depths.
                if previous_depth < depth || (previous_depth == depth && previous < slot) {
                    break;
                }
                order[at] = previous;
                at -= gap;
            }
            order[at] = slot;
            i += 1;
        }
        if gap == 1 {
            break;
        }
        gap /= 3;
    }
}

fn draw_layer(
    pass: &mut Pass,
    mesh: &Mesh,
    model: ModelPtr,
    material: &mut Material,
    views: &LayerViews,
    layer: u16,
) {
    let entry = SCENE_LAYERS[layer * 4];
    let kind = entry >> 8;
    let level = entry as u8;
    let part = mesh.subset(SCENE_TRANSPARENT_FIRST + layer, 1);
    if kind == 0 {
        material.set_uniform_color(gray(level, 255));
        pass.draw_billboard(
            part.ptr(),
            model,
            material.ptr(),
            views.crown,
            BillboardFacing::Vertical,
        );
    } else if kind == 1 {
        material.set_uniform_color(gray(level, 255));
        pass.draw_billboard(
            part.ptr(),
            model,
            material.ptr(),
            views.trunk,
            BillboardFacing::Vertical,
        );
    } else {
        material.set_uniform_color(gray(255, level));
        let mut texture = views.flash;
        if kind == 3 {
            texture = views.smoke;
        }
        pass.draw_billboard(
            part.ptr(),
            model,
            material.ptr(),
            texture,
            BillboardFacing::CameraPlane,
        );
    }
}

fn draw_layers(
    pass: &mut Pass,
    mesh: &Mesh,
    model: ModelPtr,
    material: &mut Material,
    views: &LayerViews,
    frame: &LayerFrame,
) {
    // Two tree panels and at most ten recorded effect layers.
    let mut order: Buf<u16, 12> = Buf::new([0; 12]);
    let mut depths: Buf<i16, 12> = Buf::new([0; 12]);
    let first_effect = SCENE_FRAME_STARTS[frame.index];
    let count = 2 + (SCENE_FRAME_STARTS[frame.index + 1] - first_effect);
    let direction = frame.direction;
    let mut at: u16 = 0;
    while at < count {
        order[at] = at;
        depths[at] = layer_depth(layer_index(at, first_effect), &direction);
        at += 1;
    }
    sort_layer_order(order.as_array(), depths.as_array(), count);
    at = 0;
    while at < count {
        let layer = layer_index(order[at], first_effect);
        draw_layer(pass, mesh, model, material, views, layer);
        at += 1;
    }
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
    let mesh = Mesh::from_native(MeshDesc::from_memory(
        unsafe { GpuMemory::from_sectors(SCENE_MESH_SECTOR, SCENE_MESH_SECTORS) },
        0,
        SCENE_MESH_COUNT,
    ));
    let grass = scene_texture(
        SCENE_GRASS_SECTOR,
        SCENE_GRASS_SECTORS,
        SCENE_GRASS_SIZE,
        TextureFormat::Rgb565,
    );
    let road = scene_texture(
        SCENE_ROAD_SECTOR,
        SCENE_ROAD_SECTORS,
        SCENE_ROAD_SIZE,
        TextureFormat::Rgb565,
    );
    let rock = scene_texture(
        SCENE_ROCK_SECTOR,
        SCENE_ROCK_SECTORS,
        SCENE_ROCK_SIZE,
        TextureFormat::Rgb565,
    );
    let crown = scene_texture(
        SCENE_NTREE1_SECTOR,
        SCENE_NTREE1_SECTORS,
        SCENE_NTREE1_SIZE,
        TextureFormat::Rgba4444,
    );
    let trunk = scene_texture(
        SCENE_NTREE2_SECTOR,
        SCENE_NTREE2_SECTORS,
        SCENE_NTREE2_SIZE,
        TextureFormat::Rgba4444,
    );
    let flash = scene_texture(
        SCENE_FLASH_SECTOR,
        SCENE_FLASH_SECTORS,
        SCENE_FLASH_SIZE,
        TextureFormat::Rgba4444,
    );
    let smoke = scene_texture(
        SCENE_SMOKE_SECTOR,
        SCENE_SMOKE_SECTORS,
        SCENE_SMOKE_SIZE,
        TextureFormat::Rgba4444,
    );
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
    let layer_views = LayerViews {
        crown: crown_view.ptr(),
        trunk: trunk_view.ptr(),
        flash: flash_view.ptr(),
        smoke: smoke_view.ptr(),
    };
    let opaque = RenderStateDesc::opaque(Lighting::directional(
        Vec3::unit_y(),
        fix32::one(),
        fix32::zero(),
    ))
    .with_filter(TextureFilter::Bilinear);
    let stage_state = RenderState::new(opaque);
    let layer_state = RenderState::new(
        opaque
            .with_depth(DepthFunc::LessEqual, false)
            .with_blend(BlendMode::SrcOver),
    );
    let mut material = Material::new(
        MaterialDesc::lit(Rgba8::white(), Rgb8::black(), Shininess::S4).with_unlit(true),
    );
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
            draw_stage(&mut pass, &mesh, model.ptr(), &mut material, &stage_views);
            pass.set_state(layer_state.ptr());
            draw_layers(
                &mut pass,
                &mesh,
                model.ptr(),
                &mut material,
                &layer_views,
                &LayerFrame {
                    direction,
                    index: frame,
                },
            );
        }
        gpu.submit(buffer);
        buffer = gpu.wait();
        display.set_next_swap(color);
        display.wait_vblank();
        odd = !odd;
    }
}
