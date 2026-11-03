use super::*;
use crate::vertex_support::rtl::{self, Pin};
use std::path::Path;
fn plain(p: &mut Vec<Pin>, name: &str, width: u32, value: u128, output: bool) {
    p.push(Pin {
        name: name.into(),
        width,
        value,
        known: (1u128 << width) - 1,
        output,
    });
}
fn bits(p: &mut Vec<Pin>, name: &str, width: u32, value: Bits) {
    p.push(Pin {
        name: name.into(),
        width,
        value: u128::from(value.value),
        known: u128::from(value.known),
        output: true,
    });
}
fn schema(e: &Edge) -> Vec<Pin> {
    let mut p = rtl::schema(&e.vertex);
    let i = e.vertex.input;
    let o = e.vertex.output.core;
    let r = &e.vertex.regs;
    plain(&mut p, "packet_ready", 1, u128::from(i.packet_ready), false);
    plain(&mut p, "word_ready", 1, u128::from(i.word_ready), false);
    for (name, width, v) in [
        ("done_t", 1, u128::from(o.done_t)),
        ("triangle_ready", 1, u128::from(o.triangle_ready)),
        ("triangle_mode", 1, u128::from(r.triangle_mode)),
        ("refs", 18, u128::from(r.refs)),
        ("building_desc", 32, u128::from(r.building_descriptor)),
        ("packet_valid", 1, u128::from(o.packet_valid)),
        ("word_valid", 1, u128::from(o.word_valid)),
        ("word_index", 5, u128::from(o.word_index)),
        ("coefficient_we", 1, u128::from(o.coefficient_write)),
        ("coefficient_wa", 9, u128::from(o.coefficient_address)),
        ("publish", 1, u128::from(o.publish)),
        ("packet_take", 1, u128::from(o.packet_take)),
        ("word_take", 1, u128::from(o.word_take)),
        ("release_slot", 1, u128::from(o.release_slot)),
        ("export_read", 1, u128::from(o.export_read)),
        ("export_addr", 9, u128::from(o.export_address)),
        ("position_re", 1, u128::from(o.position_read)),
        ("position_ra", 9, u128::from(o.position_read_address)),
        ("produced", 5, u128::from(e.produced)),
        ("consumed", 5, u128::from(e.consumed)),
        ("import_state", 2, u128::from(e.import_state)),
        ("result_owner", 1, u128::from(e.owner)),
        ("fetch_index", 5, u128::from(e.fetch)),
        ("response_index", 5, u128::from(e.response)),
        ("any_front", 1, u128::from(e.t.any_front)),
        ("sticky", 1, u128::from(e.t.sticky)),
        ("negative", 1, u128::from(e.t.negative)),
        ("align_negative", 1, u128::from(e.t.align_negative)),
        ("qmax", 5, u128::from(e.t.qmax)),
        ("cross_groups", 3, u128::from(e.t.cross_groups)),
        ("det_exp", 7, u128::from(e.t.det_exp)),
        ("index_group", 54, u128::from(e.group)),
        ("group_position", 2, u128::from(e.group_position)),
        ("index_cursor", 8, u128::from(e.cursor)),
        ("triangles", 7, u128::from(e.triangles)),
        ("sequence_id", 7, u128::from(e.sequence)),
    ] {
        plain(&mut p, name, width, v, true);
    }
    for (name, width, v) in [
        ("word_data", 54, o.word_data),
        ("descriptor", 32, o.descriptor),
        ("descriptor_head", 33, e.head),
        ("coefficient_data", 54, o.coefficient_data),
        ("pq", 36, e.pq),
        ("export_q", 36, e.low_q),
        ("export_hi", 18, e.high_q),
    ] {
        bits(&mut p, name, width, v);
    }
    p
}
fn production(p: &[Pin]) -> String {
    let mut wrapper = rtl::wrapper(p).replace(
        ".packet_ready(1'b0),.word_ready(1'b0)",
        ".packet_ready(packet_ready),.word_ready(word_ready)",
    );
    wrapper.truncate(wrapper.rfind("endmodule").unwrap());
    for (name, net) in [
        ("done_t", "dut.core.done_t"),
        ("triangle_ready", "dut.core.triangle_ready"),
        ("triangle_mode", "dut.core.triangle_mode"),
        ("refs", "dut.core.refs"),
        ("building_desc", "dut.core.building_desc"),
        ("packet_valid", "dut.packet_valid"),
        ("descriptor", "dut.descriptor"),
        ("word_valid", "dut.word_valid"),
        ("word_index", "dut.word_index"),
        ("word_data", "dut.word_data"),
        ("coefficient_we", "dut.core.step&&dut.core.t32_out"),
        ("coefficient_wa", "dut.core.coefficient_wa"),
        ("coefficient_data", "dut.core.t32_data"),
        ("publish", "dut.core.publish"),
        ("packet_take", "dut.core.packet_take"),
        ("word_take", "dut.core.word_take"),
        ("release_slot", "dut.core.release_slot"),
        ("export_read", "dut.core.export_read"),
        ("export_addr", "dut.core.export_addr"),
        ("position_re", "dut.core.core_read"),
        ("position_ra", "dut.core.t32_paddr"),
        ("produced", "dut.core.produced"),
        ("consumed", "dut.core.consumed"),
        ("import_state", "dut.core.import_state"),
        ("result_owner", "dut.core.result_owner"),
        ("fetch_index", "dut.core.fetch_index"),
        ("response_index", "dut.core.response_index"),
        ("any_front", "dut.core.t32.any_front"),
        ("sticky", "dut.core.t32.sticky"),
        ("negative", "dut.core.t32.negative"),
        ("align_negative", "dut.core.t32.align_negative"),
        ("qmax", "dut.core.t32.qmax"),
        ("cross_groups", "dut.core.t32.cross_groups"),
        ("det_exp", "dut.core.t32.det_exp"),
        ("index_group", "dut.index_group"),
        ("group_position", "dut.group_position"),
        ("index_cursor", "dut.index_cursor"),
        ("triangles", "dut.triangles"),
        ("sequence_id", "dut.sequence_id"),
        (
            "descriptor_head",
            "{dut.core.export_epoch,dut.core.descriptor}",
        ),
        ("pq", "dut.core.pq"),
        ("export_q", "dut.core.export_q"),
        ("export_hi", "dut.core.export_hi"),
    ] {
        wrapper.push_str(&format!("assign {name}={net};\n"));
    }
    wrapper.push_str("endmodule\n");
    format!("`define GPU_V2_COMMAND_PORTS\n`define GPU_V2_TRUSTED_FRONTEND\n`define GPU_V2_LIVE_GLOBALS\n{}\n{wrapper}",gpu_v2::frontend::vliw::verilog(true).replace("\r\n","\n"))
}
pub fn compare(root: &Path, label: &str, r: &Run) {
    let rows: Vec<_> = r.trace.iter().map(schema).collect();
    let source = production(&rows[0]);
    rtl::replay(root, label, rows, source, false, None);
}

pub fn root() -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-triangle")
        .join(format!(
            "qualification-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

pub const MUTATIONS: [&str; 13] = [
    "index_shift",
    "group_early",
    "norm_tail",
    "toward_zero",
    "high_address",
    "publish_early",
    "release_14",
    "full_borrow",
    "q_overwrite",
    "epoch_admit",
    "epoch_color",
    "refs_rotate",
    "stale_reset",
];
fn mutate(mut source: String, name: &str) -> String {
    let changes:Vec<(&str,&str)>=match name {
 "index_shift"=>vec![("index_group<={18'd0,index_group[53:18]};","index_group<={17'd0,index_group[53:17]};")],
 "group_early"=>vec![("if(group_position==2)state<=INDEX;","if(group_position==1)state<=INDEX;")],
 "norm_tail"=>vec![("cross_final_q={cross_groups,cross_tail}","cross_final_q={cross_groups,2'd0}")],
 "toward_zero"=>vec![("(magnitude?!sticky:sticky)","(magnitude?!sticky:1'b0)")],
 "high_address"=>vec![("export_read&&fetch_index<10,export_addr,export_hi","export_read&&fetch_index<10,export_addr^9'd16,export_hi")],
 "publish_early"=>vec![("assign publish=run&&flow==6;","assign publish=run&&flow==5&&!check_reject;")],
 "release_14"=>vec![("wire release_slot=word_take&&word_index==15;","wire release_slot=word_take&&word_index==14;"),("if(word_index==15)begin import_state<=0;consumed<=consumed+1'b1;fetch_index<=0;end","if(word_index==14)begin import_state<=0;consumed<=consumed+1'b1;fetch_index<=0;end")],
 "full_borrow"=>vec![("wire launch_t=ce&&start_t&&!busy&&!start_v&&!queue_full&&!reset;","wire launch_t=ce&&start_t&&!busy&&!start_v&&(!queue_full||release_slot)&&!reset;"),("assign triangle_ready=!busy&&!reset&&!queue_full;","assign triangle_ready=!busy&&!reset&&(!queue_full||release_slot);")],
 "q_overwrite"=>vec![("wire q_can_retire=result_owner==Q_EMPTY||\n   (result_owner==Q_DATA&&output_room);","wire q_can_retire=1'b1;")],
 "epoch_admit"=>vec![("assign meshlet_ready=!busy&&!reset&&(queue_empty||export_epoch==vertex_epoch);","assign meshlet_ready=!busy&&!reset;")],
 "epoch_color"=>vec![("export_color=epoch_color[export_epoch]","export_color=epoch_color[vertex_epoch]")],
 "refs_rotate"=>vec![("5:attribute_ref=descriptor[5:0];","5:attribute_ref=descriptor[11:6];")],
 "stale_reset"=>vec![("produced<=0;consumed<=0;","if(produced!=consumed)begin produced<=produced;consumed<=consumed;end else begin produced<=0;consumed<=0;end ")],
 _=>panic!("unknown actual DUT mutant {name}")
 };
    for (old, new) in changes {
        assert_eq!(source.matches(old).count(), 1, "{name}: {old}");
        source = source.replacen(old, new, 1);
    }
    source
}
pub fn audit_mutations(r: &Run) {
    let s = production(&schema(&r.trace[0]));
    for name in MUTATIONS {
        let _ = mutate(s.clone(), name);
    }
}
pub fn mutant(root: &Path, name: &str, r: &Run) {
    let rows: Vec<_> = r.trace.iter().map(schema).collect();
    let source = mutate(production(&rows[0]), name);
    rtl::replay(root, &format!("mutant-{name}"), rows, source, true, None);
}
pub fn contract(root: &Path, name: &str, r: &Run) {
    let rows: Vec<_> = r.trace.iter().map(schema).collect();
    let source = production(&rows[0]);
    let expected = match name {
        "too_many" | "zero_vertices" | "offset" => "GPU contract: invalid_header",
        "refs" => "GPU contract: refs_valid",
        "cursor" => "GPU contract: index cursor",
        "position" => "triangle position exceeds bounded F16 range",
        "cp_owner" => "CP read while V/T owns input Q",
        "active_page" => "DMA sink lost its statically independent write port",
        _ => panic!("unknown illegal peer {name}"),
    };
    rtl::replay(
        root,
        &format!("contract-{name}"),
        rows,
        source,
        false,
        Some(expected),
    );
}
