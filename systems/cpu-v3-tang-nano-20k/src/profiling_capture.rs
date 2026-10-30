//! Freeze an already compiled program at its real Prepared capture gate.
//! This is functional CPU/system simulation, not cache or board timing.
use crate::system_sim::{CpuV3SystemSim, VBlankMode};
use cpu_v3::StepOutcome;

#[allow(dead_code)]
#[path = "../rcc/gpu_capture_abi.rs"]
mod abi;

/// Locations from the exact compiled program's debug metadata, never guesses.
#[derive(Clone, Copy, Debug)]
pub struct CapturePoint {
    pub gate_pc: u16,
    pub header_word: usize,
    pub ranges_word: usize,
}

impl CapturePoint {
    /// Bind the existing gate and ABI arrays to this compilation's addresses.
    pub fn from_program(program: &cpu_v3::rcc_backend::CpuV3Program) -> Result<Self, String> {
        let gates: Vec<_> = program
            .debug
            .functions
            .iter()
            .filter(|f| f.name.rsplit("::").next() == Some("profiler_gate"))
            .collect();
        if gates.len() != 1 {
            return Err("Compiled program needs exactly one profiler_gate".into());
        }
        let global = |name: &str| -> Result<usize, String> {
            let found: Vec<_> = program
                .debug
                .globals
                .iter()
                .filter_map(|g| {
                    if g.name.rsplit("::").next() == Some(name) {
                        if let rcc::VarLoc::Global(address) = g.loc {
                            return Some(usize::from(address));
                        }
                    }
                    None
                })
                .collect();
            if found.len() != 1 {
                return Err(format!("Compiled capture global missing/ambiguous: {name}"));
            }
            Ok(found[0])
        };
        Ok(Self {
            gate_pc: u16::try_from(gates[0].addr.0)
                .map_err(|_| "Capture PC exceeds CSEG offset")?,
            header_word: global("GPU_CAPTURE_HEADER")?,
            ranges_word: global("GPU_CAPTURE_RANGES")?,
        })
    }
}

/// Raw driver-owned ABI bytes shared with the UART snapshot decoder.
pub struct FrozenInputs {
    pub header: Vec<u16>,
    pub ranges: Vec<u16>,
    pub payloads: Vec<Vec<u8>>,
    pub cpu_steps: usize,
    pub vblank_events: usize,
}

fn words(sim: &mut CpuV3SystemSim, start: usize, count: usize) -> Result<Vec<u16>, String> {
    let end = start.checked_add(count).ok_or("Capture address overflow")?;
    sim.cpu_mut()
        .physical_memory_mut()
        .get(start..end)
        .map(<[u16]>::to_vec)
        .ok_or_else(|| "Capture range exceeds physical memory".into())
}

fn pair(words: &[u16], at: u16) -> u32 {
    u32::from(words[usize::from(at)]) | (u32::from(words[usize::from(at) + 1]) << 16)
}

/// Stop before executing the gate and before either GPU SEND can be consumed.
/// No GPU executor is attached: an earlier submission is an explicit error.
pub fn capture_flash(
    flash: &[u8],
    point: CapturePoint,
    memory_seed: u16,
    maximum_steps: usize,
) -> Result<FrozenInputs, String> {
    if maximum_steps == 0 {
        return Err("Capture requires a positive CPU step bound".into());
    }
    let mut sim =
        CpuV3SystemSim::from_single_flash(flash, memory_seed, VBlankMode::PauseOnFrameIndexWait)
            .map_err(|e| e.to_string())?;
    let mut vblank_events = 0;
    for cpu_steps in 0..maximum_steps {
        if sim.gpu().busy() {
            return Err("GPU submission preceded the Prepared capture gate".into());
        }
        if sim.cpu().pc() == point.gate_pc {
            let header = words(
                &mut sim,
                point.header_word,
                usize::from(abi::GPU_CAPTURE_WORDS),
            )?;
            if header[usize::from(abi::GPU_CAPTURE_H_VERSION)] != abi::GPU_CAPTURE_VERSION
                || header[usize::from(abi::GPU_CAPTURE_H_WORDS)] != abi::GPU_CAPTURE_WORDS
                || header[usize::from(abi::GPU_CAPTURE_H_FLAGS)] != abi::GPU_CAPTURE_PUBLISHED
            {
                return Err("Capture gate reached without a published driver descriptor".into());
            }
            let count = header[usize::from(abi::GPU_CAPTURE_H_RANGE_COUNT)];
            if count == 0 || count > abi::GPU_CAPTURE_MAX_RANGES {
                return Err("Capture range count outside driver ABI".into());
            }
            let ranges = words(
                &mut sim,
                point.ranges_word,
                usize::from(count) * usize::from(abi::GPU_CAPTURE_RANGE_WORDS),
            )?;
            let mut payloads = Vec::new();
            for row in ranges.chunks_exact(usize::from(abi::GPU_CAPTURE_RANGE_WORDS)) {
                let base = pair(row, abi::GPU_CAPTURE_R_BASE);
                let length = pair(row, abi::GPU_CAPTURE_R_BYTES);
                if base & 1 != 0 || length == 0 || length & 1 != 0 {
                    return Err("Capture payload must be a nonempty halfword range".into());
                }
                let data = words(&mut sim, base as usize / 2, length as usize / 2)?;
                payloads.push(data.into_iter().flat_map(u16::to_le_bytes).collect());
            }
            return Ok(FrozenInputs {
                header,
                ranges,
                payloads,
                cpu_steps,
                vblank_events,
            });
        }
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
            vblank_events += 1;
        }
        match sim
            .step()
            .map_err(|e| format!("CPU fault before capture: {e:?}"))?
        {
            StepOutcome::Halted { signal } => {
                return Err(format!("Program halted before capture: {signal:#06x}"));
            }
            StepOutcome::Running | StepOutcome::Signaled(_) => {}
        }
    }
    Err(format!(
        "CPU step bound {maximum_steps} reached before capture; incomplete"
    ))
}
