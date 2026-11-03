use crate::{actual, Cache, Input, Output, Pin};
use std::fmt::Write as _;
pub fn fields(i: Input, o: Output, s: &Cache) -> Vec<Pin> {
    let mut p = Vec::new();
    let mut pin = |name: &str, width: u32, value: u128, known: u128, output: bool| {
        p.push(Pin {
            name: name.into(),
            width,
            value,
            known,
            output,
        })
    };
    for (n, v) in [
        ("reset", i.reset),
        ("proposal_ce", i.proposal_ce),
        ("commit_allow", i.commit_allow),
        ("abort", i.abort),
        ("in_valid", i.valid),
        ("texture_invalidate", i.texture_invalidate),
        ("weight_write", i.weight_write),
        ("next_valid", i.next_valid),
        ("out_ready", i.out_ready),
        ("mem_req_ready", i.mem_req_ready),
        ("mem_resp_valid", i.mem_resp_valid),
        ("mem_resp_complete", i.mem_resp_complete),
        ("mem_resp_ok", i.mem_resp_ok),
    ] {
        pin(n, 1, v as u128, 1, false);
    }
    for (n, w, v) in [
        ("in_payload", 35, i.payload.value),
        ("weight_address", 7, u128::from(i.weight_address)),
        ("weight_value", 36, u128::from(i.weight_value.value)),
        ("next_key", 15, u128::from(i.next_key)),
        ("mem_resp_index", 4, u128::from(i.mem_resp_index)),
        ("mem_resp_data", 64, u128::from(i.mem_resp_data)),
    ] {
        pin(n, w, v, (1u128 << w) - 1, false);
    }
    for (n, v) in [
        ("proposal_ready", o.proposal_ready),
        ("in_ready", o.in_ready),
        ("preflight_bad", false),
        ("advance", o.advance),
        ("out_valid", o.valid),
        ("mem_req_valid", o.mem_req_valid),
        ("drained", o.drained),
        ("fault", o.fault),
        ("obs_tag_read", o.tag_read),
        ("obs_tag_write", o.tag_write),
        ("obs_tag_valid", s.tag_valid),
        ("obs_read_issue", o.read_issue),
        ("obs_return_capture", o.return_capture),
        ("obs_result_pop", o.result_pop),
        ("obs_rd_valid", s.rd_valid),
        ("obs_res_valid", s.res_valid),
        ("obs_desc_valid", s.desc_valid),
        ("obs_desc_started", s.desc_started),
        ("obs_desc_presented", s.desc_presented),
        ("obs_desc_sector", s.desc_sector),
        ("obs_state_write", o.state_write),
        ("obs_plru_write", o.plru_write),
    ] {
        pin(n, 1, v as u128, 1, true);
    }
    pin("out_payload", 72, o.payload.value, o.payload.known, true);
    for (n, w, v) in [
        ("mem_req_addr", 32, u128::from(o.mem_req_addr)),
        ("obs_tag_set", 3, u128::from(o.tag_set)),
        ("obs_state_set", 3, u128::from(o.state_set)),
        ("obs_state_next", 8, u128::from(o.state_next)),
        ("obs_vic_line", 5, u128::from(o.victim_line)),
        ("obs_weight_ra", 7, u128::from(o.weight_ra)),
        ("obs_desc_next", 5, u128::from(s.desc_next)),
        ("obs_state_init", 8, u128::from(s.state_initialized)),
        ("obs_plru_init", 8, u128::from(s.plru_initialized)),
    ] {
        pin(n, w, v, (1u128 << w) - 1, true);
    }
    for (n, w, v) in [
        ("obs_hit", 1, o.hit_bits),
        ("obs_filling", 1, o.filling_bits),
        ("obs_hit_line", 5, o.hit_line_bits),
        ("obs_plru_next", 3, o.plru_next),
        ("obs_pin_line", 5, s.pin_line),
        ("obs_desc_line", 5, s.desc_line),
        ("obs_weight_q", 36, s.weight_q),
        ("obs_res_weights", 36, s.res_weights),
    ] {
        pin(n, w, u128::from(v.value), u128::from(v.known), true);
    }
    for (n, v) in [
        ("obs_res_payload", s.res_payload),
        ("obs_paid_payload", s.paid_payload),
    ] {
        pin(n, 35, v.value, v.known, true);
    }
    for b in 0..4 {
        let v = s.tag_q[b];
        pin(
            &format!("obs_tag{b}"),
            18,
            v.value.into(),
            v.known.into(),
            true,
        );
        let v = s.data_q[b];
        pin(
            &format!("out_tex{b}"),
            16,
            v.value.into(),
            v.known.into(),
            true,
        );
    }
    for r in 0..8 {
        let v = s.state_rows[r];
        pin(
            &format!("obs_state{r}"),
            8,
            v.value.into(),
            v.known.into(),
            true,
        );
        let v = s.plru_rows[r];
        pin(
            &format!("obs_plru{r}"),
            3,
            v.value.into(),
            v.known.into(),
            true,
        );
    }
    p
}
pub fn production(p: &[Pin], native: bool) -> String {
    let mut s = String::new();
    if native {
        let gowin = std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap());
        s.push_str(&std::fs::read_to_string(gowin.join("IDE/simlib/gw2a/prim_sim.v")).unwrap());
    }
    s.push_str(actual::module("gpu_v2_texture_runtime_cache_unique"));
    s.push_str("\nmodule g2a(input wire clk");
    for f in p {
        write!(
            s,
            ",{}wire[{}:0]{}",
            if f.output { "output " } else { "input " },
            f.width - 1,
            f.name
        )
        .unwrap();
    }
    write!(
        s,
        ");\ngpu_v2_texture_runtime_cache_unique #(.NATIVE({})) dut(.clk(clk)",
        u8::from(native)
    )
    .unwrap();
    for f in p.iter().filter(|f| !f.name.starts_with("obs_")) {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\n");
    for (ext, int) in [
        ("tag_read", "tag_read"),
        ("tag_write", "tag_write"),
        ("tag_valid", "tag_valid"),
        ("read_issue", "read_issue"),
        ("return_capture", "return_capture"),
        ("result_pop", "result_pop"),
        ("rd_valid", "rd_valid"),
        ("res_valid", "res_valid"),
        ("desc_valid", "desc_valid"),
        ("desc_started", "desc_started"),
        ("desc_presented", "desc_presented"),
        ("desc_sector", "desc_sector"),
        ("hit", "hit_ready"),
        ("filling", "look_filling"),
        ("state_write", "state_write"),
        ("plru_write", "plru_write"),
        ("tag_set", "tag_set"),
        ("state_set", "state_set"),
        ("state_next", "state_next"),
        ("plru_next", "plru_next"),
        ("hit_line", "hit_line"),
        ("vic_line", "vic_line"),
        ("weight_ra", "weight_ra"),
        ("desc_next", "desc_next"),
        ("state_init", "state_initialized"),
        ("plru_init", "plru_initialized"),
        ("pin_line", "pin_line"),
        ("desc_line", "desc_line"),
        ("weight_q", "weight_do"),
        ("res_weights", "res_weights"),
        ("res_payload", "res_payload"),
        ("paid_payload", "paid_payload"),
    ] {
        writeln!(s, "assign obs_{ext}=dut.{int};").unwrap();
    }
    for b in 0..4 {
        writeln!(s, "assign obs_tag{b}=dut.tag_q[{b}];").unwrap();
    }
    for r in 0..8 {
        writeln!(
            s,
            "assign obs_state{r}=dut.state_rows[{r}];assign obs_plru{r}=dut.plru_rows[{r}];"
        )
        .unwrap();
    }
    if native {
        s.push_str("GSR GSR(.GSRI(1'b1));\n");
    }
    s.push_str("endmodule\n");
    s
}
