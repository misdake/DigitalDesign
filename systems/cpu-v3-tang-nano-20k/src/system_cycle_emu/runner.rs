//! Bounded host controller for an unchanged ordinary application.
//! All phase addresses/PCs are supplied by the exact linked program manifest.
use super::{Observation, System};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

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
            "wait_started_r8":o.cpu_wait_abi[0], "wait_start_epoch_r9":o.cpu_wait_abi[1],
            "finish_r10":o.cpu_wait_abi[2],
            "channel":c.device_channel, "write":c.device_write_enable,
            "value":if c.device_write_enable {c.device_write_data as u16} else {o.device_read_data}
        })
    })
}

pub fn site_enabled(name: &str, r10: u16) -> bool {
    name != "final_wait_return" || r10 != 0
}

#[derive(Default, Debug)]
pub struct Selection {
    pub warm: Option<u64>,
    pub stable: Option<u64>,
    pub rolling: Option<u64>,
    pub complete: BTreeSet<u64>,
    pub pressed: bool,
    pub released: bool,
    pub right_read: bool,
    pub ack: bool,
}
impl Selection {
    pub fn stable_state(s: &Value) -> bool {
        [
            ("phase", 0),
            ("moving", 0),
            ("level", 28),
            ("x", 7),
            ("z", 3),
            ("orientation", 0),
            ("mask", 25),
            ("transition", 0),
        ]
        .iter()
        .all(|(k, v)| s[*k].as_u64() == Some(*v))
            && (1..=9).all(|n| {
                s[format!("switch_age_{n}")]
                    .as_u64()
                    .is_some_and(|v| v >= 260)
            })
    }
    pub fn sample(&mut self, s: &Value, next_present: u64) -> Result<Option<&'static str>, String> {
        if !self.pressed && Self::stable_state(s) {
            if self.warm.is_none() {
                self.warm = Some(next_present);
                return Ok(Some("warm"));
            }
            if next_present > self.warm.unwrap() && self.stable.is_none() {
                self.stable = Some(next_present);
                return Ok(Some("stable-A"));
            }
        }
        if self.pressed && self.rolling.is_none() && s["phase"] == 1 {
            if s["x"] != 8
                || s["z"] != 3
                || s["orientation"] != 1
                || s["mask"] != 25
                || s["transition"] != 0
            {
                return Err("normal right input reached an unexpected ROLL state".into());
            }
            if self.released
                && self.right_read
                && self.ack
                && s["motion_age"]
                    .as_u64()
                    .is_some_and(|age| (100..220).contains(&age))
            {
                self.rolling = Some(next_present);
                return Ok(Some("rolling-B"));
            }
        }
        Ok(None)
    }
    pub fn input_gate(&mut self) -> bool {
        if !self.pressed && self.stable.is_some_and(|n| self.complete.contains(&n)) {
            self.pressed = true;
            return true;
        }
        false
    }
    pub fn consumed_gate(&mut self) -> bool {
        if self.pressed && !self.released && self.right_read && self.ack {
            self.released = true;
            return true;
        }
        false
    }
    pub fn device(&mut self, event: &Value) {
        if !self.pressed || self.released || event["device"] != 5 || event["committed"] != true {
            return;
        }
        if event["write"] == false && event["channel"] == 4 && event["value"] == 0xdf {
            self.right_read = true;
        }
        if event["write"] == true
            && event["channel"]
                .as_u64()
                .is_some_and(|c| (8..=15).contains(&c))
        {
            self.ack = true;
        }
    }
    pub fn selected(&self) -> Vec<u64> {
        let mut selected = vec![1, 2];
        selected.extend(self.stable);
        selected.extend(self.rolling);
        selected.sort_unstable();
        selected.dedup();
        selected
    }
    pub fn done(&self) -> bool {
        self.stable.is_some()
            && self.rolling.is_some()
            && self.selected().iter().all(|n| self.complete.contains(n))
    }
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
        assert!(!site_enabled("final_wait_return", 0));
        assert!(site_enabled("final_wait_return", 1));
        assert!(site_enabled("vblank_return", 0));
        assert_eq!(a.sample(Some(prefix), false), None);
        assert_eq!(a.sample(Some(load), false), Some(load));
        assert_eq!(a.sample(Some(load), false), None);
        assert_eq!(a.sample(None, false), None); // MemoryWait retirement has no Execute PC.
        assert_eq!(a.sample(Some(prefix), false), Some(prefix)); // Next dynamic call.
        assert_eq!(a.sample(Some(prefix), true), None);
        assert_eq!(a.sample(Some(prefix), false), Some(prefix));
    }
    fn stable() -> Value {
        let mut s = json!({"phase":0,"moving":0,"level":28,"x":7,"z":3,"orientation":0,"mask":25,"transition":0});
        for i in 1..=9 {
            s[format!("switch_age_{i}")] = json!(260);
        }
        s
    }
    #[test]
    fn input_release_requires_real_consumption_and_rolling_has_actual_age() {
        let mut c = Selection::default();
        let mut s = stable();
        s["phase"] = json!(2);
        assert_eq!(c.sample(&s, 1).unwrap(), None);
        s = stable();
        assert_eq!(c.sample(&s, 36).unwrap(), Some("warm"));
        assert_eq!(c.sample(&s, 36).unwrap(), None);
        assert_eq!(c.sample(&s, 37).unwrap(), Some("stable-A"));
        assert!(!c.input_gate());
        c.complete.insert(37);
        assert!(c.input_gate());
        assert!(!c.input_gate());
        assert!(!c.consumed_gate());
        c.device(&json!({"device":5,"channel":4,"value":223,"write":false,"committed":false}));
        assert!(!c.right_read);
        c.device(&json!({"device":5,"channel":4,"value":223,"write":false,"committed":true}));
        assert!(!c.consumed_gate());
        c.device(&json!({"device":5,"channel":8,"value":0,"write":true,"committed":true}));
        assert!(c.consumed_gate());
        assert!(!c.consumed_gate());
        s["phase"] = json!(1);
        s["x"] = json!(8);
        s["orientation"] = json!(1);
        s["motion_age"] = json!(85);
        assert_eq!(c.sample(&s, 39).unwrap(), None);
        s["motion_age"] = json!(119);
        assert_eq!(c.sample(&s, 40).unwrap(), Some("rolling-B"));
        assert_eq!(c.selected(), vec![1, 2, 37, 40]);
        assert!(!c.done());
        c.complete.extend([1, 2, 40]);
        assert!(c.done());
        let mut c = Selection {
            pressed: true,
            ..Default::default()
        };
        s["mask"] = json!(21);
        assert!(c.sample(&s, 40).is_err());
    }
}
