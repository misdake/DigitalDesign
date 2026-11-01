//! Model-owned derived coefficients; Camera revisions describe immutable snapshots.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_scene_math;
use gpu_scene_math::*;

// Never reset on arena/context reuse: no old Camera snapshot can regain a token.
static GPU_CAMERA_REVISION_COUNTER: Buf<u16, 4> = Buf::new([0; 4]);
fn gpu_camera_revision(mut camera: Array<u16>) -> bool {
    let mut carry: u16 = 0;
    while carry < 4 && GPU_CAMERA_REVISION_COUNTER[carry] == 65535 {
        carry += 1;
    }
    if carry == 4 {
        return false;
    }
    let mut i: u16 = 0;
    while i < carry {
        GPU_CAMERA_REVISION_COUNTER[i] = 0;
        i += 1;
    }
    GPU_CAMERA_REVISION_COUNTER[carry] += 1;
    i = 0;
    while i < 4 {
        camera[GPU_CAMERA_REVISION + i] = GPU_CAMERA_REVISION_COUNTER[i];
        i += 1;
    }
    true
}
fn gpu_model_cache_init(mut cache: Array<u16>) {
    cache[GPU_MODEL_DIRTY] = 1;
    let mut i: u16 = GPU_MODEL_VIEW_REVISION;
    while i < GPU_API_LARGE_WORDS {
        cache[i] = 0;
        i += 1;
    }
}
fn gpu_model_matrix_prepare(
    desired: Array<u16>,
    model: Array<u16>,
    mut native: Array<u16>,
    mut cache: Array<u16>,
    revision: Array<u16>,
) {
    let mut i: u16 = 0;
    while i < 4 {
        if cache[GPU_MODEL_VIEW_REVISION + i] != revision[i] {
            cache[GPU_MODEL_DIRTY] = 1;
        }
        i += 1;
    }
    if cache[GPU_MODEL_DIRTY] != 0 {
        gpu_scene_matrix_prepare(desired, model, cache);
        i = 0;
        while i < 4 {
            cache[GPU_MODEL_VIEW_REVISION + i] = revision[i];
            i += 1;
        }
        cache[GPU_MODEL_DIRTY] = 0;
    }
    i = 0;
    while i < 42 {
        native[GPU_SCRATCH_INSTANCE + i] = cache[i];
        i += 1;
    }
}
