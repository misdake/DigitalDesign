//! Logic-domain differential with a shared bounded production RTL producer.
use cpu_v3_tang_nano_20k::hardware::display_cycle::{Fill, FillInput};
#[path = "../support/display_fill_qualification.rs"]
mod fill_qualification;
#[path = "../support/display_test_output.rs"]
mod test_output;

#[test]
fn black_publication_partial_and_physical_address_limit() {
    let mut fill = Fill::default();
    let mut input = FillInput {
        boot_enable: true,
        device_index: 3,
        device_channel: 3,
        device_read_enable: true,
        ..Default::default()
    };
    for _ in 0..4 {
        fill.clock(&input);
    }
    let out = fill.outputs(&input);
    assert_eq!(out.published, 3);
    assert_eq!(out.group_valid, 0);
    assert!(!out.memory_request_valid && !out.line_write && !out.started_ack);
    for (base, status) in [(0x3e7000, 1), (0x3e7010, 5), (0x200001, 5)] {
        input.device_write_enable = true;
        input.device_channel = 1;
        input.device_write_data = base as u16;
        fill.clock(&input);
        input.device_write_enable = false;
        input.device_channel = 3;
        assert_ne!(fill.outputs(&input).device_read_data & 2, 0);
        input.device_write_enable = true;
        input.device_channel = 2;
        input.device_write_data = (base >> 16) as u16;
        fill.clock(&input);
        input.device_channel = 3;
        input.device_write_data = 1;
        fill.clock(&input);
        input.device_write_enable = false;
        assert_eq!(fill.outputs(&input).device_read_data, status);
    }
}

#[test]
#[ignore = "bounded production fill, faults and two real pixel clock phases"]
fn production_fill_protocol() {
    let out = test_output::fresh_output("display-cycle-fill-normal");
    fill_qualification::qualification(&out, "normal");
}
#[test]
#[ignore = "real CDC, swap-priority and memory-error DUT mutations"]
fn production_fill_mutants() {
    let out = test_output::fresh_output("display-cycle-fill-mutants");
    fill_qualification::qualification(&out, "mutants");
}
