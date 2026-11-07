//! Level 29 ordinary-game profiling selection; never mutates application state.
use serde_json::Value;
use std::collections::BTreeSet;

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
    use serde_json::json;
    #[test]
    fn shared_final_wait_site_requires_finish() {
        assert!(!site_enabled("final_wait_return", 0));
        assert!(site_enabled("final_wait_return", 1));
        assert!(site_enabled("vblank_return", 0));
    }

    #[test]
    fn stable_and_shadow_selection_preserve_boundary_conditions() {
        let s = stable();
        assert!(Selection::stable_state(&s));
        for (name, value) in [
            ("phase", 2),
            ("moving", 1),
            ("level", 27),
            ("x", 8),
            ("z", 4),
            ("orientation", 1),
            ("mask", 21),
            ("transition", 1),
        ] {
            let mut changed = s.clone();
            changed[name] = json!(value);
            assert!(!Selection::stable_state(&changed), "{name}");
        }
        for i in 1..=9 {
            let mut changed = s.clone();
            changed[format!("switch_age_{i}")] = json!(259);
            assert!(!Selection::stable_state(&changed));
        }
        for (age, selected) in [(99, false), (100, true), (219, true), (220, false)] {
            let mut c = Selection {
                pressed: true,
                released: true,
                right_read: true,
                ack: true,
                ..Default::default()
            };
            let mut rolling = s.clone();
            rolling["phase"] = json!(1);
            rolling["x"] = json!(8);
            rolling["orientation"] = json!(1);
            rolling["motion_age"] = json!(age);
            assert_eq!(c.sample(&rolling, 40).unwrap().is_some(), selected);
        }
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
