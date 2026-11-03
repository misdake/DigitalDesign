//! Complete command GPU observer; only unowned physical payload is projected.
pub fn fields(e: &crate::Edge) -> Vec<crate::vertex_support::rtl::Pin> {
    let mut p = crate::fixture::fields(e);
    let a = e.o.control.processor.auxiliary;
    let ui = e.o.ui.unwrap();
    for f in &mut p {
        if f.name == "pipeline_idle" && (a.clear_active || a.copy_active || ui.active) {
            f.value = 0;
        }
        if f.name == "obs_cp_re" {
            f.value = u128::from(if ui.active {
                ui.cp_read
            } else {
                e.o.control.processor.ram_read
            });
        }
        if f.name == "obs_cp_addr" {
            f.value = u128::from(if ui.active {
                ui.cp_address
            } else {
                e.o.control.processor.ram_address
            });
        }
        for (n, r) in ["ro", "fb_r", "fb_w"].into_iter().zip(e.o.physical) {
            if f.name == format!("{n}_request_valid") {
                f.value = u128::from(r.valid);
            }
            if f.name == format!("{n}_address") {
                f.value = if r.valid {
                    u128::from(r.address_halfword)
                } else {
                    0
                };
            }
            if f.name == format!("{n}_count") {
                f.value = u128::from(r.count);
            }
        }
        if f.name == "fb_w_data" {
            f.value = if e.i.physical[2].write_ready {
                u128::from(e.o.write_data)
            } else {
                0
            };
            f.known = if !e.i.physical[2].write_ready || a.clear_active {
                u64::MAX.into()
            } else {
                e.o.render.backend.sink.cache.mem_write_data.known.into()
            };
        }
    }
    for (n, v) in [
        ("ui_active", ui.active),
        ("copy_active", a.copy_active),
        ("clear_active", a.clear_active),
    ] {
        p.push(crate::vertex_support::rtl::Pin {
            name: format!("obs_aux_{n}"),
            width: 1,
            value: v.into(),
            known: 1,
            output: true,
        });
    }
    p
}
pub fn production(p: &[crate::vertex_support::rtl::Pin]) -> String {
    let mut s = crate::fixture::production(p);
    for n in ["ui_active", "copy_active", "clear_active"] {
        s = s.replace(&format!("dut.processor.aux_{n}"), &format!("dut.{n}"));
    }
    // Unoffered request addresses have no owner. Write payload is observed only
    // when the native port asks for it; no accepted or held offer is projected.
    let at = s.rfind("gpu_v2_vliw_command dut(").unwrap();
    let prefix = s[..at].to_string();
    let mut wrapper = s[at..].to_string();
    let mut wires = String::new();
    for n in ["ro", "fb_r", "fb_w"] {
        wrapper = wrapper.replace(
            &format!(".{n}_address({n}_address)"),
            &format!(".{n}_address(raw_{n}_address)"),
        );
        wires.push_str(&format!("wire[21:0]raw_{n}_address;assign {n}_address={n}_request_valid?raw_{n}_address:22'd0;\n"));
    }
    wrapper = wrapper.replace(".fb_w_data(fb_w_data)", ".fb_w_data(raw_fb_w_data)");
    wires.push_str(
        "wire[63:0]raw_fb_w_data;assign fb_w_data=fb_w_data_ready?raw_fb_w_data:64'd0;\n",
    );
    format!("{prefix}{wires}{wrapper}")
}
