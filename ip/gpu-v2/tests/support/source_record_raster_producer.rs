//! Bounded test-only numeric adapter. Only SourceCapture owns source payload.
//! Oracle Input/Report/Encoded are call-local scratch, never a wall-edge queue.
use super::rr;
use gpu_v2::{
    frontend::source_capture,
    geometry::{record_transport as rec, source_record_link as link},
    triangle::sim::oracle,
};

#[derive(Debug, Default)]
pub struct Stats {
    pub accepted: u64,
    pub published: u64,
    pub last_use: u64,
    pub blocked: u64,
    pub rows: u64,
    pub row_calls: u64,
    pub zero_fans: u64,
    pub max_fans: u8,
}

#[derive(Default)]
pub struct Producer {
    owner: Option<rec::SourceOwner>,
    key: Option<rec::Key>,
    fans: u8,
    fan: u8,
    row: u8,
    offered: Option<u64>,
    end_sent: bool,
    pub stats: Stats,
}
impl Producer {
    pub fn idle(&self) -> bool {
        self.owner.is_none() && self.key.is_none() && self.offered.is_none()
    }
    pub fn held_word(&self) -> Option<u64> {
        self.offered
    }
    fn report(
        src: &source_capture::Controller,
        owner: rec::SourceOwner,
    ) -> Result<oracle::Report, String> {
        let snapshot = src.snapshot().ok_or("encoder has no actual snapshot")?;
        if snapshot.ticket != owner.ticket || snapshot.task.context != owner.context {
            return Err("encoder snapshot owner mismatch".into());
        }
        // This actual captured-row decoder is the only numerical DUT input.
        let input = snapshot.input()?;
        let report = oracle::run(&input, rr::profile())?;
        if report.clip_stages.len() > 5
            || report.clip_stages.iter().any(|s| s.polygon.len() > 8)
            || report.polygon.len() > 8
            || report.projected.len() > 8
            || report.triangles.len() > usize::from(rec::MAX_FANS)
            || report.snap_nonconvex
        {
            return Err("encoder oracle shape/nonconvex bound".into());
        }
        Ok(report)
    }
    pub fn action(&mut self, src: &source_capture::Controller) -> Result<rec::Input, String> {
        let mut action = rec::Input::default();
        let Some(owner) = self.owner else {
            return Ok(action);
        };
        if !self.end_sent {
            action.source_end = Some(rec::SourceEnd {
                source: owner,
                fans: self.fans,
            });
        }
        if self.fan < self.fans {
            if let Some(key) = self.key {
                if usize::from(self.row) < rr::WORDS {
                    if self.offered.is_none() {
                        let report = Self::report(src, owner)?;
                        if report.triangles.len() != usize::from(self.fans) {
                            return Err("held snapshot changed fan count".into());
                        }
                        let encoded = rr::Encoded::from_report(&report, usize::from(self.fan))?;
                        self.offered = Some(encoded.0[usize::from(self.row)]);
                        self.stats.row_calls += 1;
                        // report/input/encoded drop here. Exactly one word persists.
                    }
                    action.write = Some(rec::Write {
                        key,
                        row: usize::from(self.row),
                        word: self.offered.unwrap(),
                    });
                }
            } else {
                action.reserve = Some(rec::Reserve {
                    source: owner,
                    fan: self.fan,
                    rows: rr::WORDS,
                });
            }
        }
        Ok(action)
    }
    pub fn observe(
        &mut self,
        ce: bool,
        action: rec::Input,
        out: &link::Out,
        src: &source_capture::Controller,
    ) -> Result<(), String> {
        if ce && action.source_end.is_some() {
            self.end_sent = true;
        }
        if ce
            && action.reserve.is_some()
            && !out
                .transport
                .iter()
                .any(|e| matches!(e, rec::Event::Reserved(_)))
        {
            self.stats.blocked += 1;
        }
        for event in &out.transport {
            match event {
                rec::Event::SnapshotAccepted(owner) => {
                    if !self.idle() || self.stats.accepted == rr::MAX_SOURCES {
                        return Err("encoder source capacity".into());
                    }
                    let report = Self::report(src, *owner)?;
                    // Validate every fan before any writes, including later fans.
                    for fan in 0..report.triangles.len() {
                        rr::Encoded::from_report(&report, fan)?;
                    }
                    if self.stats.published + report.triangles.len() as u64 > rr::MAX_RECORDS {
                        return Err("encoder total record preflight bound".into());
                    }
                    self.fans = report.triangles.len() as u8;
                    self.fan = 0;
                    self.row = 0;
                    self.end_sent = false;
                    self.owner = Some(*owner);
                    self.stats.accepted += 1;
                    self.stats.zero_fans += u64::from(self.fans == 0);
                    self.stats.max_fans = self.stats.max_fans.max(self.fans);
                }
                rec::Event::Reserved(key) => {
                    if Some(key.source) != self.owner || key.fan != self.fan || self.key.is_some() {
                        return Err("encoder reserved identity".into());
                    }
                    self.key = Some(*key);
                }
                rec::Event::RowWritten { key, row } => {
                    if Some(*key) != self.key
                        || *row != usize::from(self.row)
                        || action.write.is_none_or(|w| {
                            w.key != *key || w.row != *row || Some(w.word) != self.offered
                        })
                    {
                        return Err("encoder row transfer identity".into());
                    }
                    self.offered = None;
                    self.row += 1;
                    self.stats.rows += 1;
                }
                rec::Event::Published(key) => {
                    if Some(*key) != self.key || usize::from(self.row) != rr::WORDS {
                        return Err("encoder publication identity".into());
                    }
                    self.key = None;
                    self.row = 0;
                    self.fan += 1;
                    self.stats.published += 1;
                    if self.stats.published > rr::MAX_RECORDS {
                        return Err("encoder record bound".into());
                    }
                }
                rec::Event::SnapshotLastUseAck(owner) => {
                    if Some(*owner) != self.owner
                        || self.fan != self.fans
                        || self.key.is_some()
                        || self.offered.is_some()
                        || !self.end_sent
                    {
                        return Err("encoder premature last use".into());
                    }
                    self.owner = None;
                    self.stats.last_use += 1;
                }
                _ => {}
            }
        }
        Ok(())
    }
}
