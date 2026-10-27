//! Live arithmetic D/LOD plus metadata ports, excluding the Runtime controller.
use crate::texture::{
    emu::derivative::{rtl, Calendar},
    rtl::context_store,
};
use std::{
    fmt::Write,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub(super) fn source(d: &Calendar, l: &Calendar, cap: usize, native: bool, loan: bool) -> String {
    let dd = rtl::describe(d, "actual_d");
    let ld = rtl::describe(l, "actual_l");
    let uv_width = dd
        .inputs
        .iter()
        .find(|i| i.key == "helper_uv")
        .unwrap()
        .width;
    let mut s=format!("module derived_probe(input clk,reset,ce,admit,d_issue,l_issue,c_read,input[3:0] allocate,d_head,l_head,c_head,d_owner,l_owner,input[35:0]raw_data,input[{}:0]uv,output bad,advance,df,lf,output[2:0]reads,writes,phase,output[35:0]raw_q,output[55:0]derived_q,output[83:0]ready_q,output[127:0]duv);\n",uv_width*8-1);
    s.push_str(
        "wire dv,lv,di_ready,li_ready;wire[7:0]dp,lp;wire[3:0]raw_ra,derived_ra,ready_ra;\n",
    );
    for (desc, prefix) in [(&dd, "d"), (&ld, "l")] {
        for o in &desc.outputs {
            writeln!(s, "wire[{}:0]{prefix}_{};", o.width - 1, o.key).unwrap();
        }
    }
    s.push_str("assign phase=dp[0]?0:dp[1]?1:dp[2]?2:dp[3]?3:dp[4]?4:dp[5]?5:dp[6]?6:7;\n");
    s.push_str("wire[52:0]dword={d_filter,d_has_mip,d_max_n,d_slot,d_mask,d_quad,d_bias,d_slope};\nwire[82:0]lword={l_slot,l_mask,l_quad,l_last_fine,l_n1,l_n0,l_parent1,l_parent0,l_side1,l_side0,l_halve,l_nearest,l_shift0};\n");
    s.push_str("assign duv={d_uv7,d_uv6,d_uv5,d_uv4,d_uv3,d_uv2,d_uv1,d_uv0};\n");
    s.push_str("actual_d d(.clk(clk),.reset(reset),.ce(ce),.in_valid(d_issue),.in_ready(di_ready),.fault(df),.out_valid(dv),.dbg_phase(dp),");
    let mut maps = vec![];
    for i in &dd.inputs {
        let expr: String = match i.key.as_str() {
            "helper_uv" => format!("uv[{}+:{}]", i.row as u32 * uv_width, uv_width),
            "bias" => "raw_q[15:0]".into(),
            "force_coarsest" => "raw_q[16]".into(),
            "meta" => format!("raw_q[{}+:4]", 17 + i.row * 4),
            "has_mip" => "raw_q[33]".into(),
            "filter" => "raw_q[35:34]".into(),
            _ => panic!("D input"),
        };
        maps.push(format!(".{}({expr})", i.name));
    }
    maps.extend(
        dd.outputs
            .iter()
            .map(|o| format!(".{}(d_{})", o.name, o.key)),
    );
    s.push_str(&maps.join(","));
    s.push_str(");\n");
    s.push_str("actual_l l(.clk(clk),.reset(reset),.ce(ce),.in_valid(l_issue),.in_ready(li_ready),.fault(lf),.out_valid(lv),.dbg_phase(lp),");
    let mut maps = vec![];
    for i in &ld.inputs {
        let expr: String = match i.key.as_str() {
            "slope" => "derived_q[17:0]".into(),
            "bias" => "derived_q[33:18]".into(),
            "meta" => [
                "derived_q[49:46]",
                "derived_q[37:34]",
                "derived_q[41:38]",
                "derived_q[45:42]",
            ][i.row]
                .into(),
            "has_mip" => "derived_q[50]".into(),
            "filter" => "derived_q[52:51]".into(),
            _ => panic!("L input"),
        };
        maps.push(format!(".{}({expr})", i.name));
    }
    maps.extend(
        ld.outputs
            .iter()
            .map(|o| format!(".{}(l_{})", o.name, o.key)),
    );
    s.push_str(&maps.join(","));
    s.push_str(");\n");
    s.push_str("gpu_v2_context_store store(.clk(clk),.reset(reset),.ce(ce),.fault(df|lf),.control_bad((d_issue&&!di_ready)||(l_issue&&!li_ready)),.phase(phase),.admit(admit),.d_issue(d_issue),.l_issue(l_issue),.c_read(c_read),.d_return(dv),.l_return(lv),.allocate(allocate),.d_head(d_head),.l_head(l_head),.c_head(c_head),.d_owner(d_owner),.l_owner(l_owner),.raw_data(raw_data),.derived_data(dword),.ready_data(lword),.bad(bad),.advance(advance),.writes(writes),.reads(reads),.raw_ra(raw_ra),.derived_ra(derived_ra),.ready_ra(ready_ra),.raw_q(raw_q),.derived_q(derived_q),.ready_q(ready_q)");
    if loan {
        write!(
            s,
            ",.held_derived(dword),.loan_owner(d_head==0?4'd{}:d_head-4'd1)",
            cap - 1
        )
        .unwrap();
    }
    s.push_str(");\nendmodule\n");
    s.push_str(&rtl::emit_debug(d, "actual_d"));
    s.push_str(&rtl::emit_debug(l, "actual_l"));
    s.push_str(&if loan {
        context_store::runtime_source(cap, native).unwrap()
    } else {
        context_store::source_with_lod_ready(cap, native, false, true).unwrap()
    });
    s
}
fn bounded(mut c: Command, dir: &Path, name: &str) {
    c.stdout(Stdio::from(
        std::fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
    ))
    .stderr(Stdio::from(
        std::fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
    ));
    let mut p = c.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = p.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name}: {} {}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            p.kill().unwrap();
            p.wait().unwrap();
            panic!("Derived RTL watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(super) fn run(dir: &Path) {
    for name in ["portable", "native"] {
        let mut c = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        c.current_dir(dir).args([
            "-g2012",
            "-s",
            "tb",
            "-o",
            &format!("{name}.vvp"),
            &format!("{name}.v"),
            "tb.v",
        ]);
        if name == "native" {
            c.arg(
                std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap())
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        // D's actual native packed ALU always needs the vendor primitive model.
        if name == "portable" {
            c.arg(
                std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap())
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        bounded(c, dir, &format!("compile-{name}"));
        let mut c = Command::new(std::env::var_os("VVP_EXE").unwrap());
        c.current_dir(dir).arg(format!("{name}.vvp"));
        bounded(c, dir, &format!("run-{name}"));
    }
}
