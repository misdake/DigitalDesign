//! Split publication holds ownership before either hardware SEND.
use super::*;

#[test]
fn prepared_input_is_not_completed_cancelled_or_submitted_by_polling() {
    let body = r#"
check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();check(gpu_pass_clear(cb,1,2));
check(gpu_display_render_start(GPU_CB.color));
check(gpu_driver_prepare_idle(view_of(&mut GPU_CB))==GPU_PREPARED);
check(GPU_CB.state==GPU_CB_PREPARED && !gpu_driver_can_build());
check(gpu_driver_owned_buffer().addr()==addr_of(&GPU_CB).addr());
check(gpu_driver_poll(view_of(&mut GPU_CB))==GPU_PREPARED);
check(gpu_driver_poll(view_of(&mut GPU_CB))==GPU_PREPARED);
check(!gpu_cb_cancel(cb) && !gpu_discard_ready(cb));
check(!gpu_buffer_reset(view_of(&mut GPU_CB)) && gpu_cb_begin()==0);
check(!gpu_slot_upload(view_of(&mut GPU_CB),false,0,GPU_CODE.as_array(),0));
check(gpu_driver_prepare_idle(view_of(&mut GPU_CB))==GPU_REJECTED);
signal(1,1);
check(gpu_driver_start_prepared(view_of(&mut GPU_CB),true)==GPU_PENDING);
check(gpu_driver_start_prepared(view_of(&mut GPU_CB),true)==GPU_REJECTED);
dev_send(7,0,0);check(gpu_poll(cb)==GPU_COMPLETED);check(gpu_poll(cb)==GPU_IDLE);
check(gpu_discard_ready(cb));
let next=gpu_cb_begin();check(gpu_pass_clear(next,3,4));
check(gpu_submit_idle(next)==GPU_PENDING);dev_send(7,0,0);check(gpu_poll(next)==GPU_COMPLETED);
"#;
    let words = compile(&format!("{CONTROL_SETUP}{body}halt(0x600d);}}"));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&words);
    let state = Rc::new(RefCell::new(PeerState::default()));
    for kind in [0, 3, 4, 7] {
        cpu.attach_device(
            kind,
            Box::new(ControlPeer {
                state: state.clone(),
                kind,
            }),
        );
    }
    // SIGNAL yields to the host after publication, with no Device4 write.
    let mut published = false;
    for _ in 0..1_000_000 {
        let outcome = cpu.step().unwrap();
        if matches!(outcome, cpu_v3::StepOutcome::Signaled(_)) {
            published = true;
            break;
        }
        assert!(!matches!(outcome, cpu_v3::StepOutcome::Halted { .. }));
    }
    assert!(
        published,
        "publication checkpoint was not reached within the bound"
    );
    assert_eq!(state.borrow().counts, 0);
    assert_eq!(state.borrow().starts, 0);
    assert_eq!(state.borrow().maintenance, 1);
    assert!(matches!(
        cpu.run(1_000_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
    assert_eq!(state.borrow().counts, 2);
    assert_eq!(state.borrow().starts, 2);
    assert_eq!(state.borrow().maintenance, 3);
}

#[test]
fn resume_failures_keep_prepared_ownership_and_send_nothing() {
    for (stimulus, result) in [
        ("dev_send(7,0,5);", "GPU_CACHE_ERROR"),
        ("dev_send(7,0,3);", "GPU_FAULT"),
        ("dev_send(7,0,1);", "GPU_FAULT"),
        ("GPU_CB.command_words+=2;", "GPU_FAULT"),
    ] {
        let body = format!(
            r#"
check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();check(gpu_pass_clear(cb,1,2));
check(gpu_display_render_start(GPU_CB.color));
check(gpu_driver_prepare_idle(view_of(&mut GPU_CB))==GPU_PREPARED);
{stimulus}
check(gpu_driver_start_prepared(view_of(&mut GPU_CB),true)=={result});
check(GPU_CB.state==GPU_CB_PREPARED);
check(gpu_driver_owned_buffer().addr()==addr_of(&GPU_CB).addr());
check(!gpu_cb_cancel(cb) && !gpu_discard_ready(cb));
check(gpu_driver_poll(view_of(&mut GPU_CB))==GPU_FAULT);
"#
        );
        let words = compile(&format!("{CONTROL_SETUP}{body}halt(0x600d);}}"));
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&words);
        let state = Rc::new(RefCell::new(PeerState::default()));
        for kind in [0, 3, 4, 7] {
            cpu.attach_device(
                kind,
                Box::new(ControlPeer {
                    state: state.clone(),
                    kind,
                }),
            );
        }
        assert!(
            matches!(
                cpu.run(1_000_000).unwrap(),
                RunOutcome::Halted { signal: 0x600d, .. }
            ),
            "{stimulus}"
        );
        assert_eq!(state.borrow().counts, 0);
        assert_eq!(state.borrow().starts, 0);
    }
}
