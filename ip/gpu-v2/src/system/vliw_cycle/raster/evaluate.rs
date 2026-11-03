use super::*;
fn or(a: Bits, b: Bits) -> Bits {
    Bits {
        value: a.value | b.value,
        known: (a.known & b.known) | (a.known & a.value) | (b.known & b.value),
    }
}
fn gated(a: Bits, on: bool) -> Bits {
    if on {
        a.slice(0, 30)
    } else {
        Bits::new(0, 30)
    }
}
impl Raster {
    pub(super) fn evaluate(&self, i: Input) -> Eval {
        let s = &self.state;
        let ph = s.phase;
        let u = self.uop;
        let op = ((u >> 27) & 15) as u8;
        let rop = ((s.r_uop >> 27) & 15) as u8;
        let root_level = match (i.bbox_xlo ^ i.bbox_xhi) | ((i.bbox_ylo ^ i.bbox_yhi) & 15) {
            n if n & 16 != 0 => 9,
            n if n & 8 != 0 => 8,
            n if n & 4 != 0 => 7,
            n if n & 2 != 0 => 6,
            n if n & 1 != 0 => 5,
            _ => 4,
        };
        let root_keep = match root_level {
            4 => 31,
            5 => 30,
            6 => 28,
            7 => 24,
            8 => 16,
            _ => 0,
        };
        let bbox_origin = [
            u16::from(i.bbox_xlo & root_keep) << 4,
            u16::from(i.bbox_ylo & root_keep) << 4,
        ];
        let tile_count = s.tile_tail.wrapping_sub(s.tile_head) & 7;
        let tile_empty = s.tile_head == s.tile_tail;
        let tile_full = tile_count == 2;
        let head_meta = s.tile_meta[usize::from(s.tile_head & 1)];
        let head_level = (head_meta >> 18) & 15;
        let tile_mask = if head_level >= 10 {
            1023
        } else {
            ((1u16 << head_level) - 1) & 1023
        };
        let tile_end = [
            (head_meta as u16 & 511) | tile_mask,
            ((head_meta >> 9) as u16 & 511) | tile_mask,
        ];
        let xbit = s.level < 9 && s.nx & (1 << s.level) != 0;
        let ybit = s.level < 9 && s.ny & (1 << s.level) != 0;
        let return_side = (1u16 << s.level) & 511;
        let node_mask = match s.level {
            9 => 31,
            8 => 15,
            7 => 7,
            6 => 3,
            5 => 1,
            _ => 0,
        };
        let outside = s.nx >= 400
            || s.ny >= 240
            || s.nx >> 4 > u16::from(i.bbox_xhi)
            || ((s.ny >> 4) & 15) > u16::from(i.bbox_yhi)
            || ((s.nx >> 4) | node_mask) < u16::from(i.bbox_xlo)
            || (((s.ny >> 4) & 15) | node_mask) < u16::from(i.bbox_ylo);
        let row_end = s.sx + 2 > s.end_x;
        let scan_end = row_end && s.sy + 2 > s.end_y;
        let check_finish = s.life == EXEC
            && s.scan_mode == CHECK
            && s.scan_inflight
            && s.s_kind == S_CHECK
            && ph == 3;
        let check_accept = s.check_mask != 0;
        let reject_tail = ph == 3
            && !s.scan_full
            && ((s.scan_classify_pending && !check_accept && scan_end)
                || (check_finish && (scan_end || (!check_accept && !row_end && s.sy & 2 != 0))));
        let scanner_finishing = s.scan_mode == OFF || reject_tail;
        let packet_take = i.start && !i.reset;
        let input_take = i.geo_write && !i.reset;
        let rom_match = s.rom_valid && s.rom_pc == s.w_pc;
        let early_prep_pc = s.w_pc != 14;
        let walk_allowed = (s.life == EXEC || (s.life == BOUND && early_prep_pc))
            && !s.walk_done
            && !(i.bbox_done && i.bbox_empty);
        let origin_op = op == 8;
        let fast_origin = origin_op && s.nx == 0 && s.ny == 0;
        let enqueue_block = op == 4 && s.level <= 4 && s.w_edge == 0 && tile_full;
        let fast_w = walk_allowed
            && rom_match
            && s.w_edge == 0
            && ((op == 7 && s.level == root_level) || (op == 4 && outside) || fast_origin);
        let cancelled = s.r_valid && s.r_epoch != s.w_epoch;
        let r_origin = rop == 8;
        let origin_issue = s.r_valid && r_origin && !cancelled;
        let origin_retire = s.origin_valid & 2 != 0;
        let origin_finish = origin_retire && s.origin_edge1 == 2;
        let micro_issue = walk_allowed
            && rom_match
            && s.w_edge < 3
            && !enqueue_block
            && !fast_w
            && (origin_op || !(origin_issue || s.origin_valid != 0) || origin_finish);
        let mut micro_step = ((u >> 6) & 63) as u8;
        let mut micro_sub = (u >> 20) & 3 == 1;
        match (u >> 25) & 3 {
            1 => micro_step = micro_step.wrapping_add(s.level).wrapping_sub(1) & 63,
            2 => {
                micro_step = ((if xbit == ybit { 32 } else { 48 }) + s.level) & 63;
                micro_sub = ybit;
            }
            3 => micro_step = micro_step.wrapping_add(s.prep_level).wrapping_sub(1) & 63,
            _ => {}
        }
        if origin_op {
            micro_step = 62;
        }
        let micro_tw =
            (((u >> 12) & 63) as u8 + if u & (1 << 24) != 0 { s.prep_level } else { 0 }) & 63;
        let w_retire = s.r_valid && !cancelled && !r_origin;
        let w_cond_retire = w_retire && matches!(rop, 4 | 5);
        let double_step = rop == 14;
        let work_live = !double_step
            && !((s.r_uop >> 18) & 3 == 2 && self.work.q.value & (1 << 29) == 0)
            && !((s.r_uop >> 18) & 3 == 3 && self.work.q.value & (1 << 29) != 0);
        let step = self.steps.wq.slice(0, 30);
        let step_live = !((s.r_uop >> 20) & 3 == 2 && step.value & (1 << 29) == 0)
            && !((s.r_uop >> 20) & 3 == 3 && step.value & (1 << 29) != 0);
        let formatted_work = if (s.r_uop >> 18) & 3 == 1 {
            Bits::concat(self.work.q.slice(0, 29), Bits::new(0, 1), 1)
        } else {
            self.work.q
        };
        let fmt_a = if double_step {
            step
        } else {
            gated(formatted_work, work_live)
        };
        let fmt_b = gated(step, step_live);
        let value = alu(fmt_a, fmt_b, s.r_sub);
        let cond_pass = pass(value, s.tau & (1 << s.r_edge) != 0);
        let w_early = w_cond_retire && !cond_pass;
        let w_decide = w_cond_retire && (s.r_edge == 2 || w_early);
        let w_static_tail = micro_issue && s.w_edge == 2 && !matches!(op, 4 | 5);
        let all_pass = s.w_pass == 3 && cond_pass;
        let branch_all_pass = s.r_edge == 2 && all_pass;
        let terminal_tile = (branch_all_pass && s.level <= 4) || s.level <= 2;
        let upper_split = branch_all_pass && s.level > 4;
        let branch_finish = if s.r_uop & (1 << 27) != 0 {
            terminal_tile
        } else {
            !branch_all_pass
        };
        let branch_low = (s.r_uop & (1 << 27) == 0 && !upper_split)
            || (s.r_uop & (1 << 27) != 0 && terminal_tile);
        let branch_split = !branch_finish && !branch_low;
        let branch_target = 8
            | (u8::from(branch_finish) << 2)
            | (u8::from(!branch_finish) << 1)
            | u8::from(branch_low);
        let tile_publish = w_decide && s.r_uop & (1 << 27) != 0 && terminal_tile;
        let rf_we = input_take || origin_retire || (w_retire && s.r_uop & (1 << 22) != 0);
        let rf_wa = if input_take {
            i.geo_address
        } else if origin_retire {
            8 | s.origin_edge1
        } else {
            ((((s.r_uop >> 3) & 3) as u8) << 2) | s.r_edge
        };
        let rf_wd = or(
            or(
                gated(i.geo_data, input_take),
                gated(self.origin.result, origin_retire),
            ),
            gated(value, w_retire),
        );
        let table_we = w_retire && s.r_uop & (1 << 23) != 0;
        let mailbox_write = w_retire && rop == 4 && s.level <= 4;
        let scan_cover_valid = s.s_valid
            && s.s_kind == S_SCAN
            && (s.s_return_edge != 0
                || (s.life == EXEC
                    && s.scan_mode == SCAN
                    && ph == 0
                    && s.scan_qvalid
                    && (s.lane != 0 || i.quad_ready && i.run_enable)))
            && s.s_return_edge == 2
            && s.scan_inflight
            && ph == 2;
        let prefetch_handoff = s.prefetch_active && scan_cover_valid && s.lane == 3 && scan_end;
        let start_tile = s.life == EXEC && (scanner_finishing || prefetch_handoff) && !tile_empty;
        let preload_issue = start_tile && !prefetch_handoff && ph != 0;
        let load_issue = preload_issue || (s.life == EXEC && s.scan_mode == LOAD && s.s_edge < 3);
        let load_edge = if preload_issue { 0 } else { s.s_edge };
        let prefetch_issue = s.life == EXEC
            && s.scan_mode == SCAN
            && s.scan_inflight
            && ph == 2
            && s.lane < 3
            && s.prefetch_active;
        let checking = !s.scan_full && matches!(s.scan_mode, CHECK | ALIGN);
        let current_vec = if s.scan_bank { 2 } else { 1 };
        let future_vec = if s.scan_bank { 1 } else { 2 };
        let prefetch_vec = if head_meta & (1 << 22) != 0 {
            0
        } else if s.scan_full {
            2
        } else {
            future_vec
        };
        let check_advance = check_finish && !reject_tail;
        let fetch_check = (checking || s.scan_classify_pending) && !(ph == 3 && check_accept);
        let fetch_row_end = if check_advance { !row_end } else { row_end };
        let quad_valid = s.life == EXEC
            && s.scan_mode == SCAN
            && s.lane == 0
            && ph == 0
            && s.scan_qvalid
            && i.run_enable
            && !i.reset;
        let quad_take = quad_valid && i.quad_ready;
        let scan_start = s.life == EXEC
            && s.scan_mode == SCAN
            && ph == 0
            && s.scan_qvalid
            && (s.lane != 0 || quad_take);
        let prefetch_start = scan_start && s.lane == 0 && scan_end && !tile_empty;
        let check_start = s.life == EXEC && s.scan_mode == CHECK && ph == 0 && s.scan_qvalid;
        let scan_fetch0 = s.life == EXEC
            && matches!(s.scan_mode, SCAN | ALIGN | CHECK)
            && ph == 3
            && !reject_tail;
        let scan_fetch1 = scan_start || check_start;
        let scan_fetch2 =
            s.life == EXEC && matches!(s.scan_mode, SCAN | CHECK) && s.scan_inflight && ph == 1;
        let scan_read = scan_fetch0 || scan_fetch1 || scan_fetch2;
        let scan_read_edge = if ph == 2 { s.lane } else { (ph + 1) & 3 };
        let row_issue = s.life == EXEC
            && s.scan_full
            && s.scan_mode == SCAN
            && s.scan_inflight
            && ph == 2
            && row_end
            && !scan_end
            && s.lane != 3;
        let scan_rf_read = scan_read || row_issue;
        let read_vec = if s.scan_full {
            u8::from(ph != 2 && !(s.row_first && s.lane == 0))
        } else if check_advance {
            future_vec
        } else {
            current_vec
        };
        let step_address = if fetch_check {
            if fetch_row_end {
                58
            } else {
                33
            }
        } else if !s.scan_full && row_end && s.lane == 3 {
            59
        } else if ph == 2 {
            49
        } else if s.lane & 1 != 0 {
            57
        } else {
            32
        };
        let s_scan_retire = s.s_valid && s.s_kind == S_SCAN && (s.s_return_edge != 0 || scan_start);
        let s_check_retire =
            s.s_valid && s.s_kind == S_CHECK && (s.s_return_edge != 0 || check_start);
        let s_row_retire = s.s_valid && s.s_kind == S_ROW;
        let s_load_retire = s.s_valid && s.s_kind == S_LOAD;
        let s_prefetch_retire = s.s_valid && s.s_kind == S_PREFETCH;
        let scan_alu = alu(self.scan.q, self.steps.sq, s.s_sub);
        let scan_write =
            s_scan_retire || s_check_retire || s_row_retire || s_load_retire || s_prefetch_retire;
        let write_vec = if s.s_kind == S_PREFETCH {
            prefetch_vec
        } else if s.s_kind == S_CHECK {
            future_vec
        } else if s.s_kind == S_LOAD {
            if s.scan_full {
                0
            } else {
                1
            }
        } else if s.s_kind == S_ROW {
            0
        } else if s.scan_full {
            1
        } else {
            current_vec
        };
        let scan_wd = if matches!(s.s_kind, S_LOAD | S_PREFETCH) {
            self.mailbox.q
        } else {
            scan_alu
        };
        let half = super::super::compute::math::signed(scan_wd.value & 0xfffff, 20) >> 1;
        let nx = super::super::compute::math::signed(self.nx.q.value, 18);
        let ny = super::super::compute::math::signed(self.ny.q.value, 18);
        let corner_x = half + nx;
        let corner_y = half + ny;
        let corner_xy = corner_x + ny;
        let inclusive = !s.check_tau || scan_wd.value & 1 != 0;
        let corner_pass = |v: i64| {
            let v = v as u64 & 0xfffff;
            v & (1 << 19) == 0 && (v & 0x7ffff != 0 || inclusive)
        };
        let corner_window = (scan_wd.value >> 20) & 1023
            == if scan_wd.value & (1 << 19) != 0 {
                1023
            } else {
                0
            };
        let next_quad_mask = if corner_window {
            u8::from(pass(scan_wd, s.check_tau))
                | (u8::from(corner_pass(corner_x)) << 1)
                | (u8::from(corner_pass(corner_y)) << 2)
                | (u8::from(corner_pass(corner_xy)) << 3)
        } else if scan_wd.value & (1 << 29) == 0 {
            15
        } else {
            0
        };
        let predicate_write = s_load_retire
            || s_prefetch_retire
            || s_check_retire
            || (s_scan_retire && !s.scan_full && s.lane == 3 && !scan_end);
        let threshold_read = scan_rf_read || load_issue || prefetch_issue;
        let threshold_edge = if load_issue {
            load_edge
        } else if prefetch_issue {
            s.lane
        } else {
            scan_read_edge
        };
        let sample_cover = s.scan_full || (s.cover_acc && pass(self.scan.q, s.tau & 4 != 0));
        let quad_end_valid = scan_cover_valid && s.lane == 3 && !i.reset;
        let quad_mask = s.cover_mask | (u8::from(sample_cover) << s.lane);
        let prep_last = if s.life == BOUND { 8 } else { root_level - 1 };
        let mut issue_next_pc = ((u >> 31) & 31) as u8;
        match op {
            3 if s.prep_level < prep_last => issue_next_pc = 2,
            2 | 14 if s.w_pc != 17 && s.life == EXEC && s.prep_level > prep_last => {
                issue_next_pc = 14
            }
            7 if !xbit && ybit => issue_next_pc = 13,
            _ => {}
        }
        if s.w_pc == 4 && s.prep_level > 3 && issue_next_pc != 14 {
            issue_next_pc = 9;
        }
        let mut rom_read = false;
        let mut rom_address = s.w_pc;
        if (s.life == EXEC || (s.life == BOUND && early_prep_pc)) && !rom_match {
            rom_read = true;
        }
        if w_static_tail {
            rom_read = true;
            rom_address = issue_next_pc;
        }
        if w_decide {
            rom_read = true;
            rom_address = branch_target;
        }
        if fast_w {
            if !origin_op {
                rom_read = true;
                rom_address = 13;
            } else if s.w_pc != 10 {
                rom_read = true;
                rom_address = 10;
            }
        }
        if s.life == IMPORT && input_take && i.geo_address == 10 {
            rom_read = true;
            rom_address = 5;
        }
        let work = RfPorts {
            reset: i.reset,
            ce: i.ce,
            read: micro_issue,
            write: rf_we,
            ra: (((u & 3) as u8) << 2) | s.w_edge,
            wa: rf_wa,
            data: rf_wd,
        };
        let mailbox = RfPorts {
            reset: i.reset,
            ce: i.ce,
            read: load_issue || prefetch_issue,
            write: mailbox_write,
            ra: if prefetch_issue {
                (s.tile_head & 1) * 4 + s.lane
            } else if preload_issue {
                (s.tile_head & 1) * 4
            } else {
                u8::from(s.scan_slot) * 4 + s.s_edge
            },
            wa: (s.tile_tail & 1) * 4 + s.r_edge,
            data: self.work.q,
        };
        let scan = RfPorts {
            reset: i.reset,
            ce: i.ce,
            read: scan_rf_read,
            write: scan_write,
            ra: read_vec * 4 + scan_read_edge,
            wa: write_vec * 4 + s.s_return_edge,
            data: scan_wd,
        };
        let nx = RfPorts {
            reset: i.reset,
            ce: i.ce,
            read: threshold_read,
            write: input_take && i.geo_address & 12 == 0,
            ra: threshold_edge,
            wa: i.geo_address & 3,
            data: i.geo_data.slice(1, 18),
        };
        let ny = RfPorts {
            write: input_take && i.geo_address & 8 == 0 && i.geo_address & 4 != 0,
            ..nx
        };
        let steps = StepPorts {
            reset: i.reset,
            ce: i.ce,
            w_read: micro_issue,
            w_address: s.w_edge * 64 + micro_step,
            write: table_we || (input_take && i.geo_address & 8 == 0),
            address: if input_take {
                (i.geo_address & 3) * 64 + if i.geo_address & 4 != 0 { 48 } else { 32 }
            } else {
                s.r_tw
            },
            data: rf_wd,
            n_write: input_take && i.geo_address & 8 == 0,
            n_axis: i.geo_address & 4 != 0,
            n_edge: i.geo_address & 3,
            n_data: i.geo_data.slice(1, 18),
            s_read: scan_rf_read,
            s_address: scan_read_edge * 64 + step_address,
        };
        let o = Output {
            quad_valid,
            quad_take,
            quad_x: ((s.sx >> 1) & 255) as u8,
            quad_y: ((s.sy >> 1) & 127) as u8,
            sample_valid: scan_start && !i.reset,
            sample_lane: s.lane,
            sample_h: self.scan.q,
            sample_cover,
            sample_anchor: sample_cover && !s.anchor_seen,
            quad_end_valid,
            quad_mask,
            busy: s.busy,
            done: s.done,
            full_scan: s.scan_full,
            evt_import: s.life == IMPORT,
            evt_prep: s.life == BOUND
                || (s.life == EXEC
                    && ((!s.walk_done && (s.w_pc < 10 || s.w_pc >= 14))
                        || (s.r_valid && (s.r_pc < 10 || s.r_pc >= 14)))),
            evt_traverse: s.life == EXEC
                && ((micro_issue && (10..=13).contains(&s.w_pc))
                    || (w_retire && (10..=13).contains(&s.r_pc))
                    || (fast_w && !origin_op)),
            evt_node: (micro_issue && s.w_pc == 10 && s.w_edge == 0) || (fast_w && op == 4),
            evt_row: load_issue
                || row_issue
                || s_load_retire
                || s_row_retire
                || prefetch_issue
                || s_prefetch_retire,
            evt_align: s.life == EXEC
                && (s.scan_mode == ALIGN
                    || (s.scan_mode == SCAN
                        && s.lane == 0
                        && !s.scan_inflight
                        && !scan_start
                        && i.run_enable
                        && i.quad_ready)),
            evt_credit_wait: s.life == EXEC
                && s.scan_mode == SCAN
                && s.lane == 0
                && !s.scan_inflight
                && !scan_start
                && (!i.run_enable || !i.quad_ready),
            evt_leaf: scan_start || check_start || s.scan_inflight,
            evt_empty_quad: quad_end_valid && quad_mask == 0,
        };
        Eval {
            o,
            work,
            mailbox,
            scan,
            nx,
            ny,
            steps,
            packet_take,
            input_take,
            root_level,
            bbox_origin,
            tile_count,
            tile_empty,
            tile_full,
            head_meta,
            tile_end,
            row_end,
            scan_end,
            check_finish,
            check_accept,
            reject_tail,
            scanner_finishing,
            rom_read,
            rom_address,
            origin_op,
            xbit,
            ybit,
            return_side,
            fast_w,
            micro_issue,
            micro_sub,
            micro_tw,
            w_retire,
            w_cond_retire,
            cond_pass,
            w_static_tail,
            w_decide,
            origin_issue,
            origin_retire,
            start_tile,
            preload_issue,
            load_issue,
            load_edge,
            prefetch_issue,
            fetch_check,
            scan_read,
            row_issue,
            scan_start,
            check_start,
            scan_fetch0,
            scan_rf_read,
            scan_read_edge,
            scan_cover_valid,
            prefetch_handoff,
            prefetch_start,
            prefetch_vec,
            s_load_retire,
            predicate_write,
            next_quad_mask,
            threshold_read,
            threshold_edge,
            prep_last,
            issue_next_pc,
            branch_target,
            branch_split,
            branch_all_pass,
            tile_publish,
            mailbox_write,
            origin_n: [self.steps.wq.slice(0, 18), self.steps.wq.slice(18, 18)],
            origin_xy: [u32::from(s.nx) * 2, u32::from(s.ny) * 2],
            origin_h: self.work.q.extend(30, 54),
        }
    }
}
