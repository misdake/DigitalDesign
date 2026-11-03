use super::*;
impl Raster {
    pub(super) fn update(&mut self, i: Input) -> Result<(), ContractError> {
        let e = self.evaluate(i);
        let old = self.state.clone();
        let check = |ok, rule| require(ok, "raster", rule);
        if !i.reset {
            check(
                old.phase == i.phase,
                "Raster phase replica differs from producer",
            )?;
        }
        if !i.reset && i.ce {
            check(
                !(e.origin_retire && (e.input_take || (e.w_retire && old.r_uop & (1 << 22) != 0))),
                "origin and walker RF writers overlap",
            )?;
            check(
                !(old.life == BOUND && e.micro_issue)
                    || old.w_pc <= 9
                    || matches!(old.w_pc, 16..=18),
                "bbox-dependent instruction before bounds",
            )?;
            check(
                !(old.r_valid && (old.r_uop >> 27) & 15 == 14)
                    || ((old.r_uop >> 20) & 3 == 0 && !old.r_sub),
                "step doubling B source is not fixed by ROM",
            )?;
            check(
                !i.start || old.life == IDLE,
                "T configuration before Raster idle",
            )?;
            check(
                !i.geo_write || old.life == IMPORT,
                "geometry write outside IMPORT",
            )?;
            check(
                !(e.s_load_retire && old.s_return_edge == 2 && old.phase == 3),
                "third LOAD return reached predicate branch edge",
            )?;
            if i.geo_write {
                check(
                    old.sim_geo_count < 9
                        && i.geo_address == (old.sim_geo_count % 3) * 4 + old.sim_geo_count / 3,
                    "final geometry write order",
                )?;
                check(i.geo_data.is_known(30), "unknown geometry at import")?;
            }
            if e.start_tile {
                let level = (e.head_meta >> 18) & 15;
                let mask = if level >= 10 {
                    1023
                } else {
                    (1u32 << level) - 1
                };
                check(level <= 9, "invalid tile level")?;
                check(
                    e.head_meta & (1 << 22) != 0 || level == 2,
                    "partial tile must be 4x4",
                )?;
                check(
                    e.head_meta & 511 & mask == 0 && ((e.head_meta >> 9) & 511) & mask == 0,
                    "tile origin is not aligned to its level",
                )?;
            }
            if e.load_issue || e.prefetch_issue {
                let slot = if e.prefetch_issue || e.preload_issue {
                    old.tile_head & 1
                } else {
                    u8::from(old.scan_slot)
                };
                let edge = if e.prefetch_issue {
                    old.lane
                } else {
                    e.load_edge
                };
                check(
                    old.mail_written[slot as usize] & (1 << edge) != 0,
                    "mailbox read before statically scheduled write",
                )?;
            }
            check(
                !(e.tile_publish && e.tile_full),
                "GPU contract: error (mailbox full)",
            )?;
            check(e.tile_count <= 2, "GPU contract: error (mailbox ownership)")?;
            check(
                !(e.scan_read && e.row_issue),
                "GPU contract: error (scan calendar)",
            )?;
            if e.w_cond_retire {
                check(e.work.data.is_known(30), "unknown walker predicate")?;
            }
            if e.predicate_write {
                check(
                    e.scan.data.is_known(30) && self.nx.q.is_known(18) && self.ny.q.is_known(18),
                    "unknown scanner predicate",
                )?;
            }
            if e.o.sample_valid || e.scan_cover_valid {
                check(self.scan.q.is_known(30), "unknown emitted sample")?;
            }
        }
        if i.reset {
            self.state = State {
                level: 9,
                ..Default::default()
            };
            self.uop = 0;
        } else if i.ce {
            let s = &mut self.state;
            s.phase = (i.phase + 1) & 3;
            s.done = false;
            s.origin_valid = ((old.origin_valid << 1) | u8::from(e.origin_issue)) & 3;
            s.origin_edge1 = old.origin_edge0;
            if e.origin_issue {
                s.origin_edge0 = old.r_edge;
            }
            s.scan_classify_pending =
                e.scan_cover_valid && old.lane == 3 && !old.scan_full && !e.scan_end;
            if e.rom_read {
                s.rom_valid = true;
                s.rom_pc = e.rom_address;
            }
            s.r_valid = e.micro_issue;
            if e.micro_issue {
                s.r_edge = old.w_edge;
                s.r_sub = e.micro_sub;
                s.r_tw = old.w_edge * 64 + e.micro_tw;
                s.r_uop = self.uop;
                s.r_pc = old.rom_pc;
                s.r_epoch = old.w_epoch;
            }
            s.s_valid = e.load_issue || e.scan_rf_read || e.prefetch_issue;
            if s.s_valid {
                s.s_kind = if e.load_issue {
                    S_LOAD
                } else if e.prefetch_issue {
                    S_PREFETCH
                } else if e.fetch_check {
                    S_CHECK
                } else if old.phase == 2 {
                    S_ROW
                } else {
                    S_SCAN
                };
                s.s_return_edge = if e.load_issue {
                    e.load_edge
                } else if e.prefetch_issue {
                    old.lane
                } else {
                    e.scan_read_edge
                };
                s.s_sub = !e.fetch_check && old.phase != 2 && old.lane == 3;
            }
            if e.packet_take {
                s.life = IMPORT;
                s.busy = true;
                s.walk_done = false;
                s.w_pc = 5;
                s.w_edge = 0;
                s.prep_level = 1;
                s.tau = 0;
                s.w_epoch = false;
                s.row_first = false;
                s.tile_head = 0;
                s.tile_tail = 0;
                s.scan_mode = OFF;
                s.scan_inflight = false;
                s.scan_qvalid = false;
                s.prefetch_active = false;
                s.scan_classify_pending = false;
            }
            if e.input_take && i.geo_address == 10 {
                s.life = BOUND;
                s.w_pc = 5;
                s.w_edge = 0;
            }
            if e.micro_issue {
                s.w_edge = (old.w_edge + 1) & 3;
            }
            if e.w_retire {
                if e.w_cond_retire {
                    s.w_pass = if old.r_edge == 0 {
                        u8::from(e.cond_pass)
                    } else {
                        ((old.w_pass << 1) | u8::from(e.cond_pass)) & 3
                    };
                }
                if (old.r_uop >> 18) & 3 == 2 && old.r_uop & (1 << 23) != 0 {
                    let work = self.work.q.value & 0x3fffffff;
                    let step = self.steps.wq.value & 0x3fffffff;
                    let exclusive = !((work & (1 << 29) == 0 && work != 0)
                        || (work == 0 && step & (1 << 29) == 0 && step != 0));
                    s.tau = (old.tau & !(1 << old.r_edge)) | (u8::from(exclusive) << old.r_edge);
                }
            }
            if e.tile_publish {
                s.tile_meta[usize::from(old.tile_tail & 1)] = (u32::from(e.branch_all_pass) << 22)
                    | (u32::from(old.level) << 18)
                    | (u32::from(old.ny) << 9)
                    | u32::from(old.nx);
                s.tile_tail = (old.tile_tail + 1) & 7;
            }
            if e.w_static_tail {
                s.w_edge = 0;
                s.w_pc = e.issue_next_pc;
                match (self.uop >> 27) & 15 {
                    1 => s.prep_level = 1,
                    3 if old.prep_level < e.prep_last => s.prep_level = (old.prep_level + 1) & 15,
                    7 => {
                        if e.xbit == e.ybit {
                            s.nx = old.nx ^ e.return_side;
                        } else {
                            s.ny = old.ny ^ e.return_side;
                        }
                        if !e.xbit && e.ybit {
                            s.level = (old.level + 1) & 15;
                        }
                    }
                    _ => {}
                }
            }
            if e.w_decide {
                s.w_epoch = !old.w_epoch;
                s.w_pass = 0;
                s.w_pc = e.branch_target;
                s.w_edge = 0;
                if e.branch_split {
                    s.level = old.level.wrapping_sub(1) & 15;
                }
            }
            if e.fast_w {
                s.w_edge = 0;
                if e.origin_op {
                    s.w_pc = 10;
                } else if (self.uop >> 27) & 15 == 7 {
                    s.walk_done = true;
                } else {
                    s.w_pc = 13;
                }
            }
            if i.bbox_done && old.life == BOUND {
                s.life = EXEC;
                if i.bbox_empty {
                    s.walk_done = true;
                } else {
                    s.nx = e.bbox_origin[0];
                    s.ny = e.bbox_origin[1];
                    s.level = e.root_level;
                }
            }
            if e.start_tile {
                s.scan_slot = old.tile_head & 1 != 0;
                s.scan_full = e.head_meta & (1 << 22) != 0;
                s.origin_x = (e.head_meta & 511) as u16;
                s.sx = (e.head_meta & 511) as u16;
                s.sy = ((e.head_meta >> 9) & 511) as u16;
                s.end_x = e.tile_end[0].min(399);
                s.end_y = e.tile_end[1].min(239);
                s.scan_mode = if e.prefetch_handoff { ALIGN } else { LOAD };
                s.s_edge = u8::from(e.preload_issue);
                s.lane = 0;
                s.scan_qvalid = false;
                s.anchor_seen = false;
                s.cover_mask = 0;
                s.scan_bank = e.prefetch_handoff && e.prefetch_vec & 2 != 0;
                s.prefetch_active = false;
                if e.prefetch_handoff {
                    s.row_first = true;
                }
            }
            if e.prefetch_start {
                s.prefetch_active = true;
            }
            if e.load_issue && !e.preload_issue {
                s.s_edge = (old.s_edge + 1) & 3;
                if old.s_edge == 2 {
                    s.s_edge = 0;
                    s.scan_mode = ALIGN;
                    s.lane = 0;
                    s.scan_qvalid = false;
                    s.row_first = true;
                }
            }
            if (e.s_load_retire && old.s_return_edge == 2) || e.prefetch_handoff {
                s.tile_head = (old.tile_head + 1) & 7;
            }
            if e.scan_fetch0 {
                s.scan_qvalid = true;
                if old.scan_mode == ALIGN {
                    s.scan_mode = if old.scan_full || e.check_accept {
                        SCAN
                    } else {
                        CHECK
                    };
                }
            }
            if old.scan_classify_pending && !e.check_accept && !e.reject_tail {
                s.scan_mode = CHECK;
            }
            if e.check_start {
                s.scan_qvalid = false;
                s.scan_inflight = true;
            }
            if e.predicate_write {
                s.check_mask = if old.s_return_edge == 0 {
                    e.next_quad_mask
                } else {
                    old.check_mask & e.next_quad_mask
                };
            }
            if e.check_finish {
                if e.reject_tail {
                    if !e.start_tile {
                        s.scan_mode = OFF;
                    }
                } else {
                    s.scan_bank = !old.scan_bank;
                    if e.row_end {
                        s.sx = old.origin_x;
                        s.sy = (old.sy + 2) & 511;
                    } else {
                        s.sx = (old.sx + 2) & 511;
                    }
                    if e.check_accept {
                        s.scan_mode = SCAN;
                    }
                }
            }
            if e.scan_start {
                s.scan_qvalid = false;
                s.scan_inflight = true;
                s.cover_acc = pass(self.scan.q, old.tau & 1 != 0);
                if old.lane == 0 {
                    s.cover_mask = 0;
                    s.anchor_seen = false;
                }
            }
            if old.scan_inflight && old.phase == 1 {
                s.cover_acc = old.cover_acc && pass(self.scan.q, old.tau & 2 != 0);
            }
            if e.scan_cover_valid {
                if old.lane == 0 {
                    s.row_first = false;
                }
                // A preceding whole-vector clear still applies to other lanes.
                // The RTL's later partial NBA replaces only this lane.
                s.cover_mask =
                    (s.cover_mask & !(1 << old.lane)) | (u8::from(e.o.sample_cover) << old.lane);
                if e.o.sample_cover {
                    s.anchor_seen = true;
                }
                if old.lane != 3 {
                    s.lane = old.lane + 1;
                }
            }
            if e.scan_cover_valid && old.lane == 3 {
                s.lane = 0;
                if e.scan_end {
                    if !e.prefetch_handoff {
                        s.scan_mode = OFF;
                    }
                } else if e.row_end {
                    s.sx = old.origin_x;
                    s.sy = (old.sy + 2) & 511;
                    s.row_first = true;
                } else {
                    s.sx = (old.sx + 2) & 511;
                }
            }
            if e.reject_tail && !e.start_tile {
                s.scan_mode = OFF;
            }
            if old.phase == 3 {
                s.scan_inflight = false;
            }
            if old.life == EXEC
                && old.walk_done
                && e.tile_empty
                && e.scanner_finishing
                && !old.r_valid
                && !old.s_valid
            {
                s.life = IDLE;
                s.busy = false;
                s.done = true;
            }
            if e.threshold_read {
                s.check_tau = old.tau & (1 << e.threshold_edge) != 0;
            }
            if i.start {
                s.sim_geo_count = 0;
            }
            if i.geo_write {
                s.sim_geo_count = old.sim_geo_count + 1;
            }
            if e.packet_take {
                s.mail_written = [0; 2];
            }
            if e.mailbox_write {
                let row = usize::from(old.tile_tail & 1);
                s.mail_written[row] = if old.r_edge == 0 {
                    1
                } else {
                    old.mail_written[row] | (1 << old.r_edge)
                };
            }
            if e.rom_read {
                self.uop = program()[usize::from(e.rom_address)];
            }
        }
        // Primitives receive pre-edge snapshots regardless of state-update order.
        self.origin
            .clock(i.reset, i.ce, e.origin_n, e.origin_xy, e.origin_h);
        let storage = [
            self.work.clock(e.work),
            self.mailbox.clock(e.mailbox),
            self.scan.clock(e.scan),
            self.nx.clock(e.nx),
            self.ny.clock(e.ny),
            self.steps.clock(e.steps),
        ];
        for result in storage {
            result?;
        }
        Ok(())
    }
}
