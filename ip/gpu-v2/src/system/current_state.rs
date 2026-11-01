//! Runtime target binding for the command composition.
//! Generic leaf exporters retain their independently checked interfaces.

fn once(s: &mut String, old: &str, new: &str) -> Result<(), String> {
    if s.matches(old).count() != 1 {
        return Err(format!("Current state anchor changed: {old}"));
    }
    *s = s.replacen(old, new, 1);
    Ok(())
}
fn edit(
    source: &mut String,
    name: &str,
    f: impl FnOnce(&mut String) -> Result<(), String>,
) -> Result<(), String> {
    let a = source
        .find(&format!("module {name}("))
        .or_else(|| source.find(&format!("module {name} #")))
        .ok_or_else(|| format!("Current state module {name}"))?;
    let b = a + source[a..].find("endmodule").ok_or("State module end")? + 9;
    let mut s = source[a..b].to_owned();
    f(&mut s)?;
    source.replace_range(a..b, &s);
    Ok(())
}
fn ports(s: &mut String, text: &str) -> Result<(), String> {
    let p = s
        .find("\n input")
        .or_else(|| s.find("\ninput"))
        .ok_or("State port boundary")?;
    s.insert_str(p, &format!("\n {text}"));
    Ok(())
}
fn link(s: &mut String, instance: &str, pins: &str) -> Result<(), String> {
    once(s, instance, &format!("{instance}\n {pins}"))
}
const TARGET_PORTS: &str = "input[13:0]color_base,depth_base,";
const TARGET_PINS: &str = ".color_base(color_base),.depth_base(depth_base),";

pub fn lower(mut source: String) -> Result<String, String> {
    edit(&mut source, "gpu_v2_vliw_three_port", |s| {
        ports(s, TARGET_PORTS)?;
        link(s, "gpu_v2_vliw_backend_three_port back(", TARGET_PINS)
    })?;
    edit(&mut source, "gpu_v2_vliw_backend_three_port", |s| {
        ports(s, TARGET_PORTS)?;
        link(s, "gpu_v2_pixel_framebuffer_sink sink(", TARGET_PINS)
    })?;
    edit(&mut source, "gpu_v2_pixel_framebuffer_sink", |s| {
        ports(s, TARGET_PORTS)?;
        let a = s
            .find(" parameter [31:0] COLOR_BASE=")
            .ok_or("Fixed target defaults")?;
        let b = a + s[a..].find('\n').ok_or("Target defaults end")?;
        s.replace_range(a..b, "");
        once(
            s,
            "gpu_v2_framebuffer_tdp #(.COLOR_BASE(COLOR_BASE),.DEPTH_BASE(DEPTH_BASE),",
            "gpu_v2_framebuffer_tdp #(",
        )?;
        link(s, "cache(\n", TARGET_PINS)
    })?;
    edit(&mut source, "gpu_v2_framebuffer_tdp", |s| {
        ports(s, TARGET_PORTS)?;
        once(s, " parameter[31:0]COLOR_BASE=0,DEPTH_BASE=16384,\n", "")?;
        once(s,
            "assign mem_req_addr=(mt_issue[1]?DEPTH_BASE:COLOR_BASE)+{14'd0,mt_linear_tile,mt_tile[5],mt_issue[0],7'b0};",
            "wire[6:0]target_page=(mt_issue[1]?depth_base[13:7]:color_base[13:7])+{5'd0,mt_linear_tile[8:7]};\n assign mem_req_addr={9'd0,target_page,mt_linear_tile[6:0],mt_tile[5],mt_issue[0],7'd0};")
    })?;
    for old in ["COLOR_BASE", "DEPTH_BASE"] {
        if source.contains(old) {
            return Err(format!("Obsolete current state survived: {old}"));
        }
    }
    Ok(source)
}
