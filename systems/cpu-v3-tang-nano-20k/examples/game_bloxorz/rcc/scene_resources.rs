//! Scene resource bodies live for the run; rendering borrows them or passes Copy Ptrs.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;
mod game_assets;
use game_assets::*;
mod pad_cache;
use pad_cache::*;

pub struct SceneResources {
    pub camera: Camera,
    pub screen_camera: Camera,
    pub screen_state: RenderState,
    pub state: RenderState,
    pub hint_state: RenderState,
    pub block: Mesh,
    pub pad: Mesh,
    pub shadow_mesh: Mesh,
    pub shadow_model: Model,
    pub shadow_material: Material,
    pub shadow_state: RenderState,
    pub screen: Mesh,
    pub floor_model: Model,
    pub block_model: Model,
    pub idle_model: Model,
    pub idle_material: Material,
    pub pad_model: Model,
    pub screen_model: Model,
    pub material: Material,
    pub floor_material: Material,
    pub entry_hatch_material: Material,
    pub goal_hatch_material: Material,
    pub entry_ring_material: Material,
    pub goal_ring_material: Material,
    pub carrier_material: Material,
    pub entry_hatch_model: Model,
    pub goal_hatch_model: Model,
    pub carrier_model: Model,
    pub entry0: Model,
    pub entry1: Model,
    pub entry2: Model,
    pub entry3: Model,
    pub goal0: Model,
    pub goal1: Model,
    pub goal2: Model,
    pub goal3: Model,
}
impl SceneResources {
    pub fn new(camera: Camera) -> Self {
        let screen_camera = game_screen_camera();
        let screen_state = game_screen_state();
        let state = game_solid_state();
        let hint_state = game_hint_state();
        let block = game_mesh_part(BLOCK_FIRST, BLOCK_COUNT);
        let pad = game_mesh_part(PAD_FIRST, PAD_COUNT);
        let shadow_mesh = game_mesh_part(SHADOW_FIRST, SHADOW_COUNT);
        let shadow_model = game_plain_model();
        let shadow_material = game_shadow_material();
        let shadow_state = game_shadow_state();
        let screen = game_mesh_part(SCREEN_FIRST, SCREEN_COUNT);
        let floor_model = game_floor_model();
        let block_model = game_plain_model();
        let idle_model = game_plain_model();
        let idle_material = game_idle_material();
        let pad_model = game_plain_model();
        let screen_model = game_screen_model();
        let material = game_material(Rgba8::white(), false);
        let floor_material = game_floor_material();
        let entry_hatch_material = pad_part_material(0);
        let goal_hatch_material = pad_part_material(1);
        let entry_ring_material = pad_part_material(3);
        let goal_ring_material = pad_part_material(7);
        let carrier_material = pad_part_material(2);
        let entry_hatch_model = game_plain_model();
        let goal_hatch_model = game_plain_model();
        let carrier_model = game_plain_model();
        let entry0 = game_plain_model();
        let entry1 = game_plain_model();
        let entry2 = game_plain_model();
        let entry3 = game_plain_model();
        let goal0 = game_plain_model();
        let goal1 = game_plain_model();
        let goal2 = game_plain_model();
        let goal3 = game_plain_model();
        Self {
            camera: camera,
            screen_camera: screen_camera,
            screen_state: screen_state,
            state: state,
            hint_state: hint_state,
            block: block,
            pad: pad,
            shadow_mesh: shadow_mesh,
            shadow_model: shadow_model,
            shadow_material: shadow_material,
            shadow_state: shadow_state,
            screen: screen,
            floor_model: floor_model,
            block_model: block_model,
            idle_model: idle_model,
            idle_material: idle_material,
            pad_model: pad_model,
            screen_model: screen_model,
            material: material,
            floor_material: floor_material,
            entry_hatch_material: entry_hatch_material,
            goal_hatch_material: goal_hatch_material,
            entry_ring_material: entry_ring_material,
            goal_ring_material: goal_ring_material,
            carrier_material: carrier_material,
            entry_hatch_model: entry_hatch_model,
            goal_hatch_model: goal_hatch_model,
            carrier_model: carrier_model,
            entry0: entry0,
            entry1: entry1,
            entry2: entry2,
            entry3: entry3,
            goal0: goal0,
            goal1: goal1,
            goal2: goal2,
            goal3: goal3,
        }
    }
}

pub fn game_floor_model() -> Model {
    Model::new(Transform::uniform_scale(fix!(0.5)))
}

pub fn game_plain_model() -> Model {
    Model::new(Transform::identity())
}

pub fn game_screen_model() -> Model {
    let one = fix32::from_int(1);
    Model::new(Transform::new(
        Vec3::new(fix32::zero(), fix32::zero(), -one),
        Vec3::zero(),
        Vec3::new(one, fix!(0.6), one),
    ))
}

pub fn game_screen_state() -> RenderState {
    RenderState::new(
        RenderStateDesc::transparent(game_lighting(Vec3::unit_z()))
            .with_depth(DepthFunc::Always, false)
            .with_filter(TextureFilter::Nearest),
    )
}

// Preserve the established light quantization exactly.
const WORLD_LIGHT: Lighting = game_lighting(Vec3::new(
    fix32::from_words(16711, 0),
    fix32::from_words(33422, 0),
    fix32::from_words(16711, 0),
));

pub fn game_solid_state() -> RenderState {
    RenderState::new(RenderStateDesc::opaque(WORLD_LIGHT).with_filter(TextureFilter::Bilinear))
}

pub fn game_hint_state() -> RenderState {
    RenderState::new(RenderStateDesc::transparent(WORLD_LIGHT).with_filter(TextureFilter::Bilinear))
}

pub fn game_shadow_state() -> RenderState {
    RenderState::new(
        RenderStateDesc::transparent(game_lighting(Vec3::unit_z()))
            .with_depth(DepthFunc::Always, false)
            .with_filter(TextureFilter::Nearest),
    )
}

pub fn game_shadow_material() -> Material {
    return game_material(
        Rgba8 {
            r: 10,
            g: 13,
            b: 18,
            a: 87,
        },
        true,
    );
}

pub fn game_idle_material() -> Material {
    game_material(
        Rgba8 {
            r: 154,
            g: 147,
            b: 140,
            a: 255,
        },
        false,
    )
}

pub fn game_floor_material() -> Material {
    game_material(
        Rgba8 {
            r: 82,
            g: 100,
            b: 121,
            a: 255,
        },
        false,
    )
}
