//! Independent trace audit for the v0 three-vertex transaction. It observes
//! emitted steps only; it does not inspect microkernel private state.

use crate::events::{EVENT_COMMAND, HANDLER_PC};
use crate::microkernel::{MicroStep, MvpMode, PC_TRANSFORM};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditError {
    pub edge: Option<u64>,
    pub reason: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditReport {
    pub edges: u64,
    pub dma_beats: [usize; 2],
    pub dsp_products: [usize; 2],
    pub transformed_vertices: usize,
    pub triangles: usize,
}

fn fail(step: Option<&MicroStep>, reason: &'static str) -> AuditError {
    AuditError {
        edge: step.map(|step| step.edge),
        reason,
    }
}

/// Audit a completed `v0,v1,v2,t0,end` run. The expected DSP retirement
/// distances are fixed by the selected 3-edge and 2-edge primitive paths.
pub fn audit_v0_trace(trace: &[MicroStep], mode: MvpMode) -> Result<AuditReport, AuditError> {
    if trace.is_empty() {
        return Err(fail(None, "empty trace"));
    }
    for (index, step) in trace.iter().enumerate() {
        if step.edge != index as u64 + 1 {
            return Err(fail(Some(step), "nonsequential edge"));
        }
        if let Some(event) = step.dispatched_event {
            if HANDLER_PC
                .get(usize::from(event))
                .is_none_or(|&pc| trace.get(index + 1).is_none_or(|next| next.pc != pc))
            {
                return Err(fail(Some(step), "wrong handler entry"));
            }
        }
    }

    let command_events = trace
        .iter()
        .filter(|step| step.dispatched_event == Some(EVENT_COMMAND))
        .count();
    if command_events != 3 {
        return Err(fail(None, "expected two DMA commands and one DRAW"));
    }

    let mut dma_beats = [0; 2];
    let mut last_dma_write = [None; 2];
    let mut completion = [None; 2];
    for step in trace {
        if let Some(address) = step.scratch_write {
            let token = if address < 16 {
                0
            } else if (32..52).contains(&address) {
                1
            } else {
                return Err(fail(Some(step), "scratch write outside reserved regions"));
            };
            let expected = if token == 0 {
                dma_beats[token]
            } else {
                32 + dma_beats[token]
            };
            if address != expected {
                return Err(fail(Some(step), "DMA write address or order"));
            }
            dma_beats[token] += 1;
            last_dma_write[token] = Some(step.edge);
        }
        if let Some(token) = step.dma_completed {
            if token > 1 || completion[usize::from(token)].replace(step.edge).is_some() {
                return Err(fail(Some(step), "unexpected DMA completion"));
            }
        }
    }
    if dma_beats != [16, 20] {
        return Err(fail(None, "DMA beat count"));
    }
    for token in 0..2 {
        if completion[token].is_none_or(|edge| edge <= last_dma_write[token].unwrap()) {
            return Err(fail(None, "completion before last scratch write"));
        }
        let dispatches = trace
            .iter()
            .filter(|step| step.dispatched_event == Some(token as u8))
            .collect::<Vec<_>>();
        if dispatches.len() != 1 || dispatches[0].edge <= completion[token].unwrap() {
            return Err(fail(None, "DMA event missing or premature"));
        }
    }
    let first_read = trace
        .iter()
        .find(|step| step.scratch_read.is_some())
        .ok_or_else(|| fail(None, "core made no scratch read"))?;
    if first_read.edge <= completion[1].unwrap() {
        return Err(fail(
            Some(first_read),
            "core read before both DMA completions",
        ));
    }

    let mut transform_segments = Vec::new();
    let mut segment_start = None;
    for (index, step) in trace.iter().enumerate() {
        if step.pc == PC_TRANSFORM && segment_start.is_none() {
            segment_start = Some(index);
        }
        if step.pc != PC_TRANSFORM {
            if let Some(start) = segment_start.take() {
                transform_segments.push(&trace[start..index]);
            }
        }
    }
    if let Some(start) = segment_start {
        transform_segments.push(&trace[start..]);
    }
    if transform_segments.len() != 3 {
        return Err(fail(None, "transform segment count"));
    }

    let mut dsp_products = [0; 2];
    for segment in transform_segments {
        let expected_edges = match mode {
            MvpMode::Staged => 18,
            MvpMode::Streaming => 19,
            MvpMode::DualWide => 15,
        };
        if segment.len() != expected_edges {
            return Err(fail(segment.first(), "transform edge count"));
        }
        if mode == MvpMode::DualWide {
            let expected_issues = [
                16, 17, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 18, 19, 20, 21, 22,
                23, 24,
            ];
            let mut issues = Vec::new();
            let mut retired = [false; 25];
            let mut issue_edges = [None; 25];
            let mut matrix_reads = Vec::new();
            for step in segment {
                if step.small_issue.is_some() || step.small_retire.is_some() {
                    return Err(fail(Some(step), "dual-wide used small DSP"));
                }
                if let Some(address) = step.scratch_read {
                    matrix_reads.push(address);
                }
                for tag in [step.wide_issue, step.wide_issue_second]
                    .into_iter()
                    .flatten()
                {
                    if tag >= 25 || issue_edges[usize::from(tag)].replace(step.edge).is_some() {
                        return Err(fail(Some(step), "dual-wide issue tag"));
                    }
                    issues.push(tag);
                }
                for tag in [step.wide_retire, step.wide_retire_second]
                    .into_iter()
                    .flatten()
                {
                    if tag >= 25
                        || retired[usize::from(tag)]
                        || issue_edges[usize::from(tag)].is_none_or(|edge| step.edge != edge + 2)
                    {
                        return Err(fail(Some(step), "dual-wide retirement latency"));
                    }
                    retired[usize::from(tag)] = true;
                }
            }
            if issues != expected_issues || retired.contains(&false) {
                return Err(fail(segment.last(), "dual-wide product schedule"));
            }
            if matrix_reads != (0..8).collect::<Vec<_>>() {
                return Err(fail(segment.first(), "dual-wide matrix read schedule"));
            }
            dsp_products[0] += 25;
            continue;
        }
        let mut wide_issued = [None; 16];
        let mut small_issued = [None; 9];
        let mut wide_retired = [false; 16];
        let mut small_retired = [false; 9];
        let mut matrix_reads = Vec::new();
        for step in segment {
            if step.wide_issue_second.is_some() || step.wide_retire_second.is_some() {
                return Err(fail(Some(step), "unexpected second wide lane"));
            }
            if let Some(tag) = step.wide_issue {
                let index = usize::from(tag);
                if index >= 16
                    || wide_issued[index].replace(step.edge).is_some()
                    || index != dsp_products[0] % 16
                {
                    return Err(fail(Some(step), "wide issue tag"));
                }
                dsp_products[0] += 1;
            }
            if let Some(tag) = step.small_issue {
                let index = usize::from(tag);
                if index >= 9
                    || small_issued[index].replace(step.edge).is_some()
                    || index != dsp_products[1] % 9
                {
                    return Err(fail(Some(step), "small issue tag"));
                }
                dsp_products[1] += 1;
            }
            if let Some(tag) = step.wide_retire {
                let index = usize::from(tag);
                if index >= 16
                    || wide_retired[index]
                    || wide_issued[index].is_none_or(|edge| step.edge != edge + 2)
                {
                    return Err(fail(Some(step), "wide retirement latency"));
                }
                wide_retired[index] = true;
            }
            if let Some(tag) = step.small_retire {
                let index = usize::from(tag);
                if index >= 9
                    || small_retired[index]
                    || small_issued[index].is_none_or(|edge| step.edge != edge + 1)
                {
                    return Err(fail(Some(step), "small retirement latency"));
                }
                small_retired[index] = true;
            }
            if let Some(address) = step.scratch_read {
                matrix_reads.push(address);
            }
        }
        if wide_issued.contains(&None)
            || small_issued.contains(&None)
            || wide_retired.contains(&false)
            || small_retired.contains(&false)
        {
            return Err(fail(segment.last(), "missing DSP work"));
        }
        if matrix_reads
            != if mode == MvpMode::Streaming {
                (0..8).collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        {
            return Err(fail(segment.first(), "matrix read schedule"));
        }
    }

    let writes = trace
        .iter()
        .filter_map(|step| step.result_write.map(|row| (step.edge, row)))
        .collect::<Vec<_>>();
    if writes.iter().map(|(_, row)| *row).collect::<Vec<_>>() != (0..9).collect::<Vec<_>>() {
        return Err(fail(None, "transformed row write order"));
    }
    let published = trace
        .iter()
        .filter_map(|step| step.vertex_published.map(|id| (step.edge, id)))
        .collect::<Vec<_>>();
    if published.len() != 3 || published.iter().map(|(_, id)| *id).collect::<Vec<_>>() != [0, 1, 2]
    {
        return Err(fail(None, "transformed vertex order"));
    }
    for id in 0..3 {
        if published[id].0 != writes[id * 3 + 2].0 + 1 {
            return Err(fail(None, "vertex published before row commit"));
        }
    }
    let triangles = trace
        .iter()
        .filter(|step| step.triangle_pushed)
        .collect::<Vec<_>>();
    if triangles.len() != 1 || triangles[0].edge <= published[2].0 {
        return Err(fail(None, "setup queue publication order"));
    }
    Ok(AuditReport {
        edges: trace.len() as u64,
        dma_beats,
        dsp_products,
        transformed_vertices: published.len(),
        triangles: triangles.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupted_trace_fails_the_intended_audit_paths() {
        let mut trace = (1..=19)
            .map(|edge| MicroStep {
                edge,
                pc: PC_TRANSFORM,
                ..MicroStep::default()
            })
            .collect::<Vec<_>>();
        trace[0].dispatched_event = Some(1);
        assert_eq!(
            audit_v0_trace(&trace, MvpMode::Streaming)
                .unwrap_err()
                .reason,
            "wrong handler entry"
        );
        trace[0].dispatched_event = None;
        trace[1].edge = 5;
        assert_eq!(
            audit_v0_trace(&trace, MvpMode::Streaming)
                .unwrap_err()
                .reason,
            "nonsequential edge"
        );
    }
}
