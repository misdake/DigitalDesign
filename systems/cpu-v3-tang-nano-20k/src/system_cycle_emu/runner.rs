//! Passive, application-independent helpers for bounded model runners.
use super::{Observation, System};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const MAX_EVENTS: u64 = 112_000_000;
pub const MAX_LOGIC: u64 = 80_000_000;
pub const MAX_HOST: u64 = 7_200;
pub const HOST_STACK_BYTES: usize = 8 * 1024 * 1024;

/// Windows' default main-thread stack is too small for this model composition.
/// One bounded worker owns all model clocks; this adds no model parallelism.
pub fn host_worker<T: Send + 'static>(
    run: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    std::thread::Builder::new()
        .name("ordinary-profile".into())
        .stack_size(HOST_STACK_BYTES)
        .spawn(run)
        .map_err(|e| format!("host worker spawn: {e}"))?
        .join()
        .map_err(|_| "host worker panicked".to_string())?
}

/// First unheld Execute arrival, not instruction retirement. A prefix or a
/// cache-miss LOAD may enter Execute without retiring on that edge. Non-logic
/// observations leave the deduplication state intact.
#[derive(Default)]
pub struct ExecuteArrival {
    previous: Option<(u16, u16)>,
}
impl ExecuteArrival {
    pub fn sample(&mut self, executing: Option<(u16, u16)>, held: bool) -> Option<(u16, u16)> {
        let current = if held { None } else { executing };
        let arrival = current.filter(|pc| Some(*pc) != self.previous);
        self.previous = current;
        arrival
    }
    pub fn observe(&mut self, o: Observation<'_>) -> Option<(u16, u16)> {
        let cpu = o.cpu?;
        self.sample(
            o.executing_pc,
            o.startup_hold || cpu.system.cpu_hold || cpu.dcache.valid_sweep,
        )
    }
}

pub fn snapshot(system: &System, words: &BTreeMap<String, u32>) -> Result<Value, String> {
    let mut result = serde_json::Map::new();
    for (name, address) in words {
        result.insert(
            name.clone(),
            json!(system
                .inspect_cpu_word(*address)
                .ok_or("probe outside RAM")?),
        );
    }
    Ok(Value::Object(result))
}

/// Device enables are generated only for the actual, unheld Execute edge.
/// Watch probes are deliberately not promoted to CPU reads.
pub fn cpu_device(o: Observation<'_>) -> Option<Value> {
    let c = &o.cpu?.core;
    (c.device_read_enable || c.device_write_enable).then(|| {
        json!({
            "kind":"device", "tick":o.instant.ticks, "pc":o.executing_pc,
            "retired_before":c.retired_words, "device":c.device_index,
            "channel":c.device_channel, "write":c.device_write_enable,
            "value":if c.device_write_enable {c.device_write_data as u16} else {o.device_read_data}
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_worker_supports_a_bounded_large_frame() {
        fn large_frame() -> Result<usize, String> {
            let mut frame = [0u8; 2 * 1024 * 1024];
            for i in (0..frame.len()).step_by(4096) {
                frame[i] = (i / 4096) as u8;
            }
            std::hint::black_box(&frame);
            Ok(frame.len())
        }
        assert_eq!(host_worker(large_frame).unwrap(), 2 * 1024 * 1024);
    }
    #[test]
    fn execute_arrival_is_independent_of_retirement_and_rearms_after_wait_or_hold() {
        let mut a = ExecuteArrival::default();
        let prefix = (7, 41816);
        let load = (7, 41817);
        assert_eq!(a.sample(Some(prefix), true), None);
        assert_eq!(a.sample(Some(prefix), false), Some(prefix));
        assert_eq!(a.sample(Some(prefix), false), None);
        assert_eq!(a.sample(Some(load), false), Some(load));
        assert_eq!(a.sample(Some(load), false), None);
        assert_eq!(a.sample(None, false), None); // MemoryWait retirement has no Execute PC.
        assert_eq!(a.sample(Some(prefix), false), Some(prefix)); // Next dynamic call.
        assert_eq!(a.sample(Some(prefix), true), None);
        assert_eq!(a.sample(Some(prefix), false), Some(prefix));
    }
}
