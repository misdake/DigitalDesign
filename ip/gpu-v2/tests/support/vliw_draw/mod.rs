//! First checkpoint: actual CP, controlled DMA/render/flush and source-RAM peers.
//! No fixture here claims that a pixel/backend or a native-memory DRAW ran.
use gpu_v2::system::vliw_command::Command;
use gpu_v2::system::vliw_cycle::command::{
    DeviceBus, DrawInput, Error, Phase, Processor, ProcessorInput, ProcessorOutput,
};
use std::{collections::VecDeque, fmt::Write as _};

#[derive(Clone, Copy, Default)]
pub struct Case {
    pub view: bool,
    pub bad: Option<&'static str>,
}
pub struct Pin {
    pub name: &'static str,
    pub width: u32,
    pub output: bool,
    pub value: u64,
}
pub struct Edge {
    pub input: ProcessorInput,
    pub output: ProcessorOutput,
    pub phase: Phase,
    pub pc: u8,
    pub cache_pages: u8,
    pub status: (bool, bool, bool, u8, u8, u8, u8),
}
pub struct Run {
    pub trace: Vec<Edge>,
    pub error: Option<Error>,
    pub starts: usize,
    pub completions: usize,
}
pub fn run(case: Case) -> Run {
    let mut cp = Processor::with_draws();
    let mut cells = [None; 512];
    cells[264] = Some(2048 | (2560u64 << 14));
    cells[272] = Some((100u64 << 16) | (0x1234u64 << 32) | (0xabcd_u64 << 48));
    cells[273] = Some(0x4000 | (0x1111u64 << 16) | (0xeeefu64 << 32) | (0x2800u64 << 48));
    cells[274] = Some(129 | (200u64 << 16) | (1u64 << 32) | (12u64 << 34) | (0x789au64 << 48));
    cells[280] = Some(
        0x1000
            | (if case.view {
                9 | (1 << 7)
            } else {
                6 | (1 << 4)
            }) << 16
            | (1u64 << 21)
            | (0x563412u64 << 32),
    );
    cells[281] = Some(3 | 8 | 16 | (0xfbu64 << 32));
    cells[331] = Some(0x10203040);
    cells[363] = Some(0xc0507090);
    let mut code = vec![
        Command::dma_local(0x10000, 320, 128).unwrap().0,
        Command::dma_local(0x11000, 352, 128).unwrap().0,
        Command::wait(true, true).0,
        Command::FLUSH.0,
        Command::set_target(264).unwrap().0,
        Command::set_global(272).unwrap().0,
        Command::set_render(280).unwrap().0,
        Command::TEX_INVALIDATE.0,
        Command::dma_meshlet(0x20000, 1, 128).unwrap().0,
        Command::wait(true, false).0,
        Command::draw_meshlet(1).unwrap().0,
        Command::wait(true, true).0,
        Command::FLUSH.0,
        Command::END.0,
    ];
    match case.bad {
        Some("missing_stored") => code[3] = 0,
        Some("global_code") => cells[274] = Some(129 | (17u64 << 34)),
        Some("target_alias") => cells[264] = Some(2048 | (2048u64 << 14)),
        Some("view_shape") => cells[280] = Some(0x1000 | (8 << 16) | (1 << 23)),
        Some("color_high") => cells[363] = Some(1 << 32),
        Some("bad_local") => code[0] |= 1,
        Some("bad_mesh") => code[8] |= 16,
        _ => {}
    }
    for (n, pair) in code.chunks(2).enumerate() {
        cells[448 + n] = Some(u64::from(pair[0]) | (u64::from(*pair.get(1).unwrap_or(&0)) << 32));
    }
    let mut q = 0;
    let mut pending_cache = None;
    let mut work = VecDeque::new();
    let mut draw_end = None;
    let mut render_end = 0;
    let mut flush_age = 0;
    let mut scheduled = VecDeque::from([(2, 0, (code.len() - 1) as u16), (3, 1, 0x40)]);
    let mut result = Run {
        trace: Vec::new(),
        error: None,
        starts: 0,
        completions: 0,
    };
    let mut prior_busy = false;
    for wall in 0..100000 {
        let phase = cp.phase();
        flush_age = if phase == Phase::Flushing {
            flush_age + 1
        } else {
            0
        };
        let mut d = DrawInput {
            work_ready: work.len() < 2 && wall % 5 != 0,
            meshlet_ready: wall % 9 > 2,
            meshlet_done: draw_end == Some(wall),
            ..Default::default()
        };
        if let Some(&(until, mesh, bank)) = work.front() {
            d.mesh_loading = mesh;
            d.mesh_slot = mesh;
            if until == wall {
                work.pop_front();
                if mesh {
                    d.mesh_done = true;
                    d.mesh_done_slot = true;
                } else {
                    d.uniform_done = true;
                    d.uniform_bank = bank;
                }
            }
        }
        let mut i = ProcessorInput {
            reset: wall < 2,
            ce: wall % 7 > 1,
            ram_ready: wall % 11 > 3,
            ram_q: q,
            render_idle: wall >= render_end,
            work_busy: !work.is_empty() || d.uniform_done || d.mesh_done,
            dma_idle: work.is_empty() && pending_cache.is_none(),
            flush_complete: flush_age >= 13,
            draw: d,
            ..Default::default()
        };
        // This wall-owned completion must not depend on compute CE.
        if d.uniform_done {
            i.ce = false;
        }
        if pending_cache == Some(wall) {
            i.cache_done = true;
            pending_cache = None;
        }
        i.cache_ready = pending_cache.is_none();
        if let Some((_, channel, data)) = scheduled.pop_front_if(|s| s.0 == wall) {
            i.bus = DeviceBus {
                index: 4,
                channel,
                write: true,
                data,
                ..Default::default()
            };
            i.ce = false;
        }
        let before = cp.outputs(i);
        if before.work.valid && i.ce && d.work_ready {
            let w = before.work;
            work.push_back((wall + 29, w.mesh, ((w.destination >> 2) & 3)));
            if w.mesh {
                i.draw.mesh_begin = true;
                i.draw.mesh_begin_slot = w.slot;
                i.draw.header_write = true;
                i.draw.header_slot = w.slot;
                i.draw.header_data = 2 << 16;
            }
        }
        let o = cp.outputs(i);
        result.trace.push(Edge {
            input: i,
            output: o,
            phase,
            pc: cp.pc(),
            cache_pages: cp.cache_pages(),
            status: cp.draw_status().unwrap(),
        });
        if let Err(e) = cp.clock(i) {
            result.error = Some(e);
            return result;
        }
        if i.reset {
            q = 0;
        } else if i.ce && i.ram_ready && o.ram_read {
            q = cells[o.ram_address as usize]
                .expect("controlled CP read before fixture publication");
        }
        if o.cache.valid && i.cache_ready {
            pending_cache = Some(wall + 5);
        }
        if i.ce && o.start && d.meshlet_ready {
            assert_eq!(o.bindings.mesh_color, 0xc0507090);
            assert_eq!(o.bindings.far_z, 100 << 16);
            assert_eq!(o.bindings.light, [0x1234, -21555, 0x4000]);
            assert_eq!(o.bindings.ray, [0x1111, -4369, 0x2800]);
            assert_eq!(
                (o.bindings.ambient, o.bindings.directional, o.bindings.code),
                (129, 200, 12)
            );
            assert_eq!((o.bindings.color_base, o.bindings.depth_base), (2048, 2560));
            assert_eq!(o.bindings.lod_bias, -5);
            assert_eq!(o.bindings.texture_framebuffer, case.view);
            result.starts += 1;
            draw_end = Some(wall + 7);
            render_end = wall + 41;
        }
        if prior_busy && !o.busy {
            result.completions += 1;
            if result.completions == 2 {
                assert_eq!(result.starts, 2);
                return result;
            }
            scheduled.push_back((wall + 1, 0, (code.len() - 1) as u16));
            scheduled.push_back((wall + 2, 1, 0x40));
        }
        prior_busy = o.busy;
    }
    panic!(
        "controlled CP fixture exceeded100000 edges at {:?}",
        cp.phase()
    );
}
pub fn fields(e: &Edge) -> Vec<Pin> {
    let i = e.input;
    let o = e.output;
    let d = i.draw;
    let c = o.bindings;
    let mut p = Vec::new();
    macro_rules! pin {
        ($n:literal,$w:expr,$v:expr,$out:expr) => {
            p.push(Pin {
                name: $n,
                width: $w,
                value: u64::from($v),
                output: $out,
            });
        };
    }
    pin!("reset", 1, i.reset, false);
    pin!("ce", 1, i.ce, false);
    pin!("abort", 1, i.abort, false);
    pin!(
        "load_count",
        1,
        i.bus.index == 4 && i.bus.write && i.bus.channel == 0,
        false
    );
    pin!(
        "submit",
        1,
        i.bus.index == 4 && i.bus.write && i.bus.channel == 1,
        false
    );
    pin!("control_data", 16, i.bus.data, false);
    pin!("render_idle", 1, i.render_idle, false);
    pin!("render_fault", 1, i.render_fault, false);
    pin!("memory_busy", 1, i.memory_busy, false);
    pin!("work_busy", 1, i.work_busy, false);
    pin!("dma_idle", 1, i.dma_idle, false);
    pin!("dma_fault", 1, i.dma_fault, false);
    pin!("cp_ready", 1, i.ram_ready, false);
    pin!("cp_q", 64, i.ram_q, false);
    pin!("flush_complete", 1, i.flush_complete, false);
    pin!("cache_ready", 1, i.cache_ready, false);
    pin!("cache_done", 1, i.cache_done, false);
    pin!("cache_done_bank", 1, i.cache_done_bank, false);
    pin!("work_ready", 1, d.work_ready, false);
    pin!("meshlet_ready", 1, d.meshlet_ready, false);
    pin!("meshlet_done", 1, d.meshlet_done, false);
    pin!("mesh_loading", 1, d.mesh_loading, false);
    pin!("mesh_slot", 1, d.mesh_slot, false);
    pin!("mesh_begin", 1, d.mesh_begin, false);
    pin!("mesh_begin_slot", 1, d.mesh_begin_slot, false);
    pin!("mesh_done", 1, d.mesh_done, false);
    pin!("mesh_done_slot", 1, d.mesh_done_slot, false);
    pin!("uniform_done", 1, d.uniform_done, false);
    pin!("uniform_bank", 2, d.uniform_bank, false);
    pin!("header_we", 1, d.header_write, false);
    pin!("header_slot", 1, d.header_slot, false);
    pin!("header_data", 64, d.header_data, false);
    pin!("submit_ready", 1, o.submit_ready, true);
    pin!("busy", 1, o.busy, true);
    pin!("stop", 1, o.stop, true);
    pin!("cp_re", 1, o.ram_read, true);
    pin!("cp_addr", 9, o.ram_address, true);
    pin!("cache_valid", 1, o.cache.valid, true);
    pin!("cache_source", 22, o.cache.source_halfword, true);
    pin!("cache_bank", 1, o.cache.bank, true);
    pin!("flush_request", 1, o.flush_request, true);
    pin!("start", 1, o.start, true);
    pin!("draw_slot", 1, o.draw_slot, true);
    pin!("texture_invalidate", 1, o.texture_invalidate, true);
    pin!("work_valid", 1, o.work.valid, true);
    pin!("work_mesh", 1, o.work.mesh, true);
    pin!("work_source", 22, o.work.source_halfword, true);
    pin!("work_destination", 7, o.work.destination, true);
    pin!("work_size", 2, o.work.size, true);
    pin!("work_blocks", 3, o.work.blocks, true);
    pin!("work_slot", 1, o.work.slot, true);
    pin!("cfg_far_z", 32, c.far_z, true);
    pin!("texture_base", 16, c.texture_base, true);
    pin!("texture_n", 4, c.texture_n, true);
    pin!("texture_mip", 1, c.texture_mip, true);
    pin!("texture_rgba4444", 1, c.texture_rgba4444, true);
    pin!("texture_filter", 2, c.texture_filter, true);
    pin!("texture_fb400240", 1, c.texture_framebuffer, true);
    pin!("cfg_lod_bias", 8, c.lod_bias as u8, true);
    pin!("draw_unlit", 1, c.unlit, true);
    pin!("color_base", 14, c.color_base, true);
    pin!("depth_base", 14, c.depth_base, true);
    pin!("mesh_color", 32, c.mesh_color, true);
    pin!("specular", 24, c.specular, true);
    pin!("depth_func", 3, c.depth_func, true);
    pin!("depth_write", 1, c.depth_write, true);
    pin!("blend", 1, c.blend, true);
    pin!("context_epoch", 16, c.epoch, true);
    pin!("context_mode", 2, c.mode, true);
    pin!("context_code", 5, c.code, true);
    pin!("light_x", 16, c.light[0] as u16, true);
    pin!("light_y", 16, c.light[1] as u16, true);
    pin!("light_z", 16, c.light[2] as u16, true);
    pin!("ray_x", 16, c.ray[0] as u16, true);
    pin!("ray_y", 16, c.ray[1] as u16, true);
    pin!("ray_k", 16, c.ray[2] as u16, true);
    pin!("ambient", 9, c.ambient, true);
    pin!("directional", 9, c.directional, true);
    pin!("obs_phase", 5, e.phase as u8, true);
    pin!("obs_pc", 6, e.pc, true);
    pin!("obs_cache_pages", 2, e.cache_pages, true);
    pin!("obs_global_valid", 1, e.status.0, true);
    pin!("obs_render_valid", 1, e.status.1, true);
    pin!("obs_target_valid", 1, e.status.2, true);
    pin!("obs_page_valid", 2, e.status.3, true);
    pin!("obs_uniform_valid", 4, e.status.4, true);
    pin!("obs_page_uniform", 4, e.status.5, true);
    pin!("obs_object_index", 2, e.status.6, true);
    p
}
pub fn vectors(r: &Run) -> (String, String) {
    let schema = fields(&r.trace[0])
        .iter()
        .map(|p| format!("{} {} {}\n", p.name, p.width, u8::from(p.output)))
        .collect();
    let mut rows = String::new();
    for e in &r.trace {
        for p in fields(e) {
            write!(rows, "{:x} ", p.value).unwrap();
        }
        rows.push('\n');
    }
    (schema, rows)
}
