use super::*;
fn ndc() -> &'static [u32; 320] {
    static WORDS: std::sync::OnceLock<[u32; 320]> = std::sync::OnceLock::new();
    WORDS.get_or_init(crate::system::pixel::vliw_quad_pool::ndc_words)
}
impl Pool {
    pub(super) fn update(&mut self, i: Input) -> Result<(), ContractError> {
        let e = self.evaluate(i);
        let s = self.state.clone();
        let h = self.shadow.clone();
        let check = |ok, rule| require(ok, "pool", rule);
        let attr_tail = s.attr_pixel >> 2;
        let depth_tail = h.depth_pixel >> 2;
        let slot = usize::from(s.complete_tail & 31);
        let shadow_complete = s.publish_ptr != s.allocated
            && s.publish_ptr != attr_tail
            && s.publish_ptr != depth_tail
            && s.publish_ptr != h.end_tail
            && s.publish_ptr != h.lod_tail;
        if !i.reset {
            check(
                e.o.global_used <= 24 && e.o.producer_used <= 8,
                "pool credit overflow",
            )?;
            check(
                !e.allocate || (i.phase == 0 && i.quad_x < 200 && i.quad_y < 120),
                "pool quad admission range/phase",
            )?;
            if e.geom_event {
                check(
                    i.quad_end_id == (h.end_tail & 7) && h.end_tail != s.allocated && i.phase == 2,
                    "shadow geometry prefix/order",
                )?;
            }
            if e.lod_event {
                check(
                    i.lod_id == (h.lod_tail & 7) && h.lod_tail != s.allocated,
                    "shadow LOD prefix/order",
                )?;
            }
            if e.attr_event {
                check(
                    i.result_address == ((s.attr_pixel & 31) << 2) | h.attr_word
                        && attr_tail != s.allocated,
                    "shadow attributes prefix/order",
                )?;
                check(
                    i.phase == [2, 3, 0][h.attr_word as usize],
                    "pool attribute phase",
                )?;
                let fact = h.facts[usize::from((s.attr_pixel >> 2) & 31)];
                check(
                    fact.is_some_and(|f| {
                        f.attributes & (1 << ((s.attr_pixel & 3) * 3 + h.attr_word)) == 0
                    }),
                    "duplicate attribute word",
                )?;
                check(i.result_data.is_known(36), "unknown I attribute word")?;
                check(
                    i.quad_rgb
                        || !((h.attr_word == 1 && i.result_data.value & (3 << 28) != 0)
                            || (h.attr_word == 2 && i.result_data.value & (3 << 16) != 0)),
                    "UV outside U16 source envelope",
                )?;
                check(
                    i.quad_rgb == h.attribute_rgb,
                    "producer mode changed during I return",
                )?;
            }
            if e.depth_event {
                check(
                    h.depth_pixel == s.attr_pixel,
                    "depth/attribute source cursor diverged",
                )?;
                check(
                    i.depth_pid == (h.depth_pixel & 31)
                        && depth_tail != s.allocated
                        && i.phase == 1,
                    "shadow depth prefix/order",
                )?;
                check(
                    h.facts[usize::from(depth_tail & 31)]
                        .is_some_and(|f| f.depth & (1 << (h.depth_pixel & 3)) == 0),
                    "duplicate depth event",
                )?;
            }
            if e.complete_event {
                check(
                    s.complete_tail != s.allocated
                        && h.facts[slot].is_some_and(|f| f.rgb == i.quad_rgb),
                    "completion exceeds allocation or mode changed",
                )?;
                check(
                    h.facts[slot].is_some_and(|f| f.geometry && f.depth == 15),
                    "completion before geometry/four depth events",
                )?;
                if i.quad_rgb {
                    check(
                        attr_tail == s.complete_tail
                            && h.attr_word == 2
                            && s.attr_pixel & 3 == 3
                            && h.lod_tail != s.complete_tail,
                        "RGB completion before W or old L",
                    )?;
                    check(
                        h.facts[slot].is_some_and(|f| f.attributes == 0x7ff && f.lod),
                        "RGB missing old eleven W or L",
                    )?;
                } else {
                    check(
                        attr_tail != s.complete_tail
                            && h.lod_tail == s.complete_tail
                            && i.lod_id == (s.complete_tail & 7),
                        "UV completion before W or wrong old L",
                    )?;
                    check(
                        h.facts[slot].is_some_and(|f| f.attributes == 0xfff && !f.lod),
                        "UV missing twelve W or duplicate L",
                    )?;
                }
            }
            check(
                (s.publish_ptr != s.complete_tail) == shadow_complete,
                "compressed and original publication barrier differ",
            )?;
            check(
                distance(s.allocated, s.complete_tail) <= 24
                    && distance(s.complete_tail, s.publish_ptr) <= 24,
                "compressed prefix overtook allocation/publication",
            )?;
            check(
                !(e.sample_fetch && e.sample_meta.value & 4 != 0),
                "RGB must skip LOD/UV consumer",
            )?;
            check(
                !(e.allocate && attr_tail != s.allocated && i.quad_rgb != h.attribute_rgb),
                "mode changed before I attribute drain",
            )?;
            check(
                !(e.regular_retire || e.skip_empty)
                    || (s.retired != s.publish_ptr
                        && (!e.regular_retire || i.retire_id == (s.retired & 31))
                        && s.light_cursor != s.retired
                        && s.sample_cursor != s.retired),
                "GPU contract: fault",
            )?;
            check(
                !(i.backend_ce && i.empty_skip && !e.o.empty_skip_ready),
                "GPU contract: fault",
            )?;
            check(
                !(e.regular_retire && e.skip_empty) && !e.o.basic_collision,
                "GPU contract: fault",
            )?;
            if e.light_read {
                check(
                    e.light_meta.is_known(19) && e.light_mask.is_known(4),
                    "unknown lighting metadata at read",
                )?;
            }
            if e.sample_fetch {
                check(
                    e.sample_meta.is_known(3)
                        && e.sample_mask.is_known(4)
                        && e.sample_lod.is_known(8),
                    "unknown sampling metadata at read",
                )?;
            }
            if e.ndc_read {
                check(e.ndc_address < 320, "NDC read beyond installed table")?;
            }
            if e.sample_take {
                check(
                    e.o.sample_last.is_known(1),
                    "unknown sampling LAST at acceptance",
                )?;
            }
        }
        if i.reset {
            self.state = State::default();
            let facts = h.facts;
            self.shadow = Shadow {
                facts,
                ..Default::default()
            };
            self.ndc_q = Bits::new(0, 32);
        } else {
            let n = &mut self.state;
            if e.light_read {
                n.light_descriptor = ((e.light_meta.value as u16 & 1) << 9)
                    | ((e.light_mask.value as u16 & 15) << 5)
                    | u16::from(s.light_cursor & 31);
            }
            if e.sample_fetch {
                n.sample_descriptor = ((e.sample_lod.value as u32 & 255) << 11)
                    | (u32::from(s.sample_lane == 0) << 10)
                    | ((e.sample_meta.value as u32 & 1) << 9)
                    | ((e.sample_mask.value as u32 & 15) << 5)
                    | u32::from(s.sample_cursor & 31);
            }
            if e.allocate {
                n.allocated = (s.allocated + 1) & 63;
            }
            if e.complete_event {
                n.complete_tail = (s.complete_tail + 1) & 63;
            }
            if e.attr_event {
                if i.phase == 2 {
                    n.normal_y_high = i.result_data.slice(18, 6);
                }
                if i.phase == 0 {
                    n.attr_pixel = s.attr_pixel.wrapping_add(1);
                }
            }
            if e.depth_event {
                n.depth_mask = e.next_depth_mask;
            }
            if e.o.published {
                n.publish_ptr = (s.publish_ptr + 1) & 63;
            }
            if i.backend_ce {
                if e.light_skip {
                    n.light_cursor = (s.light_cursor + 1) & 63;
                } else {
                    match s.light_state {
                        L_READ if e.light_fetch => n.light_state = L_X,
                        L_X => {
                            n.ndc_x = self.ndc_q.slice(u32::from(s.light_lane & 1) * 16, 16);
                            n.light_state = L_PIXEL;
                        }
                        L_PIXEL if e.light_take => {
                            n.light_state = L_READ;
                            n.light_lane = (s.light_lane + 1) & 3;
                            if s.light_lane == 3 {
                                n.light_cursor = (s.light_cursor + 1) & 63;
                            }
                        }
                        _ => {}
                    }
                }
                if e.sample_skip {
                    n.sample_cursor = (s.sample_cursor + 1) & 63;
                }
                if e.sample_fetch {
                    n.sample_loaded = true;
                    n.sample_lane = e.sample_read_lane;
                }
                if e.sample_take {
                    n.sample_loaded = false;
                    if e.o.sample_last.value & 1 != 0 {
                        n.sample_cursor = (s.sample_cursor + 1) & 63;
                        n.sample_lane = 0;
                    } else {
                        n.sample_lane = e.sample_next_lane;
                    }
                }
            }
            if e.regular_retire || e.skip_empty {
                n.retired = (s.retired + 1) & 63;
            }
            if e.allocate {
                let address = usize::from(s.allocated & 31);
                self.headers[address] = e.header_write;
                self.colors[address] = Bits::new(u64::from(i.quad_color & 0xffffff), 24);
                self.shadow.facts[address] = Some(Facts {
                    rgb: i.quad_rgb,
                    ..Default::default()
                });
                if attr_tail == s.allocated {
                    self.shadow.attribute_rgb = i.quad_rgb;
                }
            }
            if e.mask_write {
                self.masks[usize::from((s.attr_pixel >> 2) & 31)] =
                    Bits::new(u64::from(e.next_depth_mask), 4);
            }
            if e.lod_event && !i.quad_rgb {
                self.lods[slot] = Bits::new(u64::from(i.lod_value), 8);
            }
            if e.geom_event {
                self.shadow.end_tail = (h.end_tail + 1) & 63;
                self.shadow.facts[usize::from(h.end_tail & 31)]
                    .as_mut()
                    .unwrap()
                    .geometry = true;
            }
            if e.lod_event {
                self.shadow.lod_tail = (h.lod_tail + 1) & 63;
                self.shadow.facts[usize::from(h.lod_tail & 31)]
                    .as_mut()
                    .unwrap()
                    .lod = true;
            }
            if e.attr_event {
                self.shadow.attr_word = if h.attr_word == 2 { 0 } else { h.attr_word + 1 };
                self.shadow.facts[usize::from((s.attr_pixel >> 2) & 31)]
                    .as_mut()
                    .unwrap()
                    .attributes |= 1 << ((s.attr_pixel & 3) * 3 + h.attr_word);
            }
            if e.depth_event {
                self.shadow.depth_pixel = h.depth_pixel.wrapping_add(1);
                self.shadow.facts[usize::from(depth_tail & 31)]
                    .as_mut()
                    .unwrap()
                    .depth |= 1 << (h.depth_pixel & 3);
            }
            if e.ndc_read {
                self.ndc_q = Bits::new(u64::from(ndc()[e.ndc_address]), 32);
            }
        }
        let address = u16::from(s.attr_pixel & 127);
        let normal = Ports {
            reset: i.reset,
            we: e.normal_write,
            mask: e.normal_mask,
            wa: address,
            data: e.normal_data,
            re: e.light_read,
            ra: u16::from(s.light_cursor & 31) * 4 + u16::from(s.light_lane),
        };
        let uv = Ports {
            we: e.uv_write,
            mask: e.uv_mask,
            data: e.uv_data,
            re: e.sample_fetch,
            ra: u16::from(s.sample_cursor & 31) * 4 + u16::from(e.sample_read_lane),
            ..normal
        };
        let basic = Ports {
            we: e.basic_write,
            mask: e.basic_mask,
            data: e.basic_data,
            re: e.basic_read,
            ra: u16::from(i.basic_address & 127),
            ..normal
        };
        let clocks = [
            self.normal.clock(normal),
            self.uv.clock(uv),
            self.basic.clock(basic),
            self.depth.clock(
                i.reset,
                e.depth_event,
                s.attr_pixel & 127,
                Bits::new(
                    (u64::from(i.quad_color >> 24) << 16) | u64::from(i.depth_z16),
                    24,
                ),
                e.depth_read,
                i.depth_address & 127,
            ),
        ];
        for clock in clocks {
            clock?;
        }
        Ok(())
    }
}
