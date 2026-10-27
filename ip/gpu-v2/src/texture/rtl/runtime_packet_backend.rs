//! Actual compact Work16 -> short Packet -> ordered Pool64 backend.
//! Producer16 and Group32 remain separate credits; upstream Work reservations
//! belong to the Runtime coefficient controller, outside this local RTL unit.
//! `issue_enable` gates E0 only. Every accepted producer then advances on the
//! shared CE and writes at E2, even while Group32 is blocked.
//! Reachable Pool preflight: rows=P+G-heads <= P+G, G<=32, so P<16
//! implies rows<48. Pop uses an old valid head; write-without-reservation is
//! checked above shared CE. `pool_bad` is consequently unreachable for healthy
//! handshakes, and is not fed back into CE (which would form a combinational loop).
//! Short Packet ready is ce&&!fault: whenever this wrapper actually issues on
//! healthy shared CE, Packet is ready. No independent Packet ready credit exists.
use super::{compact_leaves, packet_store_ordered, work};

pub const MODULE: &str = "gpu_v2_texture_runtime_packet_backend";

pub fn source(native: bool) -> String {
    compose(
        packet_store_ordered::source(native),
        "gpu_v2_packet_store_ordered",
    )
}

/// Matched diagnostic baseline for qualification and backend-only resource fit.
/// The generic public Store/emitter and Legacy path retain their contracts.
#[cfg(test)]
pub(crate) fn reference_source(native: bool) -> String {
    compose(
        super::packet_store::source(1, native).unwrap(),
        "gpu_v2_packet_store",
    )
}

fn compose(pool: String, module: &str) -> String {
    let mut source = work::compact_verilog();
    source.push_str(compact_leaves::packet());
    source.push_str(&pool);
    source.push_str(&WRAPPER.replace("POOL_MODULE", module));
    source
}

const WRAPPER: &str = r#"
module gpu_v2_texture_runtime_packet_backend(
 input wire clk,reset,ce,abort,issue_enable,in_valid,
 input wire [75:0] in_member,
 output wire in_ready,
 input wire out_ready,
 output wire out_valid,output wire [71:0] out_payload,
 output wire fault,
 output wire [5:0] pool_strobes,
 output wire [4:0] producer_count,unwritten_count,
 output wire [5:0] group_count,row_count,
 output wire [4:0] work_count,
 output wire capture_fire,work_ack
);
 reg terminal;
 wire work_ready,work_valid,work_fault,packet_fault,pool_fault;
 wire [75:0] member;wire [1:0] tap;
 wire packet_valid,packet_ready,pool_ready,front_valid,pool_bad;
 wire [71:0] packet,front;
 wire work_read,work_write;
 wire [3:0] work_read_row,work_write_row;
 wire [1:0] work_loaded;
 wire [5:0] allocate_row,write_row,transfer_row,read_row,reclaim_row;
 wire [3:0] owner_address;wire [5:0] owner_value;
 wire [1:0] head_valid_out;wire pending_out;wire [71:0] returned_payload;
 reg owner_e1;
 wire fault_eff=terminal || abort || work_fault || packet_fault || pool_fault;
 // Preflight is independent of the eventual shared commit CE: no fault/CE loop.
 wire attempted=ce && !reset && !fault_eff;
 wire issue=work_valid && pool_ready && issue_enable;
 wire input_bad=in_valid && work_ready && (in_member[39:36]==0);
 wire capture_bad=issue && !member[36+tap];
 wire write_bad=packet_valid && (unwritten_count==0);
 wire preflight_bad=attempted && (input_bad || capture_bad || write_bad);
 wire advance=attempted && !preflight_bad;
 assign fault=fault_eff || preflight_bad;
 assign in_ready=advance && work_ready;
 assign out_valid=!reset && !fault && front_valid;
 assign out_payload=front;
 gpu_v2_texture_work #(.DEPTH(16)) u_work(
  .clk(clk),.reset(reset),.ce(advance),.in_valid(in_valid),.in_member(in_member),
  .in_ready(work_ready),.out_ready(pool_ready && issue_enable),.out_valid(work_valid),
  .out_member(member),.out_tap(tap),.read_fire(work_read),.write_fire(work_write),
  .capture_fire(capture_fire),.ack(work_ack),.read_row(work_read_row),.write_row(work_write_row),
  .materialized(work_count),.loaded(work_loaded),.fault(work_fault));
 packet_leaf #(.SHORT_ALIGNMENT(1)) u_packet(
  .clk(clk),.reset(reset),.ce(advance),.in_valid(issue),.in_member(member),.in_tap(tap),
  .in_ready(packet_ready),.out_valid(packet_valid),.out_packet(packet),.fault(packet_fault));
 POOL_MODULE u_pool(
  .clk(clk),.reset(reset),.ce(advance),.reserve_valid(issue),
  .reserve_owner({member[71:68],member[73:72]}),.write_valid(packet_valid),.write_payload(packet),
  .owner_request(owner_e1),.consume(front_valid && out_ready),
  .producer_ready(pool_ready),.front_valid(front_valid),.front_payload(front),.fault(pool_fault),.bad(pool_bad),
  .producer_count(producer_count),.unwritten_count(unwritten_count),.group_count(group_count),.row_count(row_count),
  .allocate_row(allocate_row),.write_row(write_row),.transfer_row(transfer_row),.read_row(read_row),.reclaim_row(reclaim_row),
  .owner_address(owner_address),.strobes(pool_strobes),.head_valid_out(head_valid_out),.pending_out(pending_out),
  .owner_value(owner_value),.returned_payload(returned_payload));
 always @(posedge clk)begin
  if(reset)begin terminal<=0;owner_e1<=0;end
  else begin
   if(abort || preflight_bad || work_fault || packet_fault || pool_fault)terminal<=1;
   if(advance)owner_e1<=issue;
  end
 end
endmodule
"#;
