//! Native Memory feedback drives P1a transport; its actual DMA beats feed G2a.
//! The two production RTL cones separately replay the same edge tape.
use super::*;
use crate::transport_support;
use gpu_v2::system::vliw_cycle::{
    command_dma::{Cache, Work},
    Input as TransportInput, Transport,
};

pub struct NativeRun {
    pub initialization_us: u128,
    pub stepping_us: u128,
    pub vertex: Run,
    pub transport: Vec<transport_support::Edge>,
    pub native_cycles: u64,
    pub overlapping_page_writes: usize,
    pub overlapping_cache_writes: usize,
    pub dma_words: usize,
}
pub fn run() -> NativeRun {
    let initial = std::time::Instant::now();
    let cases = [
        Case::default(),
        Case {
            slot: true,
            uniform: 21,
            normal18: true,
            rgb: true,
            ..Default::default()
        },
    ];
    let datasets = cases.map(data);
    let mut memory = transport_support::native::Memory::new();
    let mut expected = [None; 512];
    for (frame, d) in datasets.iter().enumerate() {
        for &(a, v) in &d.rows {
            expected[a as usize] = Some(v);
            let physical = if a >= 320 {
                0x1000 + frame * 64 + (a as usize - usize::from(cases[frame].uniform) * 16) * 4
            } else if a >= 256 {
                0x1400 + frame * 0x400 + (a as usize - 256 - frame * 4) * 4
            } else {
                0x1440 + frame * 0x400 + (a as usize - frame * 128) * 4
            };
            let words = std::array::from_fn::<_, 4, _>(|k| (v >> (k * 16)) as u16);
            memory.preload(physical, &words);
        }
    }
    for beat in 0..16 {
        let value = 0x5678_9000_0000_00abu64 + beat as u64;
        expected[448 + beat] = Some(value);
        memory.preload(
            0x1c00 + beat * 4,
            &std::array::from_fn::<_, 4, _>(|k| (value >> (k * 16)) as u16),
        );
    }
    let works = [
        Work {
            source_halfword: 0x1000,
            destination: 80,
            size: 2,
            ..Default::default()
        },
        Work {
            source_halfword: 0x1040,
            destination: 84,
            size: 2,
            ..Default::default()
        },
        Work {
            source_halfword: 0x1400,
            mesh: true,
            ..Default::default()
        },
        Work {
            source_halfword: 0x1800,
            mesh: true,
            slot: true,
            ..Default::default()
        },
    ];
    let mut transport = Transport::default();
    let mut model = Meshlet::default();
    let mut result = NativeRun {
        initialization_us: initial.elapsed().as_micros(),
        stepping_us: 0,
        vertex: Run {
            trace: Vec::new(),
            stats: Stats::default(),
            error: None,
        },
        transport: Vec::new(),
        native_cycles: 0,
        overlapping_page_writes: 0,
        overlapping_cache_writes: 0,
        dma_words: 0,
    };
    let mut accepted = 0;
    let mut uniform_done = [false; 2];
    let mut mesh_done = [false; 2];
    let mut cache_accepted = false;
    let mut cache_done = false;
    let mut started = 0;
    let mut completed = 0;
    let mut writes = [0; 2];
    let mut cp = 0;
    let stepping = std::time::Instant::now();
    for wall in 0..200000 {
        let reset = wall < 2;
        let ce = wall % 9 > 2;
        let mut work = works[accepted.min(3)];
        work.valid = !reset && accepted < 4 && (accepted < 3 || started > 0);
        let mut ti = TransportInput {
            reset,
            ce,
            work,
            cache: Cache {
                valid: started > 0 && !cache_accepted,
                source_halfword: 0x1c00,
                bank: false,
            },
            ..Default::default()
        };
        ti.physical = memory.feedback(transport.outputs(ti).router, reset);
        let to = transport.outputs(ti);
        let mut i = Input {
            reset,
            ce,
            slot: started == 1,
            mesh_color: 0xffa1_b2c3,
            output_ready: wall % 13 > 3,
            dma_write: to.dma.ram_write,
            dma_address: to.dma.ram_address,
            dma_data: to.dma.ram_data,
            ..Default::default()
        };
        if !reset
            && started < 2
            && completed == started
            && uniform_done[started]
            && mesh_done[started]
            && !model.input_active
            && model.outputs(i).ready
            && ce
        {
            i.start = true;
        }
        if completed == 2
            && cache_done
            && to.dma.idle
            && !to.router.busy
            && cp == 0
            && !model.input_active
            && ce
        {
            i.cp_read = true;
            i.cp_address = 448;
        }
        let e = edge(&model, i);
        let o = e.output;
        result.vertex.trace.push(e);
        result.transport.push(transport_support::Edge {
            input: ti,
            output: to,
        });
        if i.dma_write {
            assert!(o.dma_ready, "unstallable native beat wall{wall}");
            assert_eq!(
                Some(i.dma_data),
                expected[i.dma_address as usize],
                "native backing identity address{}",
                i.dma_address
            );
            result.dma_words += 1;
            if !ce {
                result.vertex.stats.ce0_dma += 1;
            }
            if model.core.regs.busy && i.dma_address < 256 {
                result.overlapping_page_writes += 1;
            }
            if model.core.regs.busy && i.dma_address >= 448 {
                result.overlapping_cache_writes += 1;
            }
        }
        if o.core.emit_valid && (!ce || !i.output_ready) {
            result.vertex.stats.stalled_emit += 1;
        }
        if o.busy && !ce {
            result.vertex.stats.ce0_busy += 1;
        }
        if o.core.position_write || o.core.attribute_write {
            let frame = started - 1;
            let n = writes[frame];
            assert_eq!(o.core.emit_address, n as u16);
            assert_eq!(
                o.core.emit_data.get(36, "native V result").unwrap(),
                datasets[frame].expected[n]
            );
            let field = n % 5;
            let v = n / 5;
            let epoch = usize::from(frame == 0);
            if field < 3 {
                assert!(o.core.position_write);
                assert_eq!(o.core.position_address, (field * 64 + v) as u16);
                result.vertex.stats.position_writes += 1;
            } else {
                assert!(o.core.attribute_write);
                assert_eq!(
                    o.core.attribute_address,
                    (256 + (field - 3) * 128 + epoch * 64 + v) as u16
                );
                result.vertex.stats.attribute_writes += 1;
            }
            writes[frame] += 1;
            result.vertex.stats.writes += 1;
        }
        transport.clock(ti).unwrap();
        model
            .clock(i)
            .unwrap_or_else(|e| panic!("native G2 wall{wall}: {e}"));
        memory.clock();
        if !reset {
            if ce && work.valid && to.dma.work_ready {
                accepted += 1;
            }
            if ti.cache.valid && to.dma.cache_ready {
                cache_accepted = true;
            }
            if to.dma.uniform_done {
                uniform_done[to.dma.uniform_bank as usize] = true;
            }
            if to.dma.mesh_done {
                mesh_done[usize::from(to.dma.mesh_done_slot)] = true;
            }
            if to.dma.cache_done {
                cache_done = true;
            }
            if i.start {
                started += 1;
            }
            if o.done && ce {
                assert_eq!(o.core.published_vertices, 3);
                assert_eq!(writes[completed], 15);
                completed += 1;
            }
            if i.cp_read {
                cp = 1;
            } else if cp == 1 {
                assert_eq!(
                    o.cp_q.get(64, "cache read after drain").unwrap(),
                    expected[448].unwrap()
                );
                cp = 2;
            }
        }
        if cp == 2 {
            assert_eq!(accepted, 4);
            assert_eq!(result.dma_words, 88);
            assert!(
                result.overlapping_page_writes > 0 && result.overlapping_cache_writes > 0,
                "native overlap page{} cache{}",
                result.overlapping_page_writes,
                result.overlapping_cache_writes
            );
            for (frame, d) in datasets.iter().enumerate() {
                for n in 0..15 {
                    let field = n % 5;
                    let v = n / 5;
                    let got = if field < 3 {
                        model.core.position((field * 64 + v) as u16)
                    } else {
                        model.core.attribute(
                            (256 + (field - 3) * 128 + usize::from(frame == 0) * 64 + v) as u16,
                        )
                    };
                    if field >= 3 || frame == 1 {
                        assert_eq!(
                            got.get(36, "native retained result").unwrap(),
                            d.expected[n]
                        );
                    }
                }
            }
            result.native_cycles = memory.native_cycles();
            result.stepping_us = stepping.elapsed().as_micros();
            return result;
        }
    }
    panic!("native input/V closure exceeded200000 logic edges")
}
