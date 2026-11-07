//! Bounded real-stage rules, native Instances, RTT and actual Display tests.
use super::changes::{global, preload};
use super::*;
use cpu_v3::StepOutcome;
use std::path::PathBuf;

mod scenarios;

mod harness;
use harness::*;

mod reference;
use reference::*;

mod inspection;
use inspection::*;

mod rules_cases;

mod transition_cases;

mod rendering_cases;

mod shadow_cases;

mod goal_cases;

mod hud_cases;

mod atlas;
mod batch_cost;
mod buttons;
mod cpu_emu;
mod fragile_batch;
mod frame_schedule;
mod moving;
mod orientation;
mod pad_surface;
mod static_cache;
