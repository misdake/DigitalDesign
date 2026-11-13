//! One real frame using only producer descriptions and static working storage.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod surface;
use surface::*;
mod boot_resources;
use boot_resources::*;

#[static_buffer]
const COLOR: Rgb565Surface = Rgb565Surface::allocate();
#[static_buffer]
const DEPTH: DepthSurface = DepthSurface::allocate();
#[static_buffer]
const COMMANDS: CommandMemory = CommandMemory::allocate(4096, 4096);
#[static_buffer]
const GRID: UiGridStorage = UiGridStorage::allocate();

const POSE: Transform = Transform::translation(Vec3::new(fix!(0), fix!(0), fix!(-3)));
const WORLD: Mat3x4 = POSE.world_matrix();
const NORMAL: Mat3x3 = POSE.normal_matrix();
const PROJECTION: Projection = Projection::from_focal_pixels(fix!(200), fix!(199));

fn main() {
    let mut gpu = Gpu::init(GpuConfig::single(COMMANDS));
    let mut commands = gpu.command_buffer();
    let color = ColorBuffer::from_surface(COLOR);
    let depth = DepthBuffer::from_surface(DEPTH);
    let mesh = Mesh::from_native(TRIANGLE);
    let texture = Texture::from_native(CHECKER);
    let mut view = texture.view();
    view.set_address_mode(TextureAddressMode::ClampToEdge);
    let model = Model::from_matrices(WORLD, NORMAL);
    let camera = Camera::new(CameraDesc::new(Vec3::zero(), Vec3::zero(), PROJECTION));
    let material = Material::new(MaterialDesc::unlit(Rgba8::white()));
    let state = RenderState::new(RenderStateDesc::opaque(Lighting::ambient(fix!(1))));
    let atlas = UiAtlas::from_desc(FONT);
    let mut grid = UiGrid::from_storage(GRID, &atlas);
    grid.fill(255);
    grid.write_text(2, 1, b"SDK");
    {
        let mut pass = commands.pass(
            &color,
            &depth,
            ColorLoad::clear(Rgb565::rgb(8, 8, 24)),
            DepthLoad::clear_far(),
        );
        pass.set_camera(&camera);
        pass.set_state(&state);
        pass.draw(&mesh, &model, &material, &view);
        pass.draw_ui(&grid, CellRect::full_grid());
    }
    gpu.submit(commands);
    commands = gpu.wait();
    let mut display = Display::open();
    display.set_next_swap(&color);
    display.wait_vblank();
    halt(0x600d);
}
