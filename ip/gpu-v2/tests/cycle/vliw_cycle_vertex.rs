#[path = "../support/vliw_vertex/mod.rs"]
mod support;
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;

#[test]
#[ignore = "short exact World/View preparation RTL differential"]
fn world_preparation_keeps_legacy_vertex_results_with_dma_and_stalls() {
    let root = support::rtl::root();
    for (name, vertices, slot) in [("world-three", 3, false), ("world-full", 64, true)] {
        let r = support::run(support::Case {
            world: true,
            vertices,
            slot,
            normal18: slot,
            rgb: slot,
            ..Default::default()
        });
        assert_eq!(r.stats.writes, vertices * 5);
        support::rtl::compare(&root, name, &r);
    }
}
#[test]
fn production_three_vertices_are_fifteen_writes_from_real_input_storage() {
    let r = support::run(support::Case::default());
    assert_eq!(r.stats.writes, 15);
    assert!(r.stats.ce0_dma > 0 && r.stats.ce0_busy > 0 && r.stats.stalled_emit > 0);
}
#[test]
fn both_pages_modes_counts_and_bounds_use_original_schedule() {
    for n in [0, 1, 3, 64] {
        for mode in [false, true] {
            let r = support::run(support::Case {
                vertices: n,
                normal18: mode,
                rgb: mode,
                slot: mode,
                ..Default::default()
            });
            assert_eq!(r.stats.writes, n * 5);
        }
    }
    for uniform in [21, 22, 23] {
        let r = support::run(support::Case {
            uniform,
            ..Default::default()
        });
        assert_eq!(r.stats.writes, 15);
    }
    for cull in 1..=4 {
        let r = support::run(support::Case {
            cull,
            ..Default::default()
        });
        assert_eq!(r.stats.writes, if cull < 4 { 0 } else { 15 });
    }
}
#[test]
fn source_work_bypass_reset_and_unwritten_cells_keep_knownness() {
    use gpu_v2::system::vliw_cycle::vertex::{Bits, InputRam, RamPorts};
    let mut r = InputRam::default();
    r.clock(RamPorts {
        reset: true,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(r.source_q(), Bits::new(0, 64));
    assert_eq!(r.peek(7)[0].known, 0);
    r.clock(RamPorts {
        dma_write: true,
        dma_address: 7,
        dma_data: 0x8123_4567_fedc_ba98,
        ..Default::default()
    })
    .unwrap();
    r.clock(RamPorts {
        left_read: true,
        right_read: true,
        left_address: 7,
        right_address: 7,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(r.source_q(), Bits::new(0x8123_4567_fedc_ba98, 64));
    r.clock(RamPorts {
        work_write: true,
        work_address: 384,
        work_data: Bits::new(0x123456789, 36),
        ..Default::default()
    })
    .unwrap();
    r.clock(RamPorts {
        right_read: true,
        right_address: 384,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(r.right_q(), Bits::new(0x123456789, 36));
    assert_eq!(r.source_q(), Bits::new(0x8123_4567_fedc_ba98, 64));
    r.clock(RamPorts {
        reset: true,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(r.peek(7)[0].slice(0, 32), Bits::new(0xfedcba98, 32));
    assert_eq!(r.source_q(), Bits::new(0, 64));
    assert!(r
        .clock(RamPorts {
            right_read: true,
            right_address: 384,
            ..Default::default()
        })
        .is_err());
    assert!(r
        .clock(RamPorts {
            dma_write: true,
            dma_address: 7,
            left_read: true,
            left_address: 7,
            ..Default::default()
        })
        .is_err());
}
#[test]
#[ignore = "short exact production bounds/Vertex RTL differential"]
fn production_vertex_old_state_matches_every_sample() {
    let root = support::rtl::root();
    let r = support::run(support::Case::default());
    support::rtl::audit_mutation_anchors(&r);
    support::rtl::compare(&root, "three-vertices", &r);
    for uniform in [21, 22, 23] {
        support::rtl::compare(
            &root,
            &format!("uniform-{uniform}"),
            &support::run(support::Case {
                uniform,
                ..Default::default()
            }),
        );
    }
    for (name, case) in [
        (
            "normal18-page1",
            support::Case {
                normal18: true,
                rgb: true,
                slot: true,
                ..Default::default()
            },
        ),
        (
            "zero",
            support::Case {
                vertices: 0,
                ..Default::default()
            },
        ),
        (
            "sixty-four",
            support::Case {
                vertices: 64,
                ..Default::default()
            },
        ),
        (
            "behind",
            support::Case {
                cull: 1,
                ..Default::default()
            },
        ),
        (
            "left",
            support::Case {
                cull: 2,
                ..Default::default()
            },
        ),
        (
            "right",
            support::Case {
                cull: 3,
                ..Default::default()
            },
        ),
        (
            "ineligible",
            support::Case {
                cull: 4,
                ..Default::default()
            },
        ),
    ] {
        let r = support::run(case);
        support::rtl::compare(&root, name, &r);
    }
    let native = support::native::run();
    support::rtl::compare(&root, "native-pages-cache-vertex", &native.vertex);
    transport_support::rtl(
        &root,
        "native-pages-cache-transport",
        &native.transport,
        None,
        true,
        false,
    );
    support::rtl::compare(
        &root,
        "unsupported-prefix",
        &support::invalid("unsupported"),
    );
    support::rtl::compare(&root, "warm-reset-resubmit", &support::warm());
    for m in support::rtl::MUTATIONS {
        support::rtl::qualify(&root, &format!("mutant-{m}"), &r, Some(m), false);
    }
    for fault in [
        "bad_header",
        "bad_uniform",
        "bad_dma",
        "cp_owner",
        "active_payload",
        "active_metadata",
        "active_uniform",
        "collision",
    ] {
        support::rtl::qualify(
            &root,
            &format!("contract-{fault}"),
            &support::invalid(fault),
            None,
            true,
        );
    }
    std::fs::write(root.join("receipt.json"),format!("{{\"native_logic_edges\":{},\"native_cycles\":{},\"dma_qwords\":{},\"vertex_writes\":{},\"overlap_page_qwords\":{},\"overlap_cache_qwords\":{},\"positive_or_prefix_cases\":15,\"actual_dut_mutants\":13,\"invalid_peers\":8,\"full_system\":false}}\n",native.vertex.trace.len(),native.native_cycles,native.dma_words,native.vertex.stats.writes,native.overlapping_page_writes,native.overlapping_cache_writes)).unwrap();
    println!("G2A QUALIFIED {}", root.display());
}

#[test]
#[ignore = "short billboard schedule differential against production RTL"]
fn billboard_schedule_selects_from_metadata_and_matches_every_sample() {
    let root = support::rtl::root();
    let r = support::run(support::Case {
        vertices: 5,
        billboard: true,
        cull: 3,
        slot: true,
        ..Default::default()
    });
    assert_eq!(r.stats.writes, 25);
    support::rtl::compare(&root, "billboard", &r);
}

#[test]
fn billboard_bootstrap_before_after_preserves_initialized_outputs() {
    use gpu_v2::system::vliw_cycle::vertex::{Bits, Error, Input, Meshlet};

    fn run(vertices: usize, old_rom: bool, tail: Option<u32>) -> (usize, Vec<Bits>, Option<Error>) {
        let case = support::Case {
            vertices,
            billboard: true,
            uniform: 22,
            slot: true,
            ..Default::default()
        };
        let mut source = support::data(case);
        let tail_address = u16::from(case.uniform) * 16 + 15;
        source.rows.retain(|&(address, _)| address != tail_address);
        if let Some(value) = tail {
            source.rows.push((tail_address, u64::from(value)));
        }
        let mut model = Meshlet::default();
        let mut loaded = 0;
        let mut launched = false;
        let mut output = Vec::new();
        for wall in 0..10000 {
            let mut input = Input {
                reset: wall < 2,
                ce: wall % 9 > 2,
                output_ready: wall % 13 > 3,
                slot: case.slot,
                mesh_color: 0xffa1_b2c3,
                ..Default::default()
            };
            if wall >= 2 && loaded < source.rows.len() {
                (input.dma_address, input.dma_data) = source.rows[loaded];
                input.dma_write = true;
                input.ce = false;
                loaded += 1;
            }
            if loaded == source.rows.len()
                && !launched
                && model.outputs(input).ready
                && !input.dma_write
            {
                input.start = true;
                input.ce = true;
                launched = true;
            }
            if old_rom {
                // Original bootstrap words, before the explicit zero-unpack.
                match model.core.regs.pc {
                    327 => model.core.regs.control = 0x1e1,
                    328 => model.core.regs.control = 0x3000,
                    _ => {}
                }
            }
            let observed = model.outputs(input);
            if observed.core.position_write || observed.core.attribute_write {
                assert_eq!(observed.core.emit_address as usize, output.len());
                output.push(observed.core.emit_data);
            }
            if let Err(error) = model.clock(input) {
                return (wall, output, Some(error));
            }
            if observed.done {
                assert_eq!(output.len(), vertices * 5);
                return (wall, output, None);
            }
        }
        panic!("Billboard comparison exceeded cycle limit");
    }

    for vertices in [1, 5, 64] {
        let baseline = run(vertices, false, Some(0));
        for tail in [Some(0), None, Some(0x1234_5678)] {
            let before = run(vertices, true, tail);
            let after = run(vertices, false, tail);
            assert_eq!(after.2, None);
            assert_eq!(after.1, baseline.1, "fixed output depends on uniform tail");
            if tail.is_none() {
                assert_eq!(before.2, Some(Error::Unknown("valid Vertex result")));
                assert_eq!(before.1.len(), 4);
                assert_eq!(before.1[..3], after.1[..3]);
                assert!(!before.1[3].is_known(36));
                println!("vertices={vertices} tail=unwritten: old Vertex error at cycle {}, fixed completes at cycle {}", before.0, after.0);
                continue;
            }
            assert_eq!(before.2, None);
            assert_eq!(before.0, after.0, "bootstrap cycle count");
            let mut differences = 0;
            let mut unknown = 0;
            for (field, (&old, &new)) in before.1.iter().zip(&after.1).enumerate() {
                if old != new {
                    assert_eq!(field % 5, 3, "position or UV/color changed");
                    differences += 1;
                    unknown += usize::from(!old.is_known(36));
                }
                assert!(new.is_known(36));
            }
            assert_eq!(differences, if tail == Some(0) { 0 } else { vertices });
            assert_eq!(unknown, if tail.is_none() { vertices } else { 0 });
            println!("vertices={vertices} tail={tail:?} before/after_cycles={} differing_words={differences} old_unknown_words={unknown}", before.0);
        }
    }
}

#[test]
fn native_memory_supplies_both_pages_uniforms_and_parallel_cache() {
    let r = support::native::run();
    println!("G2A_SAMPLE {{\"initialization_us\":{},\"stepping_us\":{},\"logic_edges\":{},\"native_cycles\":{},\"trace_collection\":true}}",r.initialization_us,r.stepping_us,r.vertex.trace.len(),r.native_cycles);
    assert_eq!(r.vertex.stats.writes, 30);
    assert!(r.native_cycles > 0 && r.vertex.error.is_none());
}
#[test]
fn unsupported_and_illegal_owners_preserve_prefix_without_vertex_results() {
    use gpu_v2::system::vliw_cycle::vertex::Error;
    for fault in [
        "unsupported",
        "bad_header",
        "bad_uniform",
        "bad_dma",
        "cp_owner",
        "active_payload",
        "active_metadata",
        "active_uniform",
        "collision",
    ] {
        let r = support::invalid(fault);
        assert_eq!(r.stats.writes, 0, "{fault}");
        assert!(r.error.is_some(), "{fault}");
        assert!(!r.trace.iter().any(|e| e.output.done));
        if fault == "unsupported" {
            assert!(matches!(
                r.error,
                Some(Error::UnsupportedTriangles {
                    count: 1,
                    slot: false
                })
            ));
            assert_eq!(
                r.stats.ce0_dma,
                support::data(support::Case::default()).rows.len()
            );
        }
    }
}

#[test]
fn explicit_triangle_launch_is_unsupported_before_state_changes() {
    use gpu_v2::system::vliw_cycle::vertex::{Core, CoreInput, Error};
    let mut c = Core::default();
    let old = c.rf_word(0);
    assert_eq!(
        c.clock(CoreInput {
            ce: true,
            start_t: true,
            ..Default::default()
        }),
        Err(Error::UnsupportedStartT)
    );
    assert_eq!(c.rf_word(0), old);
    assert!(!c.regs.busy);
}

#[test]
fn warm_reset_resubmits_retained_input_and_preserves_storage_contract() {
    assert_eq!(support::warm().stats.writes, 30);
}
