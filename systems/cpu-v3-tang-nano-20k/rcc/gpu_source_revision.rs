//! Monotonic source snapshot identity for Camera and Model updates.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;

// Never reset on arena/context reuse: no old source snapshot can regain a token.
static GPU_SOURCE_REVISION_COUNTER: Buf<u16, 4> = Buf::new([0; 4]);
fn gpu_source_revision(mut data: Array<u16>, at: u16) -> bool {
    let mut carry: u16 = 0;
    while carry < 4 && GPU_SOURCE_REVISION_COUNTER[carry] == 65535 {
        carry += 1;
    }
    if carry == 4 {
        return false;
    }
    let mut i: u16 = 0;
    while i < carry {
        GPU_SOURCE_REVISION_COUNTER[i] = 0;
        i += 1;
    }
    GPU_SOURCE_REVISION_COUNTER[carry] += 1;
    i = 0;
    while i < 4 {
        data[at + i] = GPU_SOURCE_REVISION_COUNTER[i];
        i += 1;
    }
    true
}

fn gpu_camera_revision(camera: Array<u16>) -> bool {
    gpu_source_revision(camera, GPU_CAMERA_REVISION)
}
fn gpu_model_revision(model: Array<u16>) -> bool {
    gpu_source_revision(model, GPU_MODEL_REVISION)
}
