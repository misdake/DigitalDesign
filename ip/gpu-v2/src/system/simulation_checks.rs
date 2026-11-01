//! Production GPU diagnostics observe the datapath without controlling it.
//!
//! The independent checked component factories keep their reusable APIs. This
//! specialization removes their diagnostic feedback from the command closure;
//! assertions execute on the original sampling edges, including wall-time RAM
//! returns. Both simulation and synthesis use the same specialized datapath.

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
            format!(
                "begin\n`ifndef SYNTHESIS\n if(!reset&&{}(({expression})!=0))$fatal(1,\"GPU contract: {name}\");\n`endif\n end ",
                if abort { "!abort&&" } else { "" }
            )
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

fn specialize(name: &str, s: &mut String) -> Result<(), String> {
    match name {
        "gpu_v2_vliw_backend_three_port" => register(s, "stopped", true)?,

        "gpu_v2_texture_runtime_color" => {
            register(s, "terminal", false)?;
            check(s, "preflight_fault", "1'b0", "1'b1", false)?;
        }
        "gpu_v2_texture_runtime_color_checked" => register(s, "fault", false)?,
        "gpu_v2_vliw_quad_pool" => register(s, "fault", false)?,
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
        let source="module gpu_v2_vliw_backend_three_port(input clk,reset,ce,abort,output reg stopped);always @(posedge clk)if(ce)stopped<=1;\nendmodule\nmodule unused(input clk);endmodule\n".to_owned();
        let result = lower(source).unwrap();
        assert!(
            result.contains("always @(posedge clk)if(reset)stopped<=0;else if(abort)stopped<=1;")
        );
        assert!(!result.contains("if(ce)stopped<=1"));
        assert!(result.contains("module unused(input clk);endmodule"));
    }
}
