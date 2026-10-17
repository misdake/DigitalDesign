// The producer supplies covered lanes in ascending order; mask/quad/slot are
// immutable for that quad. Each input pays one of four CE phases; no helper RAM.
module gpu_v2_texture_direct_lod(
 input clk,reset,ce,commit_allow,
 input in_valid,input[31:0]in_uv,input[3:0]in_quad,in_mask,in_slot,
 input[1:0]in_lane,input[7:0]in_lod,input[3:0]in_max_n,input in_has_mip,input[1:0]in_filter,
 output in_ready,advance,preflight_bad,fault,input out_ready,
 output out_valid,output[37:0]out_owner,output[35:0]out_result,
 output[1:0]phase,output issue,returned,output[4:0]occupied,completed
);
 reg[1:0]phase_q;reg stopped;
 assign phase=phase_q;assign fault=stopped;
 wire running=ce&&!reset&&!stopped;
 wire queue_credit,queue_bad,c_ready,c_fault,c_return;
 wire phase_zero=phase_q==0;
 assign in_ready=!reset&&!stopped&&phase_zero&&queue_credit;
 wire proposed=running&&in_valid&&in_ready;
 wire input_bad=proposed&&(in_max_n>10||in_filter>2||!in_mask[in_lane]);
 assign preflight_bad=input_bad||queue_bad;
 wire allow=commit_allow&&!preflight_bad&&!fault;
 assign advance=running&&allow;
 assign issue=advance&&in_valid&&in_ready;
 assign returned=advance&&c_return;
 wire[4:0]distance=in_lod[7:3];
 wire at_coarsest=distance>={1'b0,in_max_n};
 wire[3:0]fine=in_has_mip?(at_coarsest?4'd0:in_max_n-distance[3:0]):in_max_n;
 wire[2:0]frac=(in_has_mip&&!at_coarsest)?in_lod[2:0]:3'd0;
 wire[8:0]lambda=in_filter==2?{frac,6'b0}-(frac>4):9'd0;
 wire[3:0]physical=fine==0?4'd1:fine;
 wire signed[4:0]shift=$signed({1'b0,physical})-5'sd8;
 wire signed[17:0]shift_c={{13{shift[4]}},shift};
 wire signed[11:0]side0=12'd1<<physical;
 wire signed[11:0]side1=fine>1?side0>>1:12'd2;
 wire[7:0]fx,fy;wire[9:0]x,y;
 gpu_v2_texture_serial_coord coord(
 .clk(clk),.reset(reset),.ce(advance),.in_valid(in_valid&&in_ready),.in_ready(c_ready),.fault(c_fault),.out_valid(c_return),
 .in_flags_0(in_filter==0),.in_flags_1(fine>1),.in_coordinate_shift_0(shift_c),
 .in_wrapped_uv_0(in_uv[15:0]),.in_wrapped_uv_1(in_uv[31:16]),.in_side_0(side0),.in_side_1(side1),
 .out_f0_0(fx),.out_f0_1(fy),.out_f1_0(),.out_f1_1(),
 .out_t0_0_0(x),.out_t0_0_1(),.out_t0_1_0(y),.out_t0_1_1(),
 .out_t1_0_0(),.out_t1_0_1(),.out_t1_1_0(),.out_t1_1_1());
 wire[37:0]owner={lambda==0,in_quad,in_lane,in_slot,(fine==0?4'd0:fine-4'd1),fine,in_filter==0,lambda,~lambda};
 gpu_v2_direct_coordinate_store queue(
 .clk(clk),.reset(reset),.proposal_ce(running),.commit_allow(allow),
 .issue(proposed),.returned(c_return),.take(out_ready&&out_valid),.owner_data(owner),.result_data({y,x,fy,fx}),
 .in_ready(queue_credit),.out_valid(out_valid),.bad(queue_bad),.out_owner(out_owner),.out_result(out_result),
 .owner_write(),.result_write(),.consume(),.write_row(),.return_row(),.read_row(),.occupied(occupied),.completed(completed));
 always @(posedge clk)begin
  if(reset)begin phase_q<=0;stopped<=0;end
  else begin
   if(running&&preflight_bad)stopped<=1;
   if(advance)begin phase_q<=phase_q+1'b1;if(c_fault)stopped<=1;end
  end
 end
endmodule
