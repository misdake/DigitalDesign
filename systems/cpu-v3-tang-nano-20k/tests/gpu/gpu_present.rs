//! Present/UI success and fault cases share their existing fixture hierarchy.
#[allow(dead_code)]
#[path = "../support/gpu_rcc_ui_fixture.rs"]
mod ui_fixture;
use ui_fixture::present as present_fixture;
#[path = "present/gpu_rcc_present.rs"]
mod gpu_rcc_present;
#[path = "present/gpu_rcc_present_fault.rs"]
mod gpu_rcc_present_fault;
#[path = "present/gpu_rcc_ui_fault.rs"]
mod gpu_rcc_ui_fault;
#[path = "present/gpu_rcc_ui_present.rs"]
mod gpu_rcc_ui_present;
