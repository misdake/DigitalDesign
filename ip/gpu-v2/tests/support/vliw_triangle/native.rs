//! Existing P1a transport and a single native Memory service supply all index data.
use super::*;
use crate::transport_support;
use gpu_v2::system::vliw_cycle::{
    command_dma::{Cache, Work},
    Input as TransportInput, Transport,
};
pub struct NativeRun {
    pub run: Run,
    pub transport: Vec<transport_support::Edge>,
    pub native_cycles: u64,
    pub dma_qwords: usize,
    pub overlapping_writes: usize,
    pub initialization_us: u128,
    pub stepping_us: u128,
}
pub fn run() -> NativeRun {
    let start = std::time::Instant::now();
    let cases = [
        Case::default(),
        Case {
            slot: true,
            uniform: 21,
            shape: 5,
            normal18: true,
            rgb: true,
            color: 0xfacd_4231,
            ..Default::default()
        },
    ];
    let data = cases.map(data);
    let packets: Vec<_> = data
        .iter()
        .flat_map(|d| d.packets.iter().copied())
        .collect();
    let mut memory = transport_support::native::Memory::new();
    let mut expected = [None; 512];
    for (frame, d) in data.iter().enumerate() {
        for &(a, value) in &d.rows {
            expected[a as usize] = Some(value);
            let physical = if a >= 320 {
                0x1000 + frame * 64 + (a as usize - usize::from(cases[frame].uniform) * 16) * 4
            } else if a >= 256 {
                0x1400 + frame * 0x400 + (a as usize - 256 - frame * 4) * 4
            } else {
                0x1440 + frame * 0x400 + (a as usize - frame * 128) * 4
            };
            memory.preload(
                physical,
                &std::array::from_fn::<_, 4, _>(|n| (value >> (n * 16)) as u16),
            );
        }
    }
    for beat in 0..16 {
        let value = 0xa187_3421_3456_0010u64 + beat as u64;
        expected[448 + beat] = Some(value);
        memory.preload(
            0x1c00 + beat * 4,
            &std::array::from_fn::<_, 4, _>(|n| (value >> (n * 16)) as u16),
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
    let mut m = Meshlet::with_triangles();
    let mut result = NativeRun {
        run: Run {
            error: None,
            trace: Vec::new(),
            stats: Stats::default(),
        },
        transport: Vec::new(),
        native_cycles: 0,
        dma_qwords: 0,
        overlapping_writes: 0,
        initialization_us: start.elapsed().as_micros(),
        stepping_us: 0,
    };
    let mut jobs = 0;
    let mut uniform_done = [false; 2];
    let mut mesh_done = [false; 2];
    let mut cache_accepted = false;
    let mut cache_done = false;
    let mut started = 0;
    let mut completed = 0;
    let mut writes = [0usize; 2];
    let mut wi = 0;
    let mut cp = 0;
    let stepping = std::time::Instant::now();
    for wall in 0..200000 {
        let reset = wall < 2;
        let ce = wall % 9 > 2;
        let mut work = works[jobs.min(3)];
        work.valid = !reset && jobs < 4 && (jobs < 3 || started > 0);
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
        let color = cases[if started > completed {
            started - 1
        } else {
            started.min(1)
        }]
        .color;
        let mut i = Input {
            reset,
            ce,
            slot: started == 1,
            mesh_color: color,
            output_ready: wall % 13 > 3,
            packet_ready: wall % 7 > 1,
            word_ready: wall % 17 > 5,
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
            && !m.input_active
            && m.outputs(i).ready
            && ce
        {
            i.start = true;
        }
        if completed == 2
            && cache_done
            && to.dma.idle
            && !to.router.busy
            && cp == 0
            && !m.input_active
            && ce
        {
            i.cp_read = true;
            i.cp_address = 448;
        }
        let e = edge(&m, i);
        let o = e.vertex.output;
        if i.dma_write {
            assert!(o.dma_ready, "native unstallable input");
            assert_eq!(
                Some(i.dma_data),
                expected[i.dma_address as usize],
                "native source address{}",
                i.dma_address
            );
            result.dma_qwords += 1;
            if m.core.regs.busy {
                result.overlapping_writes += 1;
            }
        }
        if i.ce && m.phase == Phase::TStart && o.core.triangle_ready {
            result.run.stats.admitted += 1;
        }
        if o.core.position_write || o.core.attribute_write {
            let frame = started - 1;
            let n = writes[frame];
            assert_eq!(o.core.emit_address, n as u16);
            assert_eq!(
                o.core.emit_data.get(36, "native V").unwrap(),
                data[frame].vertices[n]
            );
            writes[frame] += 1;
            result.run.stats.v_writes += 1;
        }
        if o.core.coefficient_write {
            assert_eq!(
                o.core
                    .coefficient_data
                    .get(54, "native T coefficient")
                    .unwrap(),
                data[started - 1].per_triangle[0].unwrap()
                    [(o.core.coefficient_address >> 4) as usize]
            );
        }
        if o.core.publish {
            result.run.stats.publish += 1;
        }
        if o.core.packet_take {
            assert_eq!(
                o.core.descriptor.get(32, "native descriptor").unwrap(),
                u64::from(packets[result.run.stats.packets].0)
            );
            result.run.stats.packets += 1;
            wi = 0;
        }
        if o.core.word_take {
            assert_eq!(o.core.word_index, wi);
            assert_eq!(
                o.core.word_data.get(54, "native T16").unwrap(),
                packets[result.run.stats.packets - 1].1[wi as usize]
            );
            wi += 1;
            result.run.stats.words += 1;
        }
        result.run.trace.push(e);
        result.transport.push(transport_support::Edge {
            input: ti,
            output: to,
        });
        transport.clock(ti).unwrap();
        m.clock(i)
            .unwrap_or_else(|e| panic!("native wall{wall}: {e}"));
        memory.clock();
        if !reset {
            if ce && work.valid && to.dma.work_ready {
                jobs += 1;
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
                assert_eq!(writes[completed], 15);
                completed += 1;
            }
            if i.cp_read {
                cp = 1;
            } else if cp == 1 {
                assert_eq!(
                    o.cp_q.get(64, "native CP cache read").unwrap(),
                    expected[448].unwrap()
                );
                cp = 2;
            }
        }
        if cp == 2 && o.drained {
            assert_eq!(result.dma_qwords, 88);
            assert_eq!(result.run.stats.words, 32);
            assert_eq!(result.run.stats.publish, 2);
            assert!(result.overlapping_writes > 0);
            result.native_cycles = memory.native_cycles();
            result.stepping_us = stepping.elapsed().as_micros();
            return result;
        }
    }
    panic!("native T input closure exceeded200000 logic edges")
}
