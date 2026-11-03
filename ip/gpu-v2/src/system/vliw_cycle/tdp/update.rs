use super::*;
impl Cache {
    fn monitor(&self, i: Input, e: &Evaluation) -> Result<(), ContractError> {
        if i.reset {
            return Ok(());
        }
        let s = &self.state;
        let check = |ok, rule| require(ok, "tdp", rule);
        check(!e.ingress_error && !e.return_bad, "GPU contract: fault_c")?;
        check(!e.extra_read, "GPU contract: fault_m")?;
        check(
            !(s.mt_active && i.mem_complete_valid) || i.mem_complete_ok,
            "GPU contract: mem_complete_ok",
        )?;
        for n in 0..2 {
            check(
                !s.ctx_valid[n] || s.ctx_line_mask[n] == 1 << s.ctx_line[n],
                "context line mask binding",
            )?;
        }
        check(
            !(e.depth_fire || e.color_fire_write)
                || (s.ctx_valid[usize::from(s.result_context)]
                    && e.result_line_mask == 1 << (s.result_address >> 5 & 7)),
            "dirty line mask lifetime",
        )?;
        for a in 0..8 {
            for b in a + 1..8 {
                check(
                    !(s.valid[a] && s.valid[b] && s.tag[a] == s.tag[b]),
                    "duplicate resident tag",
                )?;
            }
        }
        if e.row0_fire && !s.mt_active {
            check(!s.flushing, "flush admitted a new ROP owner")?;
            check(
                !(i.ce && i.row_valid && s.fill_row == 0 && !e.hit),
                "demand miss admitted a hit owner",
            )?;
            check(
                !(e.hint_start && e.sel_line == e.hit_line),
                "speculation stole same-edge owner",
            )?;
        }
        check(
            !(e.hint_start && e.pinned & (1 << e.sel_line) != 0),
            "unsafe speculative victim",
        )?;
        check(s.hint_count <= 2, "hint queue overflow")?;
        check(
            e.pinned.count_ones() <= 2 && e.pinned & (1 << e.select_line) == 0,
            "three-candidate victim proof violated",
        )?;
        check(
            !e.leaf_ce || (s.cal & 1 != 0) == i.shade_phase,
            "TDP/common shared DSP phase mismatch",
        )?;
        check(
            !(s.depth_pending && s.color_pending),
            "overlapping fixed result slots",
        )?;
        check(
            !e.o.shared_rop_ce || i.shade_phase,
            "ROP shared DSP wrong phase",
        )?;
        check(
            !s.mt_active || (s.mt_data <= e.data_end && s.mt_data >> 4 <= s.mt_issue),
            "framebuffer monotonic cursor proof",
        )?;
        if s.mt_active && i.mem_complete_valid {
            check(
                s.mt_retire < s.mt_issue,
                "framebuffer completion without accepted descriptor",
            )?;
            check(
                u16::from(s.mt_data) + u16::from(e.rbeat) + u16::from(e.wbeat)
                    >= u16::from(s.mt_retire + 1) * 16,
                "framebuffer completion before payload",
            )?;
        }
        check(
            !(s.mt_active && e.pinned & (1 << s.mt_line) != 0),
            "maintenance target is ROP pinned",
        )?;
        check(
            !(e.row0_fire && s.ctx_valid[usize::from(s.tail)]),
            "new owner overwrote a pinned context",
        )?;
        check(
            !(e.leaf_ce && s.fill_row & 1 != 0) || e.depth_input_fire,
            "paired source D missing after accepted C",
        )?;
        for n in 0..4 {
            check(
                !(i.serial.active && e.a[n].we && e.b[n].en && e.a[n].address == e.b[n].address),
                "UI half-buffer collision",
            )?;
            check(
                !(i.serial.active
                    && s.mt_active
                    && s.mt_write
                    && s.mt_issue >= 3
                    && !i.serial.lower_ready),
                "UI lower readiness withdrew after publication",
            )?;
            check(
                !(e.a_read[n] && (e.a_write_d[n] || e.a_write_c[n])),
                "TDP A static calendar collision",
            )?;
            check(
                !(e.a_write_d[n] && e.a_write_c[n]),
                "TDP D/C write collision",
            )?;
        }
        Ok(())
    }
    pub(super) fn update(&mut self, i: Input) -> Result<(), ContractError> {
        let e = self.evaluate(i);
        self.monitor(i, &e)?;
        let s = &self.state;
        let mut n = s.clone();
        if i.reset || i.prefetch_abort || i.flush_request || s.flushing {
            n.hint_count = 0;
            n.hints = [0; 2];
        } else {
            match (e.hint_pop, e.hint_push) {
                (false, true) => {
                    n.hints[usize::from(s.hint_count != 0)] = i.prefetch_tile & 1023;
                    n.hint_count = s.hint_count + 1;
                }
                (true, false) => {
                    n.hints[0] = s.hints[1];
                    n.hint_count = s.hint_count - 1;
                }
                (true, true) => {
                    if s.hint_count == 1 {
                        n.hints[0] = i.prefetch_tile & 1023;
                    } else {
                        n.hints = [s.hints[1], i.prefetch_tile & 1023];
                    }
                }
                _ => {}
            }
        }
        if i.reset {
            n.mt_active = false;
            n.mt_cancel = false;
            n.mt_request = false;
            n.mt_issue = 0;
            n.mt_retire = 0;
            n.mt_data = 0;
            n.wr_beat_valid = false;
            n.mt_remaining = 0;
            n.mt_write = false;
            n.mt_line = 0;
            n.mt_tvalid = false;
            n.mt_target = 0;
            n.flushing = false;
            n.flush_complete = false;
            n.commit_valid = false;
            n.victim = 0;
            n.cal = 0;
            n.fill_row = 0;
            n.tail = false;
            n.result_owner = false;
            n.return_key = 0;
            n.depth_pending = false;
            n.color_pending = false;
            n.ctx_valid = [false; 2];
            n.compare_bank = 0;
            n.mt_prefetch = false;
            n.protect_valid = false;
            n.protect_line = 0;
            n.valid = [false; 8];
            n.dirty_color = [false; 8];
            n.dirty_depth = [false; 8];
            n.tag = [0; 8];
        } else {
            n.commit_valid = false;
            if e.row0_fire && s.protect_valid && s.protect_line == e.hit_line {
                n.protect_valid = false;
            }
            if s.mt_active {
                if s.mt_prefetch && i.prefetch_abort {
                    n.mt_cancel = true;
                }
                if !s.mt_request
                    && s.mt_issue < e.mt_end
                    && !s.mt_cancel
                    && !(s.mt_prefetch && i.prefetch_abort)
                {
                    n.mt_request = true;
                }
                if e.req_accept {
                    n.mt_issue = (s.mt_issue + 1) & 7;
                    n.mt_request =
                        !e.issue_last && !s.mt_cancel && !(s.mt_prefetch && i.prefetch_abort);
                }
                if s.mt_cancel && s.mt_retire == s.mt_issue && !e.o.mem_req_valid {
                    n.mt_active = false;
                    n.mt_request = false;
                    n.wr_beat_valid = false;
                }
                if e.read_issue {
                    n.wr_beat_valid = true;
                }
                if e.wbeat || e.rbeat {
                    n.mt_data = (s.mt_data + 1) & 127;
                    if e.data_last || (s.mt_write && !e.read_issue) {
                        n.wr_beat_valid = false;
                    }
                }
                if i.mem_complete_valid {
                    n.mt_retire = (s.mt_retire + 1) & 7;
                    if s.mt_write && s.mt_retire & 1 != 0 {
                        if s.mt_retire & 2 != 0 {
                            n.dirty_depth[usize::from(s.mt_line)] = false;
                        } else {
                            n.dirty_color[usize::from(s.mt_line)] = false;
                        }
                    }
                    if e.retire_last
                        || (s.mt_cancel && s.mt_retire + 1 == s.mt_issue && !e.o.mem_req_valid)
                    {
                        if s.mt_write && s.mt_tvalid && !s.mt_cancel {
                            n.mt_write = false;
                            n.mt_remaining = 3;
                            n.mt_issue = 0;
                            n.mt_retire = 0;
                            n.mt_data = 0;
                            n.mt_request = false;
                            n.wr_beat_valid = false;
                            n.valid[usize::from(s.mt_line)] = false;
                        } else {
                            n.mt_active = false;
                            n.mt_request = false;
                            if !i.serial.active && !s.mt_write && !s.mt_cancel {
                                n.tag[usize::from(s.mt_line)] = s.mt_target;
                                n.valid[usize::from(s.mt_line)] = true;
                                if s.mt_prefetch && !i.prefetch_abort {
                                    n.protect_valid = true;
                                    n.protect_line = s.mt_line;
                                }
                            }
                        }
                    }
                }
            }
            if !i.flush_request && !s.flushing {
                n.flush_complete = false;
            }
            if i.flush_request && !s.flushing && !s.flush_complete {
                n.flushing = true;
                n.flush_complete = false;
            }
            if !s.mt_active {
                if i.serial.command_valid && e.o.serial_ready {
                    n.mt_active = true;
                    n.mt_cancel = false;
                    n.mt_request = false;
                    n.mt_prefetch = false;
                    n.mt_line = 0;
                    n.mt_tvalid = false;
                    n.mt_write = i.serial.writing;
                    n.mt_remaining = 1;
                    n.mt_issue = 0;
                    n.mt_retire = 0;
                    n.mt_data = 0;
                    n.wr_beat_valid = false;
                } else if s.flushing {
                    if let Some(line) = e.dirty_found {
                        let index = usize::from(line);
                        n.mt_active = true;
                        n.mt_cancel = false;
                        n.mt_request = false;
                        n.mt_prefetch = false;
                        n.mt_line = line;
                        n.mt_tvalid = false;
                        n.mt_write = true;
                        n.mt_remaining =
                            u8::from(s.dirty_depth[index]) << 1 | u8::from(s.dirty_color[index]);
                        n.mt_issue = if s.dirty_color[index] { 0 } else { 2 };
                        n.mt_retire = n.mt_issue;
                        n.mt_data = if s.dirty_color[index] { 0 } else { 32 };
                        n.wr_beat_valid = false;
                    } else if !e.any_dirty && !s.ctx_valid[0] && !s.ctx_valid[1] {
                        n.flushing = false;
                        n.flush_complete = true;
                        n.protect_valid = false;
                        n.valid = [false; 8];
                    }
                } else if i.ce && i.row_valid && s.fill_row == 0 && !e.hit || e.hint_start {
                    let demand = i.ce && i.row_valid && s.fill_row == 0 && !e.hit;
                    n.mt_active = true;
                    n.mt_cancel = false;
                    n.mt_request = false;
                    n.mt_prefetch = !demand;
                    n.mt_line = e.sel_line;
                    n.victim = (e.sel_line + 1) & 7;
                    n.mt_tvalid = true;
                    if s.protect_line == e.sel_line {
                        n.protect_valid = false;
                    }
                    n.mt_target = if demand { e.row_key } else { s.hints[0] };
                    n.mt_remaining = u8::from(e.sel_depth) << 1 | u8::from(e.sel_color);
                    n.mt_write = e.sel_dirty;
                    n.mt_issue = if e.sel_dirty && e.sel_plane { 2 } else { 0 };
                    n.mt_retire = n.mt_issue;
                    n.mt_data = if e.sel_dirty && e.sel_plane { 32 } else { 0 };
                    n.wr_beat_valid = false;
                    if !e.sel_dirty {
                        n.valid[usize::from(e.sel_line)] = false;
                    }
                } else if e.hint_unsafe {
                    n.victim = (e.sel_line + 1) & 7;
                }
            }
            if e.leaf_ce {
                n.cal = (s.cal + 1) & 3;
                n.depth_pending = e.return_fire;
                n.color_pending = s.depth_pending;
                if e.o.row_accept {
                    n.fill_row = (s.fill_row + 1) & 7;
                    if i.row_index & 1 == 0 {
                        n.source_rgb = i.data.slice(0, 24);
                    } else {
                        n.compare_bank = e.incoming_bank;
                        if i.row_index & 7 == 7 {
                            n.tail = !s.tail;
                        }
                    }
                    if e.row0_fire {
                        let owner = usize::from(s.tail);
                        n.ctx_valid[owner] = true;
                        n.ctx_line[owner] = e.hit_line;
                        n.ctx_line_mask[owner] = e.hits;
                        n.ctx_x[owner] = i.x & 511;
                        n.ctx_y[owner] = i.y;
                        n.ctx_mask[owner] = Bits::new(i.mask.into(), 4);
                    }
                }
                if e.return_fire {
                    let owner = usize::from(s.result_owner);
                    n.depth_data = e.ro.depth;
                    n.color_data = e.ro.color;
                    n.result_depth_flag = e.ro.depth_written;
                    n.result_color_flag = e.ro.color_written;
                    n.result_bank = s.return_key ^ (((s.ctx_x[owner] >> 1 & 1) as u8) << 1);
                    n.result_address = u16::from(s.ctx_line[owner]) << 5
                        | u16::from(s.ctx_y[owner] >> 1 & 3) << 3
                        | u16::from(s.return_key >> 1 & 1) << 2
                        | (s.ctx_x[owner] >> 2 & 3);
                    n.result_last = s.return_key == 3;
                    n.result_context = s.result_owner;
                    n.return_key = (s.return_key + 1) & 3;
                    if s.return_key == 3 {
                        n.result_owner = !s.result_owner;
                    }
                }
                for line in 0..8 {
                    if e.depth_fire
                        && s.result_depth_flag.is_known(1)
                        && s.result_depth_flag.value & 1 != 0
                        && e.result_line_mask & (1 << line) != 0
                    {
                        n.dirty_depth[line] = true;
                    }
                    if e.color_fire_write
                        && s.result_color_flag.is_known(1)
                        && s.result_color_flag.value & 1 != 0
                        && e.result_line_mask & (1 << line) != 0
                    {
                        n.dirty_color[line] = true;
                    }
                }
                if e.color_fire_write && s.result_last {
                    let owner = usize::from(s.result_context);
                    n.commit_valid = true;
                    n.commit_x = Bits::new(s.ctx_x[owner].into(), 9);
                    n.commit_y = Bits::new(s.ctx_y[owner].into(), 8);
                    n.commit_mask = s.ctx_mask[owner];
                    n.ctx_valid[owner] = false;
                }
            }
        }
        for bank in 0..4 {
            self.banks[bank].clock(i.reset, e.a[bank], e.b[bank])?;
        }
        self.rop.clock(e.ri);
        self.state = n;
        Ok(())
    }
}
