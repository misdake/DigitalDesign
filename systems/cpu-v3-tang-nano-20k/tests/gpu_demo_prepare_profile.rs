//! Separate executable for preparation/pipeline experiments. This permits a
//! bounded prefix and a longer GPU experiment to coexist on Windows, where a
//! running test executable cannot be relinked. The implementation is shared.
#[path = "gpu_demo_profile.rs"]
mod shared_profile;
