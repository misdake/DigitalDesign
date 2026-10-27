use gpu_v2::{
    command_processor::{control::*, payload::*, ports::Command},
    scratchpad::ports::DmaDescriptor,
    vertex::ports::Context,
};

#[test]
fn checked_scalar_payload_preserves_full_descriptor_and_context_port() {
    let dma = DmaDescriptor {
        physical_addr: u64::MAX - 8191,
        scratchpad_addr: 4096,
        byte_count: 4096,
        completion_token: 3,
    };
    let prepared = PreparedCommand::new(Command::Dma(dma)).unwrap();
    let payload = RetainedPayload::from_prepared(&prepared);
    assert_eq!(
        payload.dma(),
        (dma.physical_addr, dma.scratchpad_addr, dma.byte_count)
    );
    assert_eq!(prepared.request().token, 3);
    assert_eq!(payload.bits() >> 90, 0);
    let context = Context::default();
    let prepared = PreparedCommand::new(Command::Draw {
        region: 1,
        byte_offset: 512,
        vertices: 64,
        context: context.clone(),
    })
    .unwrap();
    let draw = RetainedPayload::from_prepared(&prepared);
    assert_eq!(draw.draw(), (1, 512, 64));
    let Command::Draw {
        context: retained, ..
    } = prepared.payload()
    else {
        panic!("DRAW")
    };
    assert_eq!(retained.mvp, context.mvp);
    assert_eq!(retained.normal_matrix, context.normal_matrix);
    let prepared = PreparedCommand::new(Command::Release {
        slot: 1,
        epoch: u32::MAX,
    })
    .unwrap();
    assert_eq!(
        RetainedPayload::from_prepared(&prepared).release(),
        (1, u32::MAX)
    );
    for command in [Command::Wait(3), Command::Fence] {
        let prepared = PreparedCommand::new(command).unwrap();
        assert_eq!(RetainedPayload::from_prepared(&prepared).bits(), 0);
    }
    assert!(RetainedPayload::from_bits(1u128 << 90).is_none());

    // Execute the default physical owner with the checked DMA value. Change
    // the input bus after capture, then verify old ownership on acceptance.
    let mut model = CommandProcessorEmu::default();
    model.step(
        Inputs {
            reset: true,
            ..Inputs::default()
        },
        RetainedPayload::default(),
    );
    let prepared = PreparedCommand::new(Command::Dma(dma)).unwrap();
    let mut captured = false;
    for _ in 0..16 {
        let o = model.step(
            Inputs {
                ce: true,
                command: Some(prepared.request()),
                ..Inputs::default()
            },
            payload,
        );
        if o.control.command_capture {
            captured = true;
            break;
        }
    }
    assert!(captured);
    for ce in [false, true, false] {
        let o = model.step(
            Inputs {
                ce,
                ..Inputs::default()
            },
            draw,
        );
        assert!(o.owned);
        assert_eq!(o.payload, payload);
        assert_eq!(model.held(), payload);
    }
    let accepted = model.step(
        Inputs {
            ce: true,
            dma_ready: true,
            ..Inputs::default()
        },
        draw,
    );
    assert!(accepted.control.dma_accept && accepted.owned);
    assert_eq!(accepted.payload, payload);
    assert!(!model.owned());
    assert_eq!(model.held(), payload);
}
