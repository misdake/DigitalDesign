use super::*;
impl Pool {
    pub(super) fn evaluate(&self, i: Input) -> Eval {
        let s = &self.state;
        let head = usize::from(s.retired & 31);
        let next = (head + 1) & 31;
        let light = usize::from(s.light_cursor & 31);
        let sample = usize::from(s.sample_cursor & 31);
        let publish = usize::from(s.publish_ptr & 31);
        let issue = usize::from(i.descriptor_issue_slot & 31);
        let global_used = distance(s.allocated, s.retired);
        let producer_used = distance(s.allocated, s.publish_ptr);
        let quad_ready = !i.reset && global_used < 24 && producer_used < 8;
        let allocate = i.front_ce && i.quad_valid && quad_ready;
        let attr_event = i.front_ce && i.result_write && !i.reset;
        let depth_event = i.front_ce && i.depth_valid && !i.reset;
        let lod_event = i.front_ce && i.lod_valid && !i.reset;
        let geom_event = i.front_ce && i.quad_end_valid && !i.reset;
        let complete_event = (i.quad_rgb && attr_event && i.phase == 0 && s.attr_pixel & 3 == 3)
            || (!i.quad_rgb && lod_event);
        let normal_write = attr_event && i.phase & 2 != 0;
        let uv_write = attr_event && matches!(i.phase, 3 | 0) && !i.quad_rgb;
        let basic_write = attr_event && matches!(i.phase, 3 | 0);
        let normal_data = Bits::concat(
            i.result_data.slice(0, 12),
            Bits::concat(s.normal_y_high, i.result_data.slice(0, 18), 18),
            24,
        );
        let uv_data = Bits::concat(
            Bits::concat(Bits::new(0, 2), i.result_data.slice(0, 16), 16),
            Bits::concat(Bits::new(0, 2), i.result_data.slice(12, 16), 16),
            18,
        );
        let bytes = if i.quad_rgb {
            [
                i.result_data.slice(12, 8),
                i.result_data.slice(0, 8),
                i.result_data.slice(8, 8),
            ]
        } else {
            [Bits::new(255, 8); 3]
        };
        let mut basic_data = Bits::new(0, 36);
        for (n, b) in bytes.into_iter().enumerate() {
            basic_data.set(n as u32 * 9, 8, b);
        }
        let normal_mask = if i.phase & 1 != 0 { 12 } else { 3 };
        let uv_mask = if i.phase & 1 != 0 { 3 } else { 12 };
        let basic_mask = if i.phase & 1 != 0 { 1 } else { 6 };
        let next_depth_mask = (if s.attr_pixel & 3 == 0 {
            0
        } else {
            s.depth_mask
        }) | (u8::from(i.depth_pass) << (s.attr_pixel & 3));
        let mask_write = depth_event && s.attr_pixel & 3 == 3;
        let header_write = Bits::new(
            (u64::from(i.quad_y & 127) << 12)
                | (u64::from(i.quad_x) << 4)
                | (u64::from(i.quad_rgb) << 2)
                | (u64::from(i.draw_unlit) << 1),
            19,
        );
        let published =
            i.backend_ce && i.publish_ready && s.publish_ptr != s.complete_tail && !i.reset;
        let empty_head = s.publish_ptr != s.retired && self.masks[head].value & 15 == 0 && !i.reset;
        let empty_skip_ready =
            empty_head && s.light_cursor != s.retired && s.sample_cursor != s.retired;
        let skip_empty = i.backend_ce && i.empty_skip && empty_skip_ready;
        let regular_retire = i.backend_ce && i.retire_valid && !i.reset;
        let light_meta = self.headers[light];
        let light_mask = self.masks[light];
        let light_exists = s.light_cursor != s.publish_ptr;
        let light_skip = s.light_state == L_READ
            && light_exists
            && (light_mask.value & 15 == 0 || light_meta.value & 2 != 0);
        let light_fetch = s.light_state == L_READ && light_exists && !light_skip;
        let light_read = i.backend_ce && light_fetch && !i.reset;
        let ndc_read = i.backend_ce && (light_fetch || s.light_state == L_X) && !i.reset;
        let ndc_address = if s.light_state == L_X {
            200 + ((light_meta.value >> 12) & 127) as usize
        } else {
            ((light_meta.value >> 4) & 255) as usize
        };
        let light_valid = !i.reset && s.light_state == L_PIXEL;
        let light_position = Bits::concat(
            self.ndc_q.slice(u32::from(s.light_lane >> 1) * 16, 16),
            s.ndc_x,
            16,
        );
        let light_take = i.backend_ce && light_valid && i.light_ready;
        let sample_meta = self.headers[sample].slice(0, 3);
        let sample_mask = self.masks[sample];
        let sample_lod = self.lods[sample];
        let sample_exists = s.sample_cursor != s.publish_ptr;
        let first = if sample_mask.value & 1 != 0 {
            0
        } else if sample_mask.value & 2 != 0 {
            1
        } else if sample_mask.value & 4 != 0 {
            2
        } else {
            3
        };
        let sample_read_lane = if s.sample_lane == 0 {
            first
        } else {
            s.sample_lane
        };
        let more1 = sample_mask.value & 2 != 0 && s.sample_lane == 0;
        let more2 = sample_mask.value & 4 != 0 && s.sample_lane & 2 == 0;
        let tail = match s.sample_lane {
            0 => 14,
            1 => 12,
            2 => 8,
            _ => 0,
        };
        let sample_last = if sample_mask.known & sample_mask.value & tail != 0 {
            Bits::new(0, 1)
        } else if sample_mask.known & tail == tail {
            Bits::new(u64::from(sample_mask.value & tail == 0), 1)
        } else {
            Bits::default()
        };
        let sample_next_lane = (u8::from(!more1) << 1) | u8::from(more1 || !more2);
        let sample_skip = !s.sample_loaded
            && s.sample_lane == 0
            && sample_exists
            && (sample_mask.value & 15 == 0 || sample_meta.value & 4 != 0);
        let sample_fetch =
            i.backend_ce && !s.sample_loaded && sample_exists && !sample_skip && !i.reset;
        let sample_valid = s.sample_loaded && !i.reset;
        let sample_take = i.backend_ce && sample_valid && i.sample_ready;
        let basic_read = i.backend_ce && i.basic_read && !i.reset;
        let depth_read = i.backend_ce && i.depth_read && !i.reset;
        let basic_collision =
            basic_read && basic_write && (s.attr_pixel & 127) == (i.basic_address & 127);
        let basic_data_out = Bits::concat(
            self.basic.q.slice(18, 8),
            Bits::concat(self.basic.q.slice(9, 8), self.basic.q.slice(0, 8), 8),
            16,
        );
        let o = Output {
            quad_ready,
            quad_id: s.allocated & 7,
            prefetch_valid: i.front_ce && i.quad_end_valid && i.quad_end_mask & 15 != 0 && !i.reset,
            prefetch_tile: (u16::from(i.quad_y >> 2) << 5) | u16::from(i.quad_x >> 3),
            published,
            published_slot: s.publish_ptr & 31,
            published_begin: descriptor(self.headers[publish], self.masks[publish]),
            descriptor_issue: descriptor(self.headers[issue], self.masks[issue]),
            descriptor_head: descriptor(self.headers[head], self.masks[head]),
            descriptor_next: descriptor(self.headers[next], self.masks[next]),
            issue_color: self.colors[issue],
            descriptor_head_tile: tile(self.headers[head]),
            descriptor_next_tile: tile(self.headers[next]),
            basic_data: basic_data_out,
            basic_collision,
            depth_data: self.depth.q,
            light_valid,
            light_normal: self.normal.q,
            light_position,
            light_lane: s.light_lane,
            light_descriptor: s.light_descriptor,
            sample_valid,
            sample_data: self.uv.q,
            sample_row: s.sample_lane,
            sample_last,
            sample_descriptor: s.sample_descriptor,
            empty_head,
            empty_skip_ready,
            retire_slot: s.retired & 31,
            global_used,
            producer_used,
            light_pending: distance(s.publish_ptr, s.light_cursor),
            sample_pending: distance(s.publish_ptr, s.sample_cursor),
            busy: s.allocated != s.retired || s.light_state != L_READ || s.sample_loaded,
        };
        Eval {
            o,
            allocate,
            attr_event,
            depth_event,
            lod_event,
            geom_event,
            complete_event,
            normal_write,
            uv_write,
            basic_write,
            normal_data,
            uv_data,
            basic_data,
            normal_mask,
            uv_mask,
            basic_mask,
            next_depth_mask,
            mask_write,
            header_write,
            light_read,
            ndc_read,
            ndc_address,
            light_skip,
            light_fetch,
            light_take,
            light_meta,
            light_mask,
            sample_meta,
            sample_mask,
            sample_lod,
            sample_read_lane,
            sample_next_lane,
            sample_skip,
            sample_fetch,
            sample_take,
            basic_read,
            depth_read,
            skip_empty,
            regular_retire,
        }
    }
}
