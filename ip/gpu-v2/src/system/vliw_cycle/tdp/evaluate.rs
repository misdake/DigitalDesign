use super::*;
fn one(v: Bits) -> bool {
    v.is_known(1) && v.value & 1 != 0
}
impl Cache {
    pub(super) fn evaluate(&self, i: Input) -> Evaluation {
        let s = &self.state;
        let mut e = Evaluation::default();
        e.mt_end = if s.mt_write && s.mt_remaining & 2 == 0 && !i.serial.active {
            2
        } else {
            4
        };
        e.data_end = e.mt_end << 4;
        e.row_key = ((i.tile >> 5) & 15) << 6 | u16::from((i.y >> 3) & 1) << 5 | (i.tile & 31);
        for n in 0..8 {
            if s.valid[n] && !(s.mt_active && usize::from(s.mt_line) == n) && s.tag[n] == e.row_key
            {
                e.hits |= 1 << n;
            }
        }
        e.hit = e.hits != 0;
        e.hit_line = u8::from(e.hits & 0xf0 != 0) << 2
            | u8::from(e.hits & 0xcc != 0) << 1
            | u8::from(e.hits & 0xaa != 0);
        let row_credit = !s.ctx_valid[usize::from(s.tail)] || s.fill_row != 0;
        let aligned = s.cal & 3 == i.row_index & 3;
        let source_ready = aligned && (s.fill_row != 0 || (!s.flushing && e.hit));
        e.ingress_error = i.ce
            && !i.reset
            && i.row_valid
            && row_credit
            && (i.row_index & 7 != s.fill_row
                || i.row_index & 1 != 0 && i.data.value & i.data.known & 0xffff0000 != 0
                || s.fill_row == 0
                    && (i.x & 1 != 0
                        || i.y & 1 != 0
                        || u32::from(i.x) + 1 >= 400
                        || u16::from(i.y) + 1 >= 240
                        || i.mask & 15 == 0));
        e.extra_read = i.mem_read_valid
            && (!s.mt_active
                || s.mt_write
                || s.mt_data >= e.data_end
                || s.mt_data >> 4 >= s.mt_issue);
        let actual_ce = i.ce && i.commit_allow;
        e.leaf_ce = actual_ce && !i.reset;
        e.return_bad = i.ce
            && !i.reset
            && self.rop.state.output_valid
            && (!s.ctx_valid[usize::from(s.result_owner)] || s.depth_pending || !i.shade_phase);
        e.o.row_ready = !i.reset && row_credit && source_ready;
        e.o.row_accept = actual_ce && i.row_valid && e.o.row_ready;
        e.color_fire = e.o.row_accept && i.row_index & 1 == 0;
        e.depth_input_fire = e.o.row_accept && i.row_index & 1 != 0;
        e.row0_fire = e.color_fire && i.row_index & 7 == 0;
        e.depth_fire = e.leaf_ce && s.depth_pending;
        e.color_fire_write = e.leaf_ce && s.color_pending;
        let line = if s.fill_row == 0 {
            e.hit_line
        } else {
            s.ctx_line[usize::from(s.tail)]
        };
        e.incoming_bank = (i.row_index >> 1 & 3) ^ (((i.x >> 1 & 1) as u8) << 1);
        let address = u16::from(i.row_index & 1) << 9
            | u16::from(line) << 5
            | u16::from(i.y >> 1 & 3) << 3
            | u16::from(i.row_index >> 2 & 1) << 2
            | (i.x >> 2 & 3);
        let covered = s.ctx_mask[usize::from(s.tail)].slice(u32::from(i.row_index >> 1 & 3), 1);
        let old_color = if i.blend {
            super::super::pixel_math::select(
                covered,
                self.banks[usize::from(e.incoming_bank)].aq,
                Bits::new(0, 16),
                16,
            )
        } else {
            Bits::new(0, 16)
        };
        e.ri = RopInput {
            reset: i.reset,
            ce: e.leaf_ce,
            shade_phase: i.shade_phase,
            valid: e.depth_input_fire,
            blend: i.blend,
            depth_func: i.depth_func,
            depth_write: i.depth_write,
            covered,
            old_color,
            old_depth: self.banks[usize::from(s.compare_bank)].aq,
            rgb: s.source_rgb.slice(0, 24),
            texture_alpha: i.data.slice(24, 8),
            texture_capture: e.color_fire,
            alpha: i.alpha,
            depth: i.data.slice(0, 16),
            shared_rop_product: i.shared_rop_product,
        };
        e.ro = self.rop.outputs(e.ri);
        e.return_fire = e.leaf_ce && e.ro.valid;
        e.o.shared_rop_offer = i.ce && !i.reset && e.ro.shared_mul_pending;
        e.o.shared_rop_ce = e.ro.shared_rop_ce;
        e.o.shared_rop_a = e.ro.shared_rop_a;
        e.o.shared_rop_b = e.ro.shared_rop_b;
        e.o.mem_req_valid = s.mt_request
            && !i.reset
            && (!i.serial.active || !s.mt_write || s.mt_issue & 2 == 0 || i.serial.lower_ready);
        e.o.mem_req_write = s.mt_write;
        e.req_accept = e.o.mem_req_valid && i.mem_req_ready;
        let only_color = s.mt_write && s.mt_remaining & 2 == 0 && !i.serial.active;
        let data_left = if only_color {
            s.mt_data & 32 == 0
        } else {
            s.mt_data & 64 == 0
        };
        e.data_last = s.mt_data & 31 == 31 && (s.mt_data & 32 != 0 || only_color);
        e.retire_last = s.mt_retire & 1 != 0 && (s.mt_retire & 2 != 0 || only_color);
        e.issue_last = s.mt_issue & 1 != 0 && (s.mt_issue & 2 != 0 || only_color);
        let data_owned = s.mt_data >> 4 != s.mt_issue || e.req_accept;
        e.wbeat = s.mt_active && s.mt_write && s.wr_beat_valid && i.mem_write_ready && data_owned;
        e.rbeat = s.mt_active && !s.mt_write && data_left && data_owned && i.mem_read_valid;
        e.read_issue = s.mt_active
            && s.mt_write
            && data_left
            && (!s.wr_beat_valid || (e.wbeat && !e.data_last));
        let read_word = (s.mt_data + u8::from(e.wbeat)) & 127;
        e.read_issue &= !i.serial.active || read_word & 32 == 0 || i.serial.lower_ready;
        let read_address = if i.serial.active {
            u16::from(read_word & 63)
        } else {
            u16::from(read_word >> 5 & 1) << 9
                | u16::from(s.mt_line) << 5
                | u16::from(read_word & 31)
        };
        let write_address = if i.serial.active {
            u16::from(s.mt_data & 63)
        } else {
            u16::from(s.mt_data >> 5 & 1) << 9
                | u16::from(s.mt_line) << 5
                | u16::from(s.mt_data & 31)
        };
        let tile = if s.mt_write {
            s.tag[usize::from(s.mt_line)]
        } else {
            s.mt_target
        };
        let linear = if i.serial.active {
            let mut remainder = i.serial.tile & 511;
            let mut row = 0;
            for (step, bit) in [(200, 8), (100, 4), (50, 2), (25, 1)] {
                if remainder >= step {
                    remainder -= step;
                    row |= bit;
                }
            }
            ((remainder & 31) << 4) | row
        } else {
            ((tile & 31) << 4) | (tile >> 6 & 15)
        };
        let base = if s.mt_issue & 2 != 0 && !i.serial.active {
            i.depth_base
        } else {
            i.color_base
        };
        let page = ((base >> 7) + (linear >> 7)) & 127;
        e.o.mem_req_address = u32::from(page) << 16
            | u32::from(linear & 127) << 9
            | u32::from(if i.serial.active {
                s.mt_issue >> 1 & 1
            } else {
                (tile >> 5 & 1) as u8
            }) << 8
            | u32::from(s.mt_issue & 1) << 7;
        e.o.mem_req_address_known = u32::MAX;
        if i.serial.copy {
            e.o.mem_req_address = ((i.serial.copy_address.value as u32 & 16383) << 9)
                | u32::from(s.mt_issue & 3) << 7;
            e.o.mem_req_address_known =
                0xff8001ff | ((i.serial.copy_address.known as u32 & 16383) << 9);
        }
        e.o.serial_ready = i.serial.active && !s.mt_active && !s.flushing;
        e.o.serial_done = i.serial.active
            && s.mt_active
            && i.mem_complete_valid
            && if s.mt_write {
                e.retire_last
            } else {
                s.mt_retire & 1 != 0
            };
        e.o.mem_write_valid = s.wr_beat_valid && !i.reset;
        let mut packed = Bits::default();
        for n in 0..4 {
            let bank = if s.mt_data & 4 != 0 { n ^ 2 } else { n };
            packed.set(n as u32 * 16, 16, self.banks[bank].bq);
        }
        e.o.mem_write_data = packed;
        let rop_read = e.color_fire && i.blend && i.mask & (1 << (i.row_index >> 1 & 3)) != 0
            || e.depth_input_fire
                && i.depth_func & 7 != 0
                && i.depth_func & 7 != 7
                && i.mask & (1 << (i.row_index >> 1 & 3)) != 0;
        for n in 0..4 {
            e.a_write_d[n] =
                e.depth_fire && one(s.result_depth_flag) && usize::from(s.result_bank) == n;
            e.a_write_c[n] =
                e.color_fire_write && one(s.result_color_flag) && usize::from(s.result_bank) == n;
            e.a_read[n] = rop_read && usize::from(e.incoming_bank) == n;
            let we = e.a_write_d[n] || e.a_write_c[n];
            e.a[n] = Port {
                en: e.a_read[n] || we,
                we,
                address: if we {
                    u16::from(e.a_write_d[n]) << 9 | s.result_address
                } else {
                    address
                },
                data: if s.depth_pending {
                    s.depth_data
                } else {
                    s.color_data
                },
            };
            let source_bank = if s.mt_data & 4 != 0 { n ^ 2 } else { n };
            if i.serial.active {
                let up = i.serial.pixels.slice(n as u32 * 16, 16);
                let uw = i.serial.pixel_valid && one(up.slice(0, 1));
                let mut data = Bits::default();
                data.set(11, 5, up.slice(11, 5));
                data.set(6, 5, up.slice(6, 5));
                data.set(5, 1, up.slice(10, 1));
                data.set(0, 5, up.slice(1, 5));
                e.a[n] = Port {
                    en: uw,
                    we: uw,
                    address: u16::from(i.serial.pixel_half) << 5
                        | u16::from(i.serial.pixel_word & 31),
                    data,
                };
            }
            e.b[n] = Port {
                en: e.read_issue || e.rbeat,
                we: e.rbeat,
                address: if e.rbeat { write_address } else { read_address },
                data: Bits::new(i.mem_read_data >> (source_bank * 16), 16),
            };
        }
        for n in 0..2 {
            if s.ctx_valid[n] {
                e.pinned |= s.ctx_line_mask[n];
            }
        }
        e.result_line_mask = s.ctx_line_mask[usize::from(s.result_context)];
        let mut free = None;
        for n in 0..8 {
            if e.pinned & (1 << n) == 0 && !s.valid[n] && free.is_none() {
                free = Some(n as u8);
            }
            if s.dirty_color[n] || s.dirty_depth[n] {
                e.any_dirty = true;
                if e.pinned & (1 << n) == 0 && e.dirty_found.is_none() {
                    e.dirty_found = Some(n as u8);
                }
            }
        }
        e.select_line = (0..3)
            .map(|d| (s.victim + d) & 7)
            .find(|n| e.pinned & (1 << n) == 0)
            .unwrap_or((s.victim + 2) & 7);
        e.sel_line = free.unwrap_or(e.select_line);
        e.sel_color = s.dirty_color[usize::from(e.sel_line)];
        e.sel_depth = s.dirty_depth[usize::from(e.sel_line)];
        e.sel_dirty = e.sel_color || e.sel_depth;
        e.sel_plane = !e.sel_color;
        let spec_found = !(i.row_valid && e.hits & (1 << e.sel_line) != 0)
            && !(s.protect_valid && s.protect_line == e.sel_line);
        let hint_resident = (0..8).any(|n| s.valid[n] && s.tag[n] == s.hints[0]);
        let incoming_resident = (0..8).any(|n| s.valid[n] && s.tag[n] == i.prefetch_tile);
        let demand_wait = i.row_valid && s.fill_row == 0 && !e.hit;
        let hint_service = !s.mt_active
            && !i.prefetch_abort
            && !i.flush_request
            && !s.flushing
            && !demand_wait
            && s.hint_count != 0;
        e.hint_start = hint_service && !hint_resident && spec_found;
        e.hint_unsafe = hint_service && !hint_resident && !spec_found;
        e.hint_pop = hint_service;
        let duplicate = (s.hint_count != 0 && s.hints[0] == i.prefetch_tile)
            || (s.hint_count == 2 && s.hints[1] == i.prefetch_tile)
            || incoming_resident
            || (s.mt_active && s.mt_tvalid && s.mt_target == i.prefetch_tile);
        e.hint_push = i.prefetch_valid
            && i.prefetch_tile & 31 < 25
            && i.prefetch_tile >> 5 < 30
            && s.hint_count < 2
            && !duplicate
            && !i.prefetch_abort
            && !s.flushing
            && !i.flush_request;
        e.o.commit_valid = s.commit_valid;
        e.o.commit_x = s.commit_x;
        e.o.commit_y = s.commit_y;
        e.o.commit_mask = s.commit_mask;
        e.o.flush_complete = s.flush_complete;
        e.o.idle = s.hint_count == 0
            && !s.flushing
            && !s.ctx_valid[0]
            && !s.ctx_valid[1]
            && !s.mt_active
            && s.fill_row == 0
            && !s.depth_pending
            && !s.color_pending
            && !e.ro.valid
            && !e.ro.shared_mul_pending;
        e
    }
}
