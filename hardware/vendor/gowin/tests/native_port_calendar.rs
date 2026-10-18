//! Direct CPU-port calendar against the independent native/pin engine.
use digital_design_hardware_gowin::sdram_memory_controller::{
    emu::{adapter, bridge},
    ports::OracleImage,
    shared_port::SharedSdramPortInputValue as Input,
};
fn pins(engine: &bridge::Engine) -> Input {
    let b = engine.output(false);
    Input {
        reset: false,
        cpu_request_valid: false,
        cpu_write: false,
        cpu_line: true,
        cpu_address: 0,
        cpu_line_count_minus_1: 0,
        cpu_write_data: 0x1122334455667788,
        cpu_response_ready: true,
        controller_read_data: b.read_data,
        controller_read_valid: b.read_valid,
        controller_init_done: b.initialized,
        controller_request_ready: b.ready,
        controller_stream_active: b.stream_active,
        controller_done: b.done,
        controller_write_data_ready: b.data_ready,
    }
}
fn step(a: &mut adapter::State, b: &mut bridge::Engine, i: &Input) {
    let p = a.output(i);
    a.clock(i);
    b.tick(bridge::Input {
        reset: i.reset,
        valid: p.controller_request_valid,
        next_valid: p.controller_next_valid,
        next_address: p.controller_next_address as u32,
        writing: p.controller_write,
        address: p.controller_address as u32,
        words: p.controller_words as u8,
        mask: p.controller_write_mask as u8,
        data: p.controller_write_data,
        data_valid: p.controller_write_data_valid,
    })
    .unwrap();
}
#[test]
fn direct_cpu_port_calendars() {
    let mut b = bridge::Engine::new(OracleImage::filled::<0xa5>(0, 32768).unwrap(), 32).unwrap();
    let mut a = adapter::State::default();
    let mut i = pins(&b);
    i.reset = true;
    step(&mut a, &mut b, &i);
    for _ in 0..100 {
        let i = pins(&b);
        step(&mut a, &mut b, &i);
    }
    let expected = [
        (6, 9, 11),
        (6, 9, 10),
        (6, 9, 11),
        (7, 10, 12),
        (6, 9, 10),
        (12, 12, 13),
        (10, 10, 11),
        (11, 11, 12),
    ];
    for (case, (address, writing)) in [
        (0, false),
        (16, false),
        (64, false),
        (2048, false),
        (64, false),
        (0, true),
        (16, true),
        (128, true),
    ]
    .into_iter()
    .enumerate()
    {
        let mut accepted = false;
        let mut start = 0;
        let (mut first, mut last) = (None, None);
        let mut feeds = Vec::new();
        let mut returned = 0;
        for cycle in 0..80 {
            let mut i = pins(&b);
            i.cpu_request_valid = !accepted;
            i.cpu_write = writing;
            i.cpu_address = address;
            let o = a.output(&i);
            if !accepted && o.cpu_request_ready {
                accepted = true;
                start = cycle;
            }
            if o.cpu_write_data_ready {
                feeds.push(cycle - start);
            }
            if o.cpu_response_valid {
                assert!(!o.cpu_error);
                first.get_or_insert(cycle - start);
                last = Some(cycle - start);
                returned += 1;
                if !writing {
                    assert_eq!(o.cpu_read_data, 0xa5a5a5a5a5a5a5a5);
                }
            }
            if let (true, Some(first), Some(last)) = (accepted && o.cpu_request_ready, first, last)
            {
                assert_eq!(returned, if writing { 1 } else { 4 });
                assert_eq!((first, last, cycle - start), expected[case]);
                println!("PORT_TIMING address={address} writing={writing} first={first} last={last} ready={} feeds={feeds:?}",cycle-start);
                break;
            }
            step(&mut a, &mut b, &i);
            assert!(cycle < 79, "bounded direct service");
        }
    }
}
