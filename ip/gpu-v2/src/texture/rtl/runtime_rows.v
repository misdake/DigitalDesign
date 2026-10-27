// Actual Context8 / row UV / D+LOD terminal loan / C6 preparation frontend.
// Parent owns public-ID lifetime and holds Raw header through accepted row3.
module gpu_v2_texture_runtime_rows(
 input wire clk,reset,ce,commit_allow,admission_allow,
 input wire in_valid,input wire[35:0] in_raw,in_uv,
 input wire out_ready,
 output wire in_ready,advance,preflight_bad,fault,
 output wire[1:0] in_row,output wire[2:0] in_slot,
 output wire raw_write,admitted,reclaim_fire,
 output wire[2:0] reclaim_slot,
 output wire out_valid,output wire[37:0] out_owner,
 output wire[35:0] out_result,
 output wire[2:0] phase,
 output wire d_issue,l_issue,c_issue,c_empty,d_return,l_return,c_return,
 output wire[2:0] d_slot,l_slot,c_slot,d_return_slot,l_return_slot,
 output wire[1:0] c_lane,
 output wire[72:0] context_state,
 output wire[4:0] c_occupied,c_completed,
 output wire uv_write,uv_read,output wire[4:0] uv_write_address,uv_read_address,
 output wire[2:0] store_reads,store_writes
);
 // D_DECLARATIONS
 wire[35:0] raw_q;
 wire[55:0] derived_q;
 wire[83:0] ready_q;
 wire[17:0] l_shift0;
 wire l_nearest,l_halve,l_last_fine;
 wire[11:0] l_side0,l_side1;
 wire[8:0] l_parent0,l_parent1;
 wire[3:0] l_n0,l_n1,l_quad,l_mask,l_slot_meta;
 reg stopped;
 wire fe_fault,fe_bad,store_bad,queue_bad;
 wire running=ce&&!reset&&!fault;
 assign fault=stopped||fe_fault;
 wire joined_allow=commit_allow&&!preflight_bad&&!fault;
 assign advance=running&&joined_allow;
 always @(posedge clk) begin
  if(reset)stopped<=0;
  else if(running&&preflight_bad)stopped<=1;
 end
 wire uv_ready,reserve_ready;
 wire row_credit=(in_row!=0)||reserve_ready;
 assign in_ready=!reset&&!fault&&uv_ready&&row_credit&&((in_row!=0)||admission_allow);
 wire row_proposal=running&&in_valid&&in_ready;
 wire raw_proposal=row_proposal&&in_row==0;
 wire admit_proposal=row_proposal&&in_row==3;
 assign admitted=advance&&admit_proposal;
 wire[6:0] cmds;
 wire raw_issue,derived_issue,ready_read,uv_request,c_take;
 assign d_issue=raw_issue;assign l_issue=derived_issue;assign c_issue=c_take;
 wire uv_d_valid,uv_c_empty,uv_c_valid;
 wire[2:0] uv_d_slot,uv_c_slot;
 wire[1:0] uv_c_lane;
 wire[143:0] d_uv;
 wire[31:0] c_uv;
 wire d_ready,l_ready,c_ready,d_fault,l_fault,c_fault,c_credit;
 wire[7:0] d_phase;
 assign phase={|d_phase[7:4],d_phase[2]|d_phase[3]|d_phase[6]|d_phase[7],d_phase[1]|d_phase[3]|d_phase[5]|d_phase[7]};
 wire arithmetic_bad=running&&((cmds[0]&&!d_ready)||(cmds[1]&&!l_ready)||(cmds[4]&&!c_ready));
 assign preflight_bad=fe_bad||store_bad||queue_bad||arithmetic_bad;
 gpu_v2_texture_runtime_frontend_shared frontend(
 .clk(clk),.reset(reset),.proposal_ce(ce&&!stopped),.commit_allow(joined_allow),.reserve_check(raw_proposal),
 .admit(admit_proposal),.d_phase_zero(d_phase[0]),.l_phase_zero(phase==4),.d_return(d_return),.l_return(l_return),
 .uv_d_valid(uv_d_valid),.uv_d_slot(uv_d_slot),.c_mask(ready_q[78:75]),
 .uv_phase_odd(phase[0]),.uv_c_empty(uv_c_empty),.uv_c_valid(uv_c_valid),.uv_c_slot(uv_c_slot),.uv_c_lane(uv_c_lane),
 .c_phase_zero(!phase[0]),.c_credit(c_credit),.late_retire(8'b0),
 .preflight_bad(fe_bad),.advance(),.reserve_ready(reserve_ready),.proposal_cmds(cmds),.input_ready(),
 .raw_issue(raw_issue),.derived_issue(derived_issue),.ready_read(ready_read),.uv_request(uv_request),.c_take(c_take),.c_empty(c_empty),
 .admit_slot(in_slot),.d_slot(d_slot),.l_slot(l_slot),.c_slot(c_slot),.d_return_slot(d_return_slot),.l_return_slot(l_return_slot),
 .c_lane(c_lane),.c_ordinal(),.reclaim_fire(reclaim_fire),.reclaim_slot(reclaim_slot),.fault(fe_fault),.state(context_state));
 gpu_v2_uv_rows uv(
 .clk(clk),.reset(reset),.ce(advance),.phase(phase),.in_valid(in_valid&&row_credit&&((in_row!=0)||admission_allow)),
 .in_slot(in_slot),.in_metadata(in_raw),.in_uv(in_uv),.in_ready(uv_ready),.in_row(in_row),
 .raw_write(raw_write),.raw_slot(),.raw_metadata(),.committed(),.committed_slot(),
 .d_take(raw_issue),.d_valid(uv_d_valid),.d_slot(uv_d_slot),.d_uv(d_uv),
 .c_request(uv_request),.c_slot(c_slot),.c_lane(c_lane),.c_take(c_take),
 .c_empty(uv_c_empty),.c_valid(uv_c_valid),.c_owner_slot(uv_c_slot),.c_owner_lane(uv_c_lane),.c_uv(c_uv),
 .write_enable(uv_write),.read_enable(uv_read),.write_address(uv_write_address),.read_address(uv_read_address),.write_data());
 wire[52:0] held_derived={d_filter,d_has_mip,d_max_n,d_slot_meta,d_mask,d_quad,d_bias,d_slope};
 wire[2:0] loan_slot=d_slot-3'd1;
 wire[82:0] ready_data={l_slot_meta,l_mask,l_quad,l_last_fine,l_n1,l_n0,l_parent1,l_parent0,l_side1,l_side0,l_halve,l_nearest,l_shift0};
 gpu_v2_context_store_shared metadata(
 .clk(clk),.reset(reset),.proposal_ce(ce),.commit_allow(joined_allow),.fault(fault),.control_bad(1'b0),.phase(phase),
 .admit(raw_proposal),.d_issue(cmds[0]),.l_issue(cmds[1]),.c_read(cmds[2]),.d_return(d_return),.l_return(l_return),
 .allocate({1'b0,in_slot}),.d_head({1'b0,d_slot}),.l_head({1'b0,l_slot}),.c_head({1'b0,c_slot}),
 .d_owner({1'b0,d_return_slot}),.l_owner({1'b0,l_return_slot}),.raw_data(in_raw),
 .derived_data(held_derived),.held_derived(held_derived),.loan_owner({1'b0,loan_slot}),.ready_data(ready_data),
 .bad(),.preflight_bad(store_bad),.advance(),.proposal_reads(),.proposal_writes(),.writes(store_writes),.reads(store_reads),
 .raw_ra(),.derived_ra(),.ready_ra(),.raw_q(raw_q),.derived_q(derived_q),.ready_q(ready_q));
 // D_INSTANCE
 gpu_v2_texture_serial_lod lod(
 .clk(clk),.reset(reset),.ce(advance),.in_valid(derived_issue),.in_ready(l_ready),.fault(l_fault),.out_valid(l_return),
 .in_slope_0(derived_q[17:0]),.in_bias_0(derived_q[33:18]),.in_meta_0(derived_q[49:46]),
 .in_has_mip_0(derived_q[50]),.in_filter_0(derived_q[52:51]),.in_meta_1(derived_q[37:34]),.in_meta_2(derived_q[41:38]),.in_meta_3(derived_q[45:42]),
 .out_shift0(l_shift0),.out_nearest(l_nearest),.out_halve(l_halve),.out_side0(l_side0),.out_side1(l_side1),
 .out_parent0(l_parent0),.out_parent1(l_parent1),.out_n0(l_n0),.out_n1(l_n1),.out_last_fine(l_last_fine),.out_quad(l_quad),.out_mask(l_mask),.out_slot(l_slot_meta));
 wire[7:0] c_fx,c_fy;
 wire[9:0] c_x,c_y;
 gpu_v2_texture_serial_coord coord(
 .clk(clk),.reset(reset),.ce(advance),.in_valid(c_take),.in_ready(c_ready),.fault(c_fault),.out_valid(c_return),
 .in_flags_0(ready_q[18]),.in_flags_1(ready_q[19]),.in_coordinate_shift_0(ready_q[17:0]),
 .in_wrapped_uv_0(c_uv[15:0]),.in_wrapped_uv_1(c_uv[31:16]),.in_side_0(ready_q[31:20]),.in_side_1(ready_q[43:32]),
 .out_f0_0(c_fx),.out_f0_1(c_fy),.out_f1_0(),.out_f1_1(),
 .out_t0_0_0(c_x),.out_t0_0_1(),.out_t0_1_0(c_y),.out_t0_1_1(),
 .out_t1_0_0(),.out_t1_0_1(),.out_t1_1_0(),.out_t1_1_1());
 wire[37:0] c_owner={ready_q[70],ready_q[74:71],c_lane,ready_q[82:79],ready_q[69:66],ready_q[65:62],ready_q[18],ready_q[61:53],ready_q[52:44]};
 gpu_v2_runtime_coordinate_store queue(
 .clk(clk),.reset(reset),.proposal_ce(running),.commit_allow(joined_allow),
 .issue(cmds[4]),.returned(c_return),.take(out_ready&&out_valid),.owner_data(c_owner),.result_data({c_y,c_x,c_fy,c_fx}),
 .in_ready(c_credit),.out_valid(out_valid),.bad(queue_bad),.out_owner(out_owner),.out_result(out_result),
 .owner_write(),.result_write(),.consume(),.write_row(),.return_row(),.read_row(),.occupied(c_occupied),.completed(c_completed));
endmodule
