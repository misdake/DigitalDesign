//! Freeze an already compiled program at its real Prepared capture gate.
//! Both CPU backends share descriptor validation and physical GPU payloads.
//! GPU/Display remain functional; native CPU counts exclude their competition.
use crate::system_sim::{CpuV3SystemSim, VBlankMode};
use cpu_v3::StepOutcome;

#[allow(dead_code)]
#[path = "../rcc/gpu_capture_abi.rs"]
pub(crate) mod abi;

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

/// Immutable GPU inputs. Only the producer-owned header/table use a CPU view;
/// every payload remains the raw physical image consumed by the GPU.
pub struct FrozenData {
    pub header: Vec<u16>,
    pub ranges: Vec<u16>,
    pub payloads: Vec<Vec<u8>>,
}
pub struct NativeFrozenInputs {
    pub data: FrozenData,
    /// Raw GPU memory, without the CPU-private metadata overlay.
    pub physical_memory: Vec<u16>,
    pub display_state: crate::system_sim::DisplayState,
    pub logic_cycles: u64,
    pub native_cycles: u64,
    pub vblank_events: usize,
}

fn copy_words(memory: &[u16], start: usize, count: usize) -> Result<Vec<u16>, String> {
    let end = start.checked_add(count).ok_or("Capture address overflow")?;
    memory
        .get(start..end)
        .map(<[u16]>::to_vec)
        .ok_or_else(|| "Capture range exceeds physical memory".into())
}

fn pair(words: &[u16], at: u16) -> u32 {
    u32::from(words[usize::from(at)]) | (u32::from(words[usize::from(at) + 1]) << 16)
}

// Validate the shared lease envelope before reading its table. Detailed command,
// layout and captured-memory semantics remain owned by the snapshot decoder.
fn range_count(header: &[u16]) -> Result<usize, String> {
    if header.len() != usize::from(abi::GPU_CAPTURE_WORDS)
        || header[usize::from(abi::GPU_CAPTURE_H_WORDS)] != abi::GPU_CAPTURE_WORDS
    {
        return Err("Invalid capture descriptor length".into());
    }
    let flags = header[usize::from(abi::GPU_CAPTURE_H_FLAGS)];
    let flags_valid = match header[usize::from(abi::GPU_CAPTURE_H_VERSION)] {
        abi::GPU_CAPTURE_VERSION => flags == abi::GPU_CAPTURE_PUBLISHED,
        abi::GPU_CAPTURE_EXPLICIT_VERSION => {
            let allowed = abi::GPU_CAPTURE_PUBLISHED
                | abi::GPU_CAPTURE_HAS_TARGET
                | abi::GPU_CAPTURE_HAS_FRONT
                | abi::GPU_CAPTURE_HAS_UI;
            flags & abi::GPU_CAPTURE_PUBLISHED != 0 && flags & !allowed == 0
        }
        _ => return Err("Unsupported capture descriptor version".into()),
    };
    if !flags_valid {
        return Err("Unpublished capture descriptor or unknown flags".into());
    }
    if header[usize::from(abi::GPU_CAPTURE_H_BINDINGS)] != abi::GPU_CAPTURE_BINDINGS_SELF_CONTAINED
        || header[usize::from(abi::GPU_CAPTURE_H_RANGE_WORDS)] != abi::GPU_CAPTURE_RANGE_WORDS
        || header[usize::from(abi::GPU_CAPTURE_H_RANGE_VERSION)] != abi::GPU_CAPTURE_RANGE_VERSION
    {
        return Err("Incompatible capture binding or range ABI".into());
    }
    if header[usize::from(abi::GPU_CAPTURE_H_GPU_STATUS)] != 0 {
        return Err("GPU is busy or faulted in capture descriptor".into());
    }
    let count = header[usize::from(abi::GPU_CAPTURE_H_RANGE_COUNT)];
    if count == 0 || count > abi::GPU_CAPTURE_MAX_RANGES {
        return Err("Capture range count outside driver ABI".into());
    }
    Ok(usize::from(count))
}

fn payload_span(row: &[u16], memory_words: usize) -> Result<(usize, usize), String> {
    if row.len() != usize::from(abi::GPU_CAPTURE_RANGE_WORDS) {
        return Err("Invalid capture range descriptor length".into());
    }
    let flags = row[usize::from(abi::GPU_CAPTURE_R_FLAGS)];
    if flags & !abi::GPU_CAPTURE_FLAGS_ALL != 0
        || flags & (abi::GPU_CAPTURE_READ | abi::GPU_CAPTURE_WRITE) == 0
        || flags & abi::GPU_CAPTURE_IMMUTABLE != 0 && flags & abi::GPU_CAPTURE_WRITE != 0
    {
        return Err("Invalid capture range flags".into());
    }
    let base = pair(row, abi::GPU_CAPTURE_R_BASE);
    let bytes = pair(row, abi::GPU_CAPTURE_R_BYTES);
    if base & 1 != 0 || bytes == 0 || bytes & 1 != 0 {
        return Err("Capture payload must be a nonempty halfword range".into());
    }
    let start = usize::try_from(base / 2).map_err(|_| "Capture address overflow")?;
    let count = usize::try_from(bytes / 2).map_err(|_| "Capture length overflow")?;
    if start
        .checked_add(count)
        .is_none_or(|end| end > memory_words)
    {
        return Err("Capture range exceeds physical memory".into());
    }
    Ok((start, count))
}

pub(crate) fn freeze_data(
    point: CapturePoint,
    mut metadata: impl FnMut(usize, usize) -> Result<Vec<u16>, String>,
    physical: &[u16],
) -> Result<FrozenData, String> {
    let header = metadata(point.header_word, usize::from(abi::GPU_CAPTURE_WORDS))?;
    let count = range_count(&header)?;
    let ranges = metadata(
        point.ranges_word,
        count * usize::from(abi::GPU_CAPTURE_RANGE_WORDS),
    )?;
    let mut payloads = Vec::new();
    let mut total_bytes = 0;
    for row in ranges.chunks_exact(usize::from(abi::GPU_CAPTURE_RANGE_WORDS)) {
        let (start, count) = payload_span(row, physical.len())?;
        total_bytes += count * 2;
        if total_bytes > usize::from(abi::GPU_CAPTURE_MAX_TOTAL_HIGH) << 16 {
            return Err("Capture exceeds aggregate byte bound".into());
        }
        payloads.push(
            copy_words(physical, start, count)?
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect(),
        );
    }
    Ok(FrozenData {
        header,
        ranges,
        payloads,
    })
}

/// Native counterpart of the legacy reject-prefix frontend.
pub fn capture_native_flash(
    flash: &[u8],
    point: CapturePoint,
    memory_seed: u16,
    maximum_cycles: usize,
) -> Result<NativeFrozenInputs, String> {
    capture_native(flash, point, memory_seed, maximum_cycles, false)
}

/// Execute real functional prefix submissions, then stop before the first
/// instruction of the actual compiled Prepared gate executes. No UART bypass.
/// CPU-private header/table reads do not publish their dirty words into DRAM.
pub fn capture_native_flash_with_functional_prefix(
    flash: &[u8],
    point: CapturePoint,
    memory_seed: u16,
    maximum_cycles: usize,
) -> Result<NativeFrozenInputs, String> {
    capture_native(flash, point, memory_seed, maximum_cycles, true)
}
fn capture_native(
    flash: &[u8],
    point: CapturePoint,
    memory_seed: u16,
    maximum_cycles: usize,
    functional_prefix: bool,
) -> Result<NativeFrozenInputs, String> {
    if maximum_cycles == 0 {
        return Err("Capture requires a positive native CPU cycle bound".into());
    }
    let mut sim = crate::system_emu::CpuV3SystemEmu::from_single_flash(
        flash,
        memory_seed,
        VBlankMode::PauseOnFrameIndexWait,
    )
    .map_err(|e| e.to_string())?;
    if functional_prefix {
        sim = sim.with_functional_gpu();
    }
    let mut vblank_events = 0;
    for _ in 0..maximum_cycles {
        if let Some(error) = sim.gpu().last_error() {
            return Err(format!("GPU prefix execution failed: {error:?}"));
        }
        if sim.gpu().busy() {
            return Err("GPU submission preceded the Prepared capture gate".into());
        }
        if sim.core().executing_instruction_address() == Some((7, point.gate_pc)) {
            let data = freeze_data(
                point,
                |start, count| sim.metadata_words(start, count),
                &sim.physical_memory(),
            )?;
            return Ok(NativeFrozenInputs {
                data,
                physical_memory: sim.physical_memory().to_vec(),
                display_state: sim.display_state(),
                logic_cycles: sim.cycles(),
                native_cycles: sim.native_cycles(),
                vblank_events,
            });
        }
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
            vblank_events += 1;
        }
        let out = sim
            .step()
            .map_err(|e| format!("Native CPU failed before capture: {e:?}"))?;
        if out.fault {
            return Err(format!(
                "Native CPU fault {} before capture",
                out.fault_code
            ));
        }
        if out.halted {
            return Err(format!(
                "Program halted before native capture: {:#06x}",
                out.halt_signal
            ));
        }
    }
    Err(format!(
        "CPU logic cycle bound {maximum_cycles} reached before capture; incomplete"
    ))
}

/// Stop before executing the gate and before either GPU SEND can be consumed.
/// No GPU executor is attached: an earlier submission is an explicit error.
pub fn capture_flash(
    flash: &[u8],
    point: CapturePoint,
    memory_seed: u16,
    maximum_steps: usize,
) -> Result<FrozenInputs, String> {
    capture(flash, point, memory_seed, maximum_steps, false)
}

/// Execute preceding submissions through the functional GPU before freezing.
/// Stops at the first arrival at `point`, without executing or bypassing its gate.
/// Earlier gates must return normally; this does not provide a UART/input peer.
pub fn capture_flash_with_functional_prefix(
    flash: &[u8],
    point: CapturePoint,
    memory_seed: u16,
    maximum_steps: usize,
) -> Result<FrozenInputs, String> {
    capture(flash, point, memory_seed, maximum_steps, true)
}

fn capture(
    flash: &[u8],
    point: CapturePoint,
    memory_seed: u16,
    maximum_steps: usize,
    functional_prefix: bool,
) -> Result<FrozenInputs, String> {
    if maximum_steps == 0 {
        return Err("Capture requires a positive CPU step bound".into());
    }
    let mut sim =
        CpuV3SystemSim::from_single_flash(flash, memory_seed, VBlankMode::PauseOnFrameIndexWait)
            .map_err(|e| e.to_string())?;
    if functional_prefix {
        sim = sim.with_functional_gpu();
    }
    let mut vblank_events = 0;
    for cpu_steps in 0..maximum_steps {
        if let Some(error) = sim.gpu().last_error() {
            return Err(format!("GPU prefix execution failed: {error:?}"));
        }
        if sim.gpu().busy() {
            return Err("GPU submission preceded the Prepared capture gate".into());
        }
        if sim.cpu().pc() == point.gate_pc {
            let physical = sim.cpu().physical_memory_slice();
            let FrozenData {
                header,
                ranges,
                payloads,
            } = freeze_data(
                point,
                |start, count| copy_words(physical, start, count),
                physical,
            )?;
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

#[cfg(test)]
#[path = "profiling_capture_tests.rs"]
mod tests;
