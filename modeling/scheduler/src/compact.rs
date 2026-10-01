//! Delay nodes without changing lane phases, II or completion.
use crate::{check_modulo, ModuloError, ModuloGraph, ModuloSchedule};

/// Reverse-topological ALAP pass. Resource operations move by whole periods;
/// zero-cost nodes may move to their latest dependency-safe body time.
/// It can shorten some retained lifetimes; adapters compare actual bit pressure.
pub fn compact_modulo(
    graph: &ModuloGraph,
    schedule: &ModuloSchedule,
) -> Result<ModuloSchedule, ModuloError> {
    if !check_modulo(graph, schedule).is_ok() {
        return Err(ModuloError::Graph(crate::ScheduleError::InvalidCandidate(
            "invalid modulo input".into(),
        )));
    }
    let mut result = schedule.clone();
    let order = graph.topological_order()?;
    for &id in order.iter().rev() {
        let limit = graph
            .children(id)
            .iter()
            .map(|&c| result.nodes[c].issue)
            .min()
            .unwrap_or(schedule.span);
        let latest = limit
            .checked_sub(graph.latency(id))
            .ok_or(ModuloError::ArithmeticOverflow)?;
        let old = result.nodes[id].issue;
        if latest < old {
            return Err(ModuloError::ArithmeticOverflow);
        }
        result.nodes[id].issue = if result.nodes[id].lane.is_some() {
            old + (latest - old) / schedule.initiation_interval * schedule.initiation_interval
        } else {
            latest
        };
    }
    result.span = result
        .nodes
        .iter()
        .enumerate()
        .map(|(id, n)| n.issue.checked_add(graph.latency(id)))
        .collect::<Option<Vec<_>>>()
        .ok_or(ModuloError::ArithmeticOverflow)?
        .into_iter()
        .max()
        .unwrap_or(0);
    if result.span != schedule.span || !check_modulo(graph, &result).is_ok() {
        return Err(ModuloError::Graph(crate::ScheduleError::InvalidCandidate(
            "ALAP certificate".into(),
        )));
    }
    Ok(result)
}
