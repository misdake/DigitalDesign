//! Event-only cache/Color ownership audit; no DUT numerical helper is called.
use gpu_v2::texture::{
    emu::color,
    ports::Group4,
    sim::{staged::bound::runtime, timed},
};

pub struct CaptureAudit {
    enabled: u64,
    pending: Option<(Group4, usize, u64)>,
    link: Option<(Group4, u64)>,
    pub reads: usize,
    pub captures: usize,
    pub accepts: usize,
    pub frozen_pending: usize,
    pub longest_link_hold: u64,
    csv: String,
}
impl Default for CaptureAudit {
    fn default() -> Self {
        Self { enabled: 0, pending: None, link: None, reads: 0, captures: 0,
            accepts: 0, frozen_pending: 0, longest_link_hold: 0,
            csv: "wall,cache_enabled,read_key,capture_key,color_key,result_key,pins,color_credits,refill_beats,link_age\n".into() }
    }
}
impl CaptureAudit {
    pub fn observe(&mut self, s: &runtime::Step) {
        assert!(!s.cache.snapshot.reference_color);
        assert_eq!(
            (s.cache.snapshot.results, s.cache.snapshot.result_credits),
            (0, 0)
        );
        assert!(!s.cache.events.iter().any(|e| matches!(
            e,
            timed::Event::Partial { .. }
                | timed::Event::Accumulate { .. }
                | timed::Event::Result { .. }
                | timed::Event::Commit { .. }
        )));
        self.enabled += u64::from(s.effective_ce);
        let mut color_key = None;
        for e in &s.color.events {
            if let color::Event::Accepted { key, first, last } = *e {
                assert!(color_key.replace(key).is_none(), "one Color input port");
                let (group, born) = self.link.take().expect("Color used uncaptured data");
                assert_eq!(
                    (key, first, last),
                    (group.quad_id * 4 + group.lane, group.first, group.last)
                );
                assert!(s.cycle > born, "capture is not same-edge available");
                self.longest_link_hold = self.longest_link_hold.max(s.cycle - born);
                self.accepts += 1;
            }
        }
        let mut read_key = None;
        let mut capture_key = None;
        let mut beats = 0;
        for e in &s.cache.events {
            match e {
                timed::Event::Captured { group, line, .. } => {
                    assert!(s.effective_ce);
                    assert!(
                        capture_key
                            .replace(group.quad_id * 4 + group.lane)
                            .is_none(),
                        "one capture port"
                    );
                    let (old, old_line, issued) =
                        self.pending.take().expect("capture without read");
                    assert_eq!((group, *line), (&old, old_line));
                    assert_eq!(self.enabled - issued, 1, "one enabled-edge bank return");
                    assert!(
                        self.link.replace((group.clone(), s.cycle)).is_none(),
                        "unconsumed capture overwritten"
                    );
                    self.captures += 1;
                }
                timed::Event::Read { group, line, .. } => {
                    assert!(s.effective_ce);
                    assert!(
                        read_key.replace(group.quad_id * 4 + group.lane).is_none(),
                        "one read port"
                    );
                    assert!(self
                        .pending
                        .replace((group.clone(), *line, self.enabled))
                        .is_none());
                    self.reads += 1;
                }
                timed::Event::Beat { .. } => beats += 1,
                _ => {}
            }
        }
        if !s.effective_ce && self.pending.is_some() {
            self.frozen_pending += 1;
        }
        let pins = self
            .pending
            .as_ref()
            .map_or(0, |(_, line, _)| 1_u64 << line);
        assert_eq!(
            s.cache.snapshot.reservations, pins,
            "pin lifetime ends only at capture"
        );
        assert_eq!(
            s.cache.snapshot.pipeline,
            usize::from(self.pending.is_some())
        );
        assert_eq!(s.snapshot.color_input, self.link.is_some());
        assert!(s.results.len() <= 1 && beats <= 1);
        let key = |v: Option<u8>| v.map_or(-1, i32::from);
        use std::fmt::Write;
        writeln!(
            self.csv,
            "{},{},{},{},{},{},{},{},{},{}",
            s.cycle,
            self.enabled,
            key(read_key),
            key(capture_key),
            key(color_key),
            key(s.results.first().map(|o| o.key)),
            pins,
            s.snapshot.color.result_credits,
            beats,
            self.link.as_ref().map_or(0, |(_, t)| s.cycle - t)
        )
        .unwrap();
    }
    pub fn finish(&self, name: &str) {
        assert!(self.pending.is_none() && self.link.is_none());
        assert_eq!(self.reads, self.captures);
        assert_eq!(self.captures, self.accepts);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sampling-sequential-storage-20261008");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}-ports.csv")), &self.csv).unwrap();
        std::fs::write(dir.join(format!("{name}-summary.json")), format!(
            "{{\"reads\":{},\"captures\":{},\"color_accepts\":{},\"frozen_pending\":{},\"longest_link_hold\":{},\"cache_pending_capacity\":1,\"link_capacity\":1,\"reference_color_allocated\":false}}\n",
            self.reads,self.captures,self.accepts,self.frozen_pending,self.longest_link_hold)).unwrap();
    }
}
