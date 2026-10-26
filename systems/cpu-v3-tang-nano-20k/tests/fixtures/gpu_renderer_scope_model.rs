//! Test-only resource/use model. This is not the isolated production facade.
//! Native helper privacy requires the pending host-approved library capability.
use crate::dsl_rt::*;
mod gpu_pipeline;
use gpu_pipeline::*;

#[rcc_resource(cleanup = renderer_cleanup)]
pub struct Renderer {
    word: u16,
}
#[rcc_resource(cleanup = frame_cleanup, parent = Renderer)]
pub struct Frame {
    word: u16,
}
#[rcc_resource(cleanup = pass_cleanup, parent = Frame)]
pub struct Pass {
    word: u16,
}
// 0 new,1 Renderer,2 Frame,3 Pass,4 closed,5 frozen; pass 0 new/2 ended/3 cancelled.
static GPU_SCOPE_MODEL: Buf<u16, 2> = Buf::new([0; 2]);

fn renderer_model_fault(code: u16) -> ! {
    GPU_SCOPE_MODEL[0u16] = 5;
    gpu_pipeline_fail(code);
    halt(0xe000 + code);
}
pub fn renderer_open() -> Renderer {
    let status = gpu_pipeline_init(DISPLAY_LINEAR_RGB565);
    if status != GPU_IDLE && status != GPU_PENDING {
        renderer_model_fault(20);
    }
    let mut atlas: GpuUiAtlas = GpuUiAtlas {
        sector: 0x9800,
        sectors: 512,
    };
    if !gpu_persistent_ui_register(view_of(&mut atlas)) {
        renderer_model_fault(21);
    }
    GPU_SCOPE_MODEL[0u16] = 1;
    Renderer { word: 1 }
}
pub fn renderer_frame(r: &mut Renderer, color: u16) -> Frame {
    if GPU_SCOPE_MODEL[0u16] != 1 {
        renderer_model_fault(22);
    }
    let mut left: u16 = 60000;
    while GPU_PIPE[2u16] != 0 || GPU_DRIVER[0u16] == 4 {
        if left == 0 || gpu_pipeline_advance() == GPU_FAULT {
            renderer_model_fault(23);
        }
        left -= 1;
    }
    let slot = gpu_pipeline_begin(color, 0x2222);
    if slot == 0 {
        renderer_model_fault(24);
    }
    GPU_SCOPE_MODEL[0u16] = 2;
    GPU_SCOPE_MODEL[1u16] = 0;
    Frame { word: slot }
}
// One short field; the game supplies neither packed words nor transparency.
pub fn frame_number(f: &mut Frame, number: u16) {
    if GPU_SCOPE_MODEL[0u16] != 2 {
        renderer_model_fault(25);
    }
    let mut digits: Buf<u16, 5> = Buf::new([0; 5]);
    let mut count: u16 = 1;
    let mut n = number;
    while n >= 10 {
        count += 1;
        n = n / 10;
    }
    let mut i = count;
    n = number;
    while i != 0 {
        i -= 1;
        digits[i] = 48 + n % 10;
        n = n / 10;
    }
    if !gpu_persistent_ui_field(0, 8, digits.as_array(), count, true) {
        renderer_model_fault(26);
    }
}
pub fn frame_pass(f: &mut Frame) -> Pass {
    if GPU_SCOPE_MODEL[0u16] != 2 || GPU_SCOPE_MODEL[1u16] != 0 {
        renderer_model_fault(27);
    }
    GPU_SCOPE_MODEL[0u16] = 3;
    Pass { word: f.word }
}
pub fn pass_text(p: &mut Pass) {
    if GPU_SCOPE_MODEL[0u16] != 3 || !gpu_pipeline_ui_range(0, 8) {
        renderer_model_fault(28);
    }
}
pub fn pass_complete(p: Pass) {
    GPU_SCOPE_MODEL[0u16] = 2;
    GPU_SCOPE_MODEL[1u16] = 2;
    resource_disarm(p);
}
fn pass_cleanup(p: Pass) {
    GPU_SCOPE_MODEL[0u16] = 2;
    GPU_SCOPE_MODEL[1u16] = 3;
}
fn frame_cleanup(f: Frame) {
    if GPU_SCOPE_MODEL[0u16] == 2 {
        if !gpu_pipeline_cancel() {
            renderer_model_fault(29);
        }
        GPU_SCOPE_MODEL[0u16] = 1;
    }
}
pub fn frame_complete(f: Frame) {
    if GPU_SCOPE_MODEL[1u16] != 2 {
        frame_cleanup(f);
        return;
    }
    if !gpu_pipeline_seal() {
        gpu_pipeline_fail(30);
        GPU_SCOPE_MODEL[0u16] = 5;
        resource_disarm(f);
        halt(0xe01e);
    }
    // Scheduler owns the sealed page now, even if it must wait for Display.
    GPU_SCOPE_MODEL[0u16] = 1;
    resource_disarm(f);
    if gpu_pipeline_advance() == GPU_FAULT {
        renderer_model_fault(31);
    }
}
fn renderer_cleanup(r: Renderer) {
    // No poll/advance or wait: asynchronous work remains frozen in place.
    if GPU_PIPE[2u16] != 0 || GPU_PIPE[3u16] != 0 || GPU_DISPLAY[2u16] != 0 {
        GPU_SCOPE_MODEL[0u16] = 5;
        gpu_pipeline_fail(32);
    } else {
        GPU_SCOPE_MODEL[0u16] = 4;
    }
}
pub fn renderer_close(r: Renderer) {
    let mut left: u16 = 60000;
    loop {
        let status = gpu_pipeline_advance();
        if status == GPU_FAULT || left == 0 {
            GPU_SCOPE_MODEL[0u16] = 5;
            gpu_pipeline_fail(33);
            resource_disarm(r);
            halt(0xe021);
        }
        if GPU_PIPE[2u16] == 0 && GPU_PIPE[3u16] == 0 && GPU_DISPLAY[2u16] == 0 {
            break;
        }
        left -= 1;
    }
    GPU_SCOPE_MODEL[0u16] = 4;
    resource_disarm(r);
}
