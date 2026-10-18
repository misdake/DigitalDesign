// Selected VLIW path: RAM Q is the normal owner until the complete pixel ACK.
// Context work starts on the old normal-row edge; the pixel issues one CE later.
module gpu_v2_pixel_lighting_consumer(
 input wire clk,reset,ce,compute_tick,
 input wire source_context_valid,source_valid,output wire source_ready,
 input wire[1:0]source_lane,input wire[35:0]source_normal,input wire[31:0]source_position,
 input wire[4:0]source_slot,input wire[3:0]source_mask,input wire source_bank,
 input wire retire_valid,retire_bank,
 input wire[15:0]context_epoch,input wire[1:0]context_mode,input wire[4:0]context_code,
 input wire signed[15:0]light_x,light_y,light_z,
 input wire[8:0]ambient,directional,input wire signed[15:0]ray_x,ray_y,ray_k,
 output wire context_wait,context_accepted,pixel_accepted,
 output wire out_valid,input wire out_ready,output wire[6:0]out_key,output wire out_quad_last,
 output wire[15:0]out_epoch,output wire[8:0]out_g,out_h
);
 reg loaded_valid,loaded_bank;
 wire covered=source_mask[source_lane];
 wire switching=source_context_valid&&covered&&(!loaded_valid||loaded_bank!=source_bank);
 wire core_context_ready,core_in_ready;
 wire core_in_valid=source_valid&&covered&&!switching;
 wire last_lane=(source_mask>>({1'b0,source_lane}+3'd1))==0;
 wire[31:0]token={24'd0,last_lane,source_slot,source_lane};
 wire[31:0]core_out_id;
 assign context_wait=switching;
 assign context_accepted=ce&&switching&&core_context_ready;
 assign source_ready=ce&&!reset&&source_valid&&(!covered||(!switching&&core_in_ready));
 assign pixel_accepted=ce&&core_in_valid&&core_in_ready;
 assign out_key=core_out_id[6:0];assign out_quad_last=core_out_id[7];
 // The arithmetic core has simultaneous operand ports, never a row collector.
 wire[15:0]nx={source_normal[11:0],4'd0};
 wire[15:0]ny={source_normal[23:12],4'd0};
 wire[15:0]nz={source_normal[35:24],4'd0};
 wire[35:0]row0={4'd0,ny,nx};
 wire[35:0]row1={4'd0,source_position[15:0],nz};
 wire[35:0]row2={20'd0,source_position[31:16]};
 always @(posedge clk)begin
  if(reset)begin loaded_valid<=0;loaded_bank<=0;end
  else if(ce)begin
   if(retire_valid&&loaded_valid&&retire_bank==loaded_bank)loaded_valid<=0;
   if(context_accepted)begin loaded_valid<=1;loaded_bank<=source_bank;end
  end
 end
 gpu_v2_lighting core(
 .clk(clk),.reset(reset),.ce(compute_tick),
 .context_valid(switching),.context_ready(core_context_ready),
 .context_epoch(context_epoch),.context_mode(context_mode),.context_code(context_code),
 .light_x(light_x),.light_y(light_y),.light_z(light_z),.ambient(ambient),.directional(directional),
 .ray_x(ray_x),.ray_y(ray_y),.ray_k(ray_k),
 .in_valid(core_in_valid),.in_ready(core_in_ready),.in_id(token),.in_row0(row0),.in_row1(row1),.in_row2(row2),
 .out_valid(out_valid),.out_ready(out_ready),.out_id(core_out_id),.out_epoch(out_epoch),.out_g(out_g),.out_h(out_h));
endmodule
