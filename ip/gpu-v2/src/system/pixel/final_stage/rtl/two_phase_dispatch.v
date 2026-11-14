module gpu_v2_postquad_final(input wire shade_phase,input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,
`ifdef GPU_V2_COMMAND_PORTS
 input wire[7:0]in_tex_alpha,output wire[7:0]out_tex_alpha,
`endif
 input wire shared_rop_offer,input wire[50:0]shared_spec_product,output wire shared_spec_ce,output wire[8:0]shared_spec_a,output wire[23:0]shared_spec_b,
 input wire[23:0]in_tint,in_tex,in_spec,input wire[8:0]in_g,in_h,
 output wire in_ready,input wire out_ready,output wire out_valid,
 output wire[23:0]out_rgb,output wire fault,preflight_bad);
 gpu_v2_final_ordered_two_cycle leaf(.shared_rop_offer(shared_rop_offer),.shared_spec_product(shared_spec_product),.shared_spec_ce(shared_spec_ce),.shared_spec_a(shared_spec_a),.shared_spec_b(shared_spec_b),.shade_phase(shade_phase),.clk(clk),.reset(reset),.ce(ce),.proposal_ce(proposal_ce),.commit_allow(commit_allow),.preflight_bad(preflight_bad),.abort(abort),
 .in_valid(in_valid),.in_ready(),.proposal_ready(in_ready),.in_tint(in_tint),.in_tex(in_tex),
`ifdef GPU_V2_COMMAND_PORTS
 .in_tex_alpha(in_tex_alpha),.out_tex_alpha(out_tex_alpha),
`endif
.in_spec(in_spec),.in_g(in_g),.in_h(in_h),
 .out_ready(out_ready),.out_valid(out_valid),.out_rgb(out_rgb),.fault(fault),.in_accept(),
 // Optional observability outputs are intentionally not consumed here.
 .advance(),.out_fire(),.credits(),.queued());endmodule
