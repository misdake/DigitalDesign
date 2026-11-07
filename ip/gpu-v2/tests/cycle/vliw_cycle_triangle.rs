#[path = "../support/vliw_triangle/mod.rs"]
mod support;
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
#[test]
fn one_triangle_reads_real_index_and_exports_sixteen_words() {
    let r = support::run(support::Case::default());
    assert_eq!(r.stats.words, 16);
    assert!(r.stats.held_words > 0);
}
#[test]
fn selected_t_numeric_rejection_and_modes() {
    for shape in 0..=7 {
        for normal18 in [false, true] {
            let _ = support::run(support::Case {
                shape,
                normal18,
                cull: shape == 3,
                rgb: normal18,
                slot: normal18,
                ..Default::default()
            });
        }
    }
}
#[test]
fn sixteen_slots_fill_wrap_and_do_not_borrow_full_credit() {
    for triangles in [17, 33] {
        let r = support::run(support::Case {
            triangles,
            hold_full: true,
            ..Default::default()
        });
        assert_eq!(r.stats.max_queue, 16);
        assert!(r.stats.full_edges > 12);
        assert_eq!(r.stats.words, triangles * 16);
    }
}

#[test]
#[ignore = "short production T32/T16 exact RTL replay"]
fn production_t32_index_and_export_match_every_edge() {
    let root = support::rtl::root();
    let r = support::run(support::Case::default());
    support::rtl::audit_mutations(&r);
    support::rtl::compare(&root, "one-triangle", &r);
    for shape in 0..=7 {
        support::rtl::compare(
            &root,
            &format!("shape-{shape}"),
            &support::run(support::Case {
                shape,
                cull: shape == 3,
                ..Default::default()
            }),
        );
    }
    support::rtl::compare(
        &root,
        "normal18-rgb-page1",
        &support::run(support::Case {
            normal18: true,
            rgb: true,
            slot: true,
            ..Default::default()
        }),
    );
    for triangles in [4, 17, 33] {
        support::rtl::compare(
            &root,
            &format!("triangles-{triangles}"),
            &support::run(support::Case {
                triangles,
                hold_full: triangles > 16,
                ..Default::default()
            }),
        );
    }
    support::rtl::compare(&root, "three-epochs", &support::epochs());
    for phase in ["queue", "Q", "word"] {
        support::rtl::compare(
            &root,
            &format!("reset-{phase}"),
            &support::reset_run(
                support::Case {
                    triangles: if phase == "queue" { 17 } else { 1 },
                    hold_full: phase == "queue",
                    ..Default::default()
                },
                phase,
            ),
        );
    }
    support::rtl::compare(
        &root,
        "max-signed26",
        &support::run(support::Case {
            shape: 10,
            ..Default::default()
        }),
    );
    let all_stalled = support::run(support::Case {
        stall_each: true,
        ..Default::default()
    });
    assert_eq!(all_stalled.stats.stalled_items, 0xffff);
    support::rtl::compare(&root, "every-item-stall", &all_stalled);
    let simultaneous = support::run(support::Case {
        triangles: 2,
        simultaneous: true,
        ..Default::default()
    });
    assert_eq!(simultaneous.stats.simultaneous, 1);
    support::rtl::compare(&root, "simultaneous-publish-release", &simultaneous);
    let fractions = support::run(support::Case {
        shape: 9,
        ..Default::default()
    });
    support::rtl::compare(&root, "fractional-large-domain", &fractions);
    assert!(
        fractions
            .trace
            .iter()
            .any(|e| e.t.sticky && e.vertex.regs.acc.value & (1 << 53) != 0),
        "negative discarded-bit witness"
    );
    let full = support::run(support::Case {
        triangles: 17,
        hold_full: true,
        ..Default::default()
    });
    let epochs = support::epochs();
    let reset = support::reset_run(
        support::Case {
            triangles: 17,
            hold_full: true,
            ..Default::default()
        },
        "queue",
    );
    for m in support::rtl::MUTATIONS {
        let trace = match m {
            "index_shift" | "group_early" => &full,
            "toward_zero" => &fractions,
            "full_borrow" => &full,
            "epoch_admit" | "epoch_color" => &epochs,
            "stale_reset" => &reset,
            _ => &r,
        };
        support::rtl::mutant(&root, m, trace);
    }
    for name in [
        "too_many",
        "zero_vertices",
        "offset",
        "refs",
        "cursor",
        "position",
        "cp_owner",
        "active_page",
    ] {
        let r = support::fault(name);
        assert!(r.error.is_some());
        support::rtl::contract(&root, name, &r);
    }
    let native = support::native::run();
    support::rtl::compare(&root, "native-T16", &native.run);
    transport_support::rtl(
        &root,
        "native-transport",
        &native.transport,
        None,
        true,
        false,
    );
    support::rtl::compare(
        &root,
        "negative-kept",
        &support::run(support::Case {
            shape: 3,
            ..Default::default()
        }),
    );
    std::fs::write(root.join("receipt.json"),format!("{{\"scope\":\"production index/T32/T16 subset\",\"positive_cases\":24,\"actual_dut_mutants\":13,\"illegal_peers\":8,\"slots\":16,\"full_case_triangles\":17,\"wrap_case_triangles\":33,\"three_epoch_blocked_enabled\":{},\"item15_stalls_enabled\":{},\"V_overlap_old_export\":{},\"native_logic_edges\":{},\"native_cycles\":{},\"native_dma_qwords\":{},\"native_T16_words\":{},\"native_overlapping_dma_writes\":{},\"full_GPU_or_root\":false}}\n",epochs.stats.epoch_blocked,epochs.stats.last_word_stalls,epochs.stats.overlapping_v,native.run.trace.len(),native.native_cycles,native.dma_qwords,native.run.stats.words,native.overlapping_writes)).unwrap();
    println!("G3A QUALIFIED {}", root.display());
}

#[test]
fn third_epoch_waits_for_last_indexed_export_while_positions_are_reused() {
    let r = support::epochs();
    assert_eq!(r.stats.v_writes, 45);
    assert_eq!(r.stats.words, 48);
}
#[test]
fn reset_invalidates_full_queue_q_and_held_word_but_retains_rf() {
    for phase in ["queue", "Q", "word"] {
        let r = support::reset_run(
            support::Case {
                triangles: if phase == "queue" { 17 } else { 1 },
                hold_full: phase == "queue",
                ..Default::default()
            },
            phase,
        );
        assert_eq!(r.stats.resets, 1);
    }
}

#[test]
fn native_memory_supplies_real_index_and_t16_without_functional_fallback() {
    let r = support::native::run();
    println!("G3A_SAMPLE {{\"initialization_us\":{},\"stepping_us\":{},\"logic_edges\":{},\"native_cycles\":{},\"dma_qwords\":{},\"T16_words\":{}}}",r.initialization_us,r.stepping_us,r.run.trace.len(),r.native_cycles,r.dma_qwords,r.run.stats.words);
    assert_eq!(r.run.stats.v_writes, 30);
    assert!(!r.transport.is_empty());
}

#[test]
fn production_illegal_input_stops_at_declared_boundary_with_prefix() {
    for name in [
        "too_many",
        "zero_vertices",
        "offset",
        "refs",
        "cursor",
        "position",
        "cp_owner",
        "active_page",
    ] {
        let r = support::fault(name);
        assert!(r.error.is_some(), "{name}");
        if name == "cursor" {
            assert_eq!(r.stats.publish, 3);
        }
        if ["refs", "position", "cp_owner", "active_page"].contains(&name) {
            assert_eq!(r.stats.v_writes, 15);
        }
    }
}

#[test]
fn maximum_signed26_and_every_item_stall_preserve_paid_reads() {
    let r = support::run(support::Case {
        shape: 10,
        ..Default::default()
    });
    assert_eq!(r.stats.words, 16);
    let r = support::run(support::Case {
        stall_each: true,
        ..Default::default()
    });
    assert_eq!(r.stats.stalled_items, 0xffff);
}

#[test]
fn simultaneous_publish_and_last_word_release_commit_old_cursors() {
    let r = support::run(support::Case {
        triangles: 2,
        simultaneous: true,
        ..Default::default()
    });
    assert_eq!(r.stats.simultaneous, 1);
    assert_eq!(r.stats.words, 32);
}
