//! Production GPU diagnostics observe the datapath without controlling it.
//!
//! The independent checked component factories keep their reusable APIs. This
//! specialization removes their diagnostic feedback from the command closure;
//! assertions execute on the original sampling edges, including wall-time RAM
//! returns. Both simulation and synthesis use the same specialized datapath.

fn once(s: &mut String, old: &str, new: &str) -> Result<(), String> {
    if s.matches(old).count() != 1 {
        return Err(format!("Simulation monitor anchor changed: {old}"));
    }
    *s = s.replacen(old, new, 1);
    Ok(())
}

fn identifier(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

// Return the complete assignment statement and its RHS. This is deliberately
// limited to named scalar/vector diagnostic signals, not a general RTL parser.
fn assignments(s: &str, name: &str, sequential: bool) -> Vec<(usize, usize, String)> {
    let mut result = Vec::new();
    for (start, _) in s.match_indices(name) {
        let b = s.as_bytes();
        let mut p = start + name.len();
        if (start > 0 && identifier(b[start - 1])) || (p < b.len() && identifier(b[p])) {
            continue;
        }
        while p < b.len() && b[p].is_ascii_whitespace() {
            p += 1;
        }
        if p < b.len() && b[p] == b'[' {
            let Some(end) = s[p..].find(']') else {
                continue;
            };
            p += end + 1;
            while p < b.len() && b[p].is_ascii_whitespace() {
                p += 1;
            }
        }
        let op = if sequential { "<=" } else { "=" };
        if !s[p..].starts_with(op) || (!sequential && s[p..].starts_with("==")) {
            continue;
        }
        p += op.len();
        let Some(end) = s[p..].find(';') else {
            continue;
        };
        result.push((start, p + end + 1, s[p..p + end].trim().to_owned()));
    }
    result
}

fn monitor(s: &mut String, condition: &str, label: &str) {
    s.push_str(&format!("\n`ifndef SYNTHESIS\n always @(posedge clk)if(!reset&&({condition}))$fatal(1,\"GPU contract: {label}\");\n`endif\n"));
}

fn check(
    s: &mut String,
    name: &str,
    replacement: &str,
    gate: &str,
    invert: bool,
) -> Result<(), String> {
    let sites = assignments(s, name, false);
    if sites.len() != 1 {
        return Err(format!(
            "Expected one {name} detector, found {}",
            sites.len()
        ));
    }
    let (start, end, expression) = &sites[0];
    s.replace_range(*start..*end, &format!("{name}={replacement};"));
    let bad = if invert {
        format!("!({expression})")
    } else {
        expression.clone()
    };
    monitor(s, &format!("({gate})&&({bad})"), name);
    Ok(())
}

// Remove the diagnostic register and its feedback in both build modes. Keep
// external abort as an explicit operation, with its original sticky lifetime.
fn register(s: &mut String, name: &str, abort: bool) -> Result<(), String> {
    let sites = assignments(s, name, true);
    if sites.is_empty() {
        return Err(format!("Missing diagnostic register {name}"));
    }
    for (start, end, expression) in sites.into_iter().rev() {
        let text = if expression == "0" {
            "begin end ".to_owned()
        } else {
            format!("begin\n`ifndef SYNTHESIS\n if(!reset&&{}(({expression})!=0))$fatal(1,\"GPU contract: {name}\");\n`endif\n end ", if abort { "!abort&&" } else { "" })
        };
        s.replace_range(start..end, &text);
    }
    if abort {
        s.push_str(&format!(
            "\n always @(posedge clk)if(reset){name}<=0;else if(abort){name}<=1;\n"
        ));
    } else {
        s.push_str(&format!("\n always @(posedge clk){name}<=0;\n"));
    }
    Ok(())
}

// Inputs remain in the interface for compatibility, but error qualifiers never
// gate the production write/advance path. The sampling condition is explicit.
fn peer_error(s: &mut String, name: &str, good: bool, event: &str) -> Result<(), String> {
    let header = s.find(");").ok_or("Module port boundary")? + 2;
    let body = s[header..].to_owned();
    let value = if good { "1'b1" } else { "1'b0" };
    let mut rewritten = body.clone();
    for (p, _) in body
        .match_indices(name)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let end = p + name.len();
        if (p == 0 || !identifier(body.as_bytes()[p - 1]))
            && (end == body.len() || !identifier(body.as_bytes()[end]))
        {
            rewritten.replace_range(p..end, value);
        }
    }
    s.replace_range(header.., &rewritten);
    monitor(
        s,
        &format!("({event})&&{}({name})", if good { "!" } else { "" }),
        name,
    );
    Ok(())
}

fn specialize(name: &str, s: &mut String) -> Result<(), String> {
    match name {
        "shared_meshlet" => {
            check(s, "invalid_header", "1'b0", "ce&&state==CHECK", false)?;
            once(
                s,
                "if(index_cursor[7])begin error<=6;",
                "if(1'b0)begin error<=6;",
            )?;
            monitor(s, "ce&&state==INDEX&&index_cursor[7]", "index cursor");
            register(s, "error", false)?;
        }
        "shared_frontend" => {
            check(s, "compact_invalid", "1'b0", "launch_v", false)?;
            check(s, "refs_valid", "1'b1", "launch_t", true)?;
            check(
                s,
                "p_range_error",
                "1'b0",
                "step&&triangle_mode&&rf_wsrc==1",
                false,
            )?;
            once(s, "vertex_count+published_vertices>64", "1'b0")?;
            monitor(
                s,
                "launch_v&&(vertex_count+published_vertices>64)",
                "vertex capacity",
            );
            register(s, "error", false)?;
        }
        "rc_raster" | "rc_lod" | "rp_prepare" => register(s, "error", false)?,
        "rc_compute" => register(s, "compute_error", false)?,
        "gpu_v2_vliw_backend_three_port" => register(s, "stopped", true)?,
        "gpu_v2_direct_coordinate_store" => check(s, "bad", "1'b0", "1'b1", false)?,
        "gpu_v2_final_ordered_two_cycle" => {
            // The stop latch retains only explicit abort, not diagnostic feedback.
            register(s, "stopped", true)?;
            check(s, "preflight_bad", "1'b0", "!abort", false)?;
        }
        "gpu_v2_framebuffer_tdp" => {
            peer_error(s, "mem_complete_ok", true, "mt_active&&mem_complete_valid")?;
            register(s, "fault_c", false)?;
            register(s, "fault_m", false)?;
            check(s, "source_bad", "1'b0", "1'b1", false)?;
        }
        "gpu_v2_packet_store_unique" => {
            register(s, "fault", false)?;
            check(s, "bad", "1'b0", "1'b1", false)?;
        }
        "gpu_v2_runtime_coefficient_directweight" => {
            register(s, "fault", false)?;
            check(s, "preflight_bad", "1'b0", "1'b1", false)?;
        }
        "gpu_v2_runtime_packet_backend_unique" => {
            register(s, "terminal", true)?;
            check(s, "preflight_bad", "1'b0", "!abort", false)?;
        }
        "gpu_v2_texture_direct_lod" => {
            register(s, "stopped", false)?;
            check(s, "preflight_bad", "1'b0", "running", false)?;
        }
        "gpu_v2_texture_runtime_backend_unique" => {
            register(s, "stopped", true)?;
            check(s, "preflight_bad", "1'b0", "running&&!abort", false)?;
        }
        "gpu_v2_texture_runtime_cache_unique" => {
            monitor(s,"!abort&&((mem_resp_valid&&(!eff_started||desc_next>=16||mem_resp_index!=desc_next[3:0]))||(mem_resp_complete&&(!eff_started||eff_next!=16)))","texture return framing");
            once(s,"wire beat_ok=beat_present&&eff_started&&desc_next<16&&mem_resp_index==desc_next[3:0];","wire beat_ok=beat_present;")?;
            once(
                s,
                "wire term_ok=terminal&&mem_resp_ok&&eff_started&&eff_next==16;",
                "wire term_ok=terminal;",
            )?;
            peer_error(s, "mem_resp_ok", true, "mem_resp_complete")?;
            register(s, "fault", true)?;
            check(s, "preflight_bad", "1'b0", "!abort", false)?;
        }
        "gpu_v2_texture_runtime_color" => {
            register(s, "terminal", false)?;
            check(s, "preflight_fault", "1'b0", "1'b1", false)?;
        }
        "gpu_v2_texture_runtime_color_checked" => register(s, "fault", false)?,
        "gpu_v2_texture_serial_coord" => check(s, "fault", "1'b0", "1'b1", false)?,
        "gpu_v2_texture_work_unique" => {
            register(s, "fault", false)?;
            check(s, "bad", "1'b0", "ce", false)?;
        }
        "gpu_v2_vliw_quad_pool" => register(s, "fault", false)?,
        "membership_leaf" => {
            once(s,"if(in_valid&&(in_level>10 || !(|{in_w0,in_w1,in_w2,in_w3})))fault<=1;\n   else begin","begin")?;
            monitor(
                s,
                "ce&&in_valid&&(in_level>10||!(|{in_w0,in_w1,in_w2,in_w3}))",
                "texture membership",
            );
            register(s, "fault", false)?;
        }
        "packet_leaf_unique" => {
            once(
                s,
                "if(in_valid&&!active[in_tap])fault<=1;\n   else begin",
                "begin",
            )?;
            monitor(s, "ce&&in_valid&&!active[in_tap]", "physical packet tap");
            register(s, "fault", false)?;
        }
        "gpu_command_dma" => {
            peer_error(
                s,
                "response_error",
                false,
                "state==RESPONSE&&response_valid",
            )?;
            peer_error(s, "response_bad", false, "state==RESPONSE&&response_valid")?;
            register(s, "fault", false)?;
        }
        "gpu_vliw_memory_router" => {
            // Keep ownership/last and request progress, not validation feedback.
            peer_error(s, "ro_response_error", false, "ro_response_valid")?;
            peer_error(s, "fb_r_response_error", false, "fb_r_response_valid")?;
            peer_error(s, "fb_w_response_error", false, "fb_w_response_valid")?;
            register(s, "fault", false)?;
            register(s, "ro_bad", false)?;
            register(s, "fb_bad", false)?;
            check(s, "ro_malformed", "1'b0", "1'b1", false)?;
            check(s, "f_malformed", "1'b0", "1'b1", false)?;
            once(s, "&&rb<16&&!reset", "&&!reset")?;
            // Request legality is already an explicit local predicate. Capture
            // its sampled value only in the simulation monitor.
            let sites = assignments(s, "ro_legal", true);
            for (a, b, expression) in sites.into_iter().rev() {
                let extra = if expression == "0" {
                    String::new()
                } else {
                    format!("\n`ifndef SYNTHESIS\n if(!({expression}))$fatal(1,\"GPU contract: RO request\");\n`endif\n")
                };
                s.replace_range(a..b, &format!("begin ro_legal<=1;{extra}end "));
            }
            check(s, "fb_legal", "1'b1", "framebuffer_request_valid", true)?;
        }
        _ => (),
    }
    Ok(())
}

pub fn lower(source: String) -> Result<String, String> {
    let source = source.replace("\r\n", "\n");
    let mut result = String::new();
    let mut rest = source.as_str();
    while let Some(start) = if rest.starts_with("module ") {
        Some(0)
    } else {
        rest.find("\nmodule ").map(|n| n + 1)
    } {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest.find("endmodule").ok_or("Unterminated GPU module")?;
        let name = rest[7..]
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next()
            .ok_or("GPU module name")?;
        let mut body = rest[..end].to_owned();
        specialize(name, &mut body)?;
        result.push_str(&body);
        result.push_str("endmodule");
        rest = &rest[end + 9..];
    }
    result.push_str(rest);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_never_feed_back_even_in_simulation() {
        let mut s="module m(input clk,reset,ce,input bad,output reg[3:0]error);\n always @(posedge clk)begin if(reset)error<=0;else if(ce&&bad)error[3]<=1;end\n".to_owned();
        register(&mut s, "error", false).unwrap();
        assert!(s.contains("always @(posedge clk)error<=0;"));
        assert!(!s.contains("error[3]<=1"));
        assert!(s.contains("`ifndef SYNTHESIS"));
        assert!(s.contains("if(ce&&bad)begin"));
    }
    #[test]
    fn first_module_is_not_skipped_without_a_preamble() {
        let source="module rc_lod(input clk,reset,ce,output reg[3:0]error);always @(posedge clk)if(ce)error[0]<=1;\nendmodule\nmodule unused(input clk);endmodule\n".to_owned();
        let result = lower(source).unwrap();
        assert!(result.contains("always @(posedge clk)error<=0;"));
        assert!(!result.contains("error[0]<=1"));
        assert!(result.contains("module unused(input clk);endmodule"));
    }
}
