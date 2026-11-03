use crate::{actual, vertex_support::rtl::Pin, Edge};
use std::fmt::Write as _;
pub fn fields(e: &Edge) -> Vec<Pin> {
    let mut p = Vec::new();
    let (mi, mo, bi, bo, m, b) = (e.mi, e.mo, e.bi, e.bo, &e.m, &e.b);
    macro_rules! pin {
        ($n:expr,$w:expr,$v:expr,$out:expr) => {
            p.push(Pin {
                name: $n.into(),
                width: $w,
                value: ($v) as u128,
                known: (1u128 << $w) - 1,
                output: $out,
            });
        };
    }
    for (n, v) in [
        ("reset", bi.reset),
        ("proposal_ce", bi.proposal_ce),
        ("commit_allow", bi.commit_allow),
        ("abort", bi.abort),
        ("issue_enable", bi.packet_enable),
        ("out_ready", bi.out_ready),
        ("texture_mip", bi.mip),
        ("texture_fb400240", bi.framebuffer),
        ("member_ce", mi.ce),
        ("member_valid", mi.valid),
        ("member_coarse", mi.coarse),
        ("member_last_fine", mi.last_fine),
    ] {
        pin!(n, 1, v, false);
    }
    pin!("texture_base", 16, bi.texture_base, false);
    pin!("member_flags", 36, mi.flags.value, false);
    pin!("member_meta", 36, mi.meta.value, false);
    for (n, v) in [
        ("in_ready", bo.ready),
        ("proposal_ready", bo.proposal_ready),
        ("proposal_ack", bo.proposal_ack),
        ("advance", bo.advance),
        ("fault", bo.fault),
        ("preflight_bad", false),
        ("out_valid", bo.valid),
        ("next_valid", bo.next_valid),
        ("capture_fire", bo.work.capture),
        ("work_ack", bo.work.ack),
        ("member_ready", mo.ready),
        ("member_return", mo.valid),
        ("obs_work_read", bo.work.read),
        ("obs_work_write", bo.work.write),
        ("obs_work_consume", b.work.consume),
        ("obs_work_fill", b.work.fill),
        ("obs_group_head", b.group.head_valid),
        ("obs_group_q", b.group.q_valid),
        ("obs_group_pending", b.group.pending),
    ] {
        pin!(n, 1, v, true);
    }
    for (n, w, v) in [
        ("work_count", 5, bo.work.materialized),
        ("producer_count", 5, bo.group.producer_count),
        ("unwritten_count", 5, bo.group.producer_count),
        ("group_count", 6, bo.group.group_count),
        ("row_count", 6, bo.group.row_count),
        ("pool_strobes", 6, bo.group.strobes),
        ("obs_work_read_row", 4, b.work.fetch),
        ("obs_work_write_row", 4, b.work.write),
        ("obs_work_reclaim", 4, b.work.reclaim),
        ("obs_work_loaded", 2, b.work.head_count),
        ("obs_work_valid", 2, b.work.head_valid),
        ("obs_group_wp", 6, b.group.write),
        ("obs_group_rp", 6, b.group.read),
        ("obs_packet_valid", 2, b.packet.valid),
        ("obs_member_valid", 7, m.valid),
    ] {
        pin!(n, w, v, true);
    }
    for (n, w, v) in [
        ("out_payload", 35, bo.payload),
        ("next_key", 15, bo.next_key),
        ("obs_work_tap", 2, bo.work.tap),
        ("obs_packet_capture", 41, b.packet.capture),
        ("obs_level_base", 15, b.packet.level_base),
        ("obs_packet_word", 35, b.packet.word),
        ("obs_group_raw", 35, b.group.raw_q),
        ("obs_group_do", 35, b.group.q),
        ("obs_head0", 4, b.work.heads[0]),
        ("obs_head1", 4, b.work.heads[1]),
    ] {
        p.push(Pin {
            name: n.into(),
            width: w,
            value: v.value.into(),
            known: v.known.into(),
            output: true,
        });
    }
    for (n, w, v) in [
        ("obs_member_common", 72, m.common),
        ("member_word", 76, mo.member),
        ("obs_work_member", 76, bo.work.member),
    ] {
        p.push(Pin {
            name: n.into(),
            width: w,
            value: v.value,
            known: v.known,
            output: true,
        });
    }
    p
}
pub fn production(p: &[Pin]) -> String {
    let mut s = String::new();
    let gowin = std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap());
    s.push_str(&std::fs::read_to_string(gowin.join("IDE/simlib/gw2a/prim_sim.v")).unwrap());
    for name in [
        "membership_leaf",
        "gpu_v2_texture_work_unique",
        "packet_leaf_unique",
        "gpu_v2_packet_store_unique",
        "gpu_v2_runtime_packet_backend_unique",
    ] {
        s.push_str(actual::module(name));
        s.push('\n');
    }
    s.push_str("module g2a(input wire clk");
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
    s.push_str(");\n");
    s.push_str("wire[3:0]level0=member_meta[25:22];wire[3:0]level=member_coarse&&level0!=0?level0-4'd1:level0;\nwire[9:0]mask=(10'b1111111111>>(4'd10-(level>0?level:4'd1)));\nwire[9:0]x=member_coarse&&level0>1?((member_meta[9:0]>>1)-member_meta[20])&mask:member_meta[9:0];wire[9:0]y=member_coarse&&level0>1?((member_meta[19:10]>>1)-member_meta[21])&mask:member_meta[19:10];\nmembership_leaf #(.SHORT_ALIGNMENT(1)) m(.clk(clk),.reset(reset),.ce(member_ce),.in_valid(member_valid),.in_w0(member_flags[8:0]),.in_w1(member_flags[17:9]),.in_w2(member_flags[26:18]),.in_w3(member_flags[35:27]),.in_c0(x),.in_c1((x+10'd1)&mask),.in_c2(y),.in_c3((y+10'd1)&mask),.in_slot(member_meta[29:26]),.in_level(level),.in_key(member_meta[35:30]),.in_fine(!member_coarse),.in_last_fine(member_last_fine),.in_ready(member_ready),.out_valid(member_return),.out_member(member_word),.fault());\n");
    s.push_str("gpu_v2_runtime_packet_backend_unique b(.clk(clk),.in_valid(member_return),.in_member(member_word)");
    for f in p
        .iter()
        .filter(|f| !f.name.starts_with("obs_") && !f.name.starts_with("member_"))
    {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\n");
    for (external, internal) in [
        ("work_read", "work_read"),
        ("work_write", "work_write"),
        ("work_consume", "u_work.consume_head"),
        ("work_fill", "u_work.fill_head"),
        ("group_head", "u_pool.head_valid"),
        ("group_q", "u_pool.q_valid"),
        ("group_pending", "u_pool.pending"),
        ("work_read_row", "u_work.fetch_ptr"),
        ("work_write_row", "u_work.write_ptr"),
        ("work_reclaim", "u_work.reclaim_ptr"),
        ("work_loaded", "u_work.head_count"),
        ("work_valid", "u_work.head_valid"),
        ("group_wp", "u_pool.wptr"),
        ("group_rp", "u_pool.rptr"),
        ("packet_valid", "u_packet.valid"),
        ("work_tap", "tap"),
        ("packet_capture", "u_packet.capture0"),
        ("level_base", "u_packet.level_base"),
        ("packet_word", "u_packet.word0"),
        ("group_do", "u_pool.data_do"),
        ("head0", "u_work.head0"),
        ("head1", "u_work.head1"),
        ("work_member", "member"),
    ] {
        writeln!(s, "assign obs_{external}=b.{internal};").unwrap();
    }
    // The primitive's internal read register is the paid pre-OCE value.
    s.push_str("assign obs_group_raw=b.u_pool.payload_hardware.bank.bp_reg;assign obs_member_common=m.common0;assign obs_member_valid=m.valid;\nGSR GSR(.GSRI(1'b1));\nendmodule\n");
    s
}
