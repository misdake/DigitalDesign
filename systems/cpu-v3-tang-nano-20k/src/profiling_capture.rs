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
            let count = range_count(&header)?;
            let ranges = words(
                &mut sim,
                point.ranges_word,
                count * usize::from(abi::GPU_CAPTURE_RANGE_WORDS),
            )?;
            let mut payloads = Vec::new();
            let memory_words = sim.cpu_mut().physical_memory_mut().len();
            let mut total_bytes = 0usize;
            for row in ranges.chunks_exact(usize::from(abi::GPU_CAPTURE_RANGE_WORDS)) {
                let (start, words_count) = payload_span(row, memory_words)?;
                total_bytes += words_count * 2;
                if total_bytes > usize::from(abi::GPU_CAPTURE_MAX_TOTAL_HIGH) << 16 {
                    return Err("Capture exceeds aggregate byte bound".into());
                }
                let data = words(&mut sim, start, words_count)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn captures() -> Vec<(Vec<u16>, Vec<Vec<u16>>)> {
        let recorded: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/prepared_capture_descriptors.json"
        ))
        .unwrap();
        recorded["captures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|capture| {
                (
                    serde_json::from_value(capture["header"].clone()).unwrap(),
                    serde_json::from_value(capture["ranges"].clone()).unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn real_v1_and_v2_prepared_envelopes_and_ranges_are_accepted() {
        for (header, ranges) in captures() {
            assert_eq!(range_count(&header).unwrap(), ranges.len());
            for row in ranges {
                payload_span(&row, 1 << 22).unwrap();
            }
        }
    }

    #[test]
    fn malformed_or_unpublished_envelopes_are_rejected() {
        for (header, _) in captures() {
            assert!(range_count(&header[..63]).unwrap_err().contains("length"));
            let mut extra = header.clone();
            extra.push(0);
            assert!(range_count(&extra).unwrap_err().contains("length"));
            for (field, value, reason) in [
                (abi::GPU_CAPTURE_H_WORDS, 63, "length"),
                (abi::GPU_CAPTURE_H_VERSION, 3, "version"),
                (abi::GPU_CAPTURE_H_FLAGS, 0, "Unpublished"),
                (abi::GPU_CAPTURE_H_FLAGS, 2, "Unpublished"),
                (abi::GPU_CAPTURE_H_FLAGS, 0x8001, "unknown flags"),
                (abi::GPU_CAPTURE_H_BINDINGS, 0, "binding"),
                (abi::GPU_CAPTURE_H_BINDINGS, 2, "binding"),
                (abi::GPU_CAPTURE_H_RANGE_WORDS, 7, "range ABI"),
                (abi::GPU_CAPTURE_H_RANGE_VERSION, 2, "range ABI"),
                (abi::GPU_CAPTURE_H_RANGE_COUNT, 0, "range count"),
                (abi::GPU_CAPTURE_H_RANGE_COUNT, 65, "range count"),
                (abi::GPU_CAPTURE_H_GPU_STATUS, 1, "busy or faulted"),
                (abi::GPU_CAPTURE_H_GPU_STATUS, 2, "busy or faulted"),
            ] {
                let mut invalid = header.clone();
                invalid[usize::from(field)] = value;
                assert!(
                    range_count(&invalid).unwrap_err().contains(reason),
                    "version{} field{field}={value}",
                    header[0]
                );
            }
        }
        let mut v1 = captures().remove(0).0;
        v1[usize::from(abi::GPU_CAPTURE_H_FLAGS)] |= abi::GPU_CAPTURE_HAS_TARGET;
        assert!(range_count(&v1).unwrap_err().contains("unknown flags"));
    }

    #[test]
    fn malformed_range_flags_extents_and_table_rows_are_rejected() {
        for (_, rows) in captures() {
            let row = &rows[0];
            assert!(payload_span(&row[..7], 1 << 22)
                .unwrap_err()
                .contains("length"));
            for (field, value, reason) in [
                (abi::GPU_CAPTURE_R_FLAGS, 0, "flags"),
                (abi::GPU_CAPTURE_R_FLAGS, 33, "flags"),
                (
                    abi::GPU_CAPTURE_R_FLAGS,
                    abi::GPU_CAPTURE_IMMUTABLE | abi::GPU_CAPTURE_WRITE,
                    "flags",
                ),
                (abi::GPU_CAPTURE_R_BASE, 1, "halfword"),
                (abi::GPU_CAPTURE_R_BYTES, 0, "halfword"),
                (abi::GPU_CAPTURE_R_BYTES, 3, "halfword"),
                (abi::GPU_CAPTURE_R_BASE + 1, 128, "physical memory"),
                (abi::GPU_CAPTURE_R_BYTES + 1, 128, "physical memory"),
            ] {
                let mut invalid = row.clone();
                invalid[usize::from(field)] = value;
                assert!(
                    payload_span(&invalid, 1 << 22)
                        .unwrap_err()
                        .contains(reason),
                    "range field{field}={value}"
                );
            }
        }
    }
}
