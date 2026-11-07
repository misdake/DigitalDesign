use super::*;
use crate as cpu_v3;
use crate::rcc_backend::{self, CompilerOptions};
use crate::{AluOp, CpuV3Sim, ImmediateOp, RunOutcome, SpecialRegister, TestCondition};
use digital_design_circuit::{build_circuit, Circuit};
use digital_design_hardware::{HardwareIdentity, Module, ModuleIo, VerilogProject};
use rcc::frontend::compile_program;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

// Bounded emulator, RTL, geometry and leaf contracts share one core harness.
mod harness;
use harness::*;

mod emu;

mod cosim;

mod geometry;

mod leaves;

#[path = "../shared_wide_study.rs"]
mod shared_wide_study;
