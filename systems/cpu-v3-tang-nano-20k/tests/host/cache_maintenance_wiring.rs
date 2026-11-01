use std::{collections::BTreeMap, ops::Range};

const SYSTEM: &str = include_str!("../../examples/cpu_v3_system/self_test.v");

// Read only the production template's named-port connections. This deliberately
// does not elaborate Verilog or model any device's state machine.
fn tokens(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if source[at..].starts_with("//") {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if source[at..].starts_with("/*") {
            at += 2 + source[at + 2..].find("*/").expect("closed comment") + 2;
            continue;
        }
        let start = at;
        if bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'_' | b'$') {
            while at < bytes.len()
                && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'_' | b'$'))
            {
                at += 1;
            }
        } else if bytes[at] == b'"' {
            at += 1;
            while at < bytes.len() && bytes[at] != b'"' {
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            assert!(at < bytes.len(), "closed string");
            at += 1;
        } else {
            at += 1;
        }
        result.push(&source[start..at]);
    }
    result
}

fn named_ports<'a>(tokens: &[&'a str]) -> BTreeMap<(&'a str, &'a str), Range<usize>> {
    let mut ports = BTreeMap::new();
    for start in 0..tokens.len().saturating_sub(2) {
        let first = tokens[start].as_bytes()[0];
        if !(first.is_ascii_alphabetic() || matches!(first, b'_' | b'$'))
            || tokens[start + 1..start + 3] != ["(", "."]
        {
            continue;
        }
        let instance = tokens[start];
        let mut at = start + 2;
        loop {
            assert_eq!(tokens[at], ".");
            let port = tokens[at + 1];
            assert_eq!(tokens[at + 2], "(");
            let expression = at + 3;
            at = expression;
            let mut depth = 1;
            while depth != 0 {
                match tokens[at] {
                    "(" => depth += 1,
                    ")" => depth -= 1,
                    _ => {}
                }
                at += 1;
            }
            assert!(ports.insert((instance, port), expression..at - 1).is_none());
            match tokens[at] {
                "," => at += 1,
                ")" => break,
                other => panic!("unexpected connection delimiter {other}"),
            }
        }
    }
    ports
}

fn assert_cpu_local_hold(source: &str) {
    let t = tokens(source);
    let ports = named_ports(&t);
    let producer = &ports[&("u_sysctl", "cpu_hold")];
    assert_eq!(producer.len(), 1, "hold must drive one private net");
    let hold = t[producer.start];
    let consumer = &ports[&("u_core", "hold")];
    let expected = format!("!boot_done || {hold} || dcache_valid_sweep");
    assert_eq!(
        &t[consumer.clone()],
        tokens(&expected),
        "CPU hold must retain the SystemControl, boot and cache-sweep inputs"
    );

    // CPU hold cannot stop the cache's old work or the GPU/MC/Display clocks.
    for instance in [
        "u_core",
        "u_data_cache",
        "u_instruction_cache",
        "u_instruction_fetch_queue",
        "u_gpu",
        "gpu_ro_request_slot",
        "gpu_fb_r_request_slot",
        "gpu_fb_w_request_slot",
        "u_memory_arbiter",
        "u_shared_sdram_port",
        "u_display",
    ] {
        assert_eq!(&t[ports[&(instance, "clk")].clone()], ["clk"], "{instance}");
    }
    for clock in ["pixel_clock", "serial_clock"] {
        assert_eq!(&t[ports[&("u_display", clock)].clone()], [clock]);
    }
    for (at, token) in t.iter().enumerate().filter(|(_, token)| **token == hold) {
        let declaration = at > 0 && t[at - 1] == "wire" && t.get(at + 1) == Some(&";");
        assert!(
            declaration || producer.contains(&at) || consumer.contains(&at),
            "private hold {token} escapes SystemControl -> CPU at token {at}"
        );
    }
}

#[test]
fn cache_maintenance_hold_is_cpu_local_in_board_wiring() {
    assert_cpu_local_hold(SYSTEM);
}

#[test]
fn endpoint_checks_ignore_comments_spacing_and_private_net_spelling() {
    let source = SYSTEM
        .replace("sysctl_cpu_hold", "maintenance_pause")
        .replace(".clk(clk)", ".clk ( clk )");
    assert_cpu_local_hold(&format!(
        "// .hold(sysctl_cpu_hold)\n/* sysctl_cpu_hold sysctl_cpu_hold */\n{source}"
    ));
}

fn miswire(instance: &str, old: &str, new: &str) -> String {
    let marker = format!(" {instance} (");
    let (prefix, tail) = SYSTEM.split_once(&marker).unwrap();
    let (body, suffix) = tail.split_once("\n);").unwrap();
    assert!(body.contains(old));
    format!("{prefix}{marker}{}\n);{suffix}", body.replacen(old, new, 1))
}

#[test]
fn endpoint_checks_reject_miswired_cpu_gpu_mc_display_and_alias() {
    let cases = [
        (
            "CPU hold disconnected",
            miswire(
                "u_core",
                ".hold(!boot_done || sysctl_cpu_hold || dcache_valid_sweep)",
                ".hold(!boot_done || dcache_valid_sweep)",
            ),
        ),
        (
            "SystemControl drives another net",
            SYSTEM.replace(".cpu_hold(sysctl_cpu_hold)", ".cpu_hold(disconnected_hold)"),
        ),
        (
            "GPU clock paused",
            miswire("u_gpu", ".clk(clk)", ".clk(clk && !sysctl_cpu_hold)"),
        ),
        (
            "MC clock paused",
            miswire(
                "u_memory_arbiter",
                ".clk(clk)",
                ".clk(clk && !sysctl_cpu_hold)",
            ),
        ),
        (
            "Display reset by CPU hold",
            miswire(
                "u_display",
                ".reset(reset)",
                ".reset(reset || sysctl_cpu_hold)",
            ),
        ),
        (
            "GPU reset through an alias",
            miswire(
                "u_gpu",
                ".reset(!cold_ready)",
                ".reset(!cold_ready || leaked_hold)",
            )
            .replace(
                "wire sysctl_cpu_hold;",
                "wire sysctl_cpu_hold;\nwire leaked_hold = sysctl_cpu_hold;",
            ),
        ),
    ];
    for (name, source) in cases {
        let expected = match name {
            "CPU hold disconnected" | "SystemControl drives another net" => "CPU hold must retain",
            "GPU clock paused" => "u_gpu",
            "MC clock paused" => "u_memory_arbiter",
            "Display reset by CPU hold" | "GPU reset through an alias" => {
                "escapes SystemControl -> CPU"
            }
            _ => unreachable!(),
        };
        let failure = std::panic::catch_unwind(|| assert_cpu_local_hold(&source))
            .expect_err(&format!("missed bad wiring: {name}"));
        let reason = failure
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| failure.downcast_ref::<&str>().copied())
            .expect("wiring assertion message");
        assert!(
            reason.contains(expected),
            "{name}: wrong rejection: {reason}"
        );
        println!("REJECTED {name}: {expected}");
    }
}
