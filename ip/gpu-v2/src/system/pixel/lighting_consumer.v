// The parent owns the source descriptor and its current odd row until ready.
// Retirement is the actual last global draw reference, not source entry ACK.
module gpu_v2_pixel_lighting_consumer(
 input wire clk,reset,ce,
 input wire source_valid, output wire source_ready,
 input wire [2:0] source_row, input wire [35:0] source_data,
 input wire [4:0] source_slot, input wire [3:0] source_mask,
 input wire source_bank,
 input wire retire_valid,retire_bank,
 input wire [15:0] context_epoch,
 input wire [1:0] context_mode, input wire [4:0] context_code,
 input wire signed [15:0] light_x,light_y,light_z,
 input wire [8:0] ambient,directional,
 input wire signed [15:0] ray_x,ray_y,ray_k,
 output wire context_wait,context_accepted,pixel_accepted,
 output wire out_valid,input wire out_ready,
 output wire [6:0] out_key,output wire out_quad_last,
 output wire [15:0] out_epoch,output wire [8:0] out_g,out_h
);
reg [35:0] normal;
reg loaded_valid,loaded_bank;
wire [1:0] lane=source_row[2:1];
wire covered=source_mask[lane];
wire switching=source_valid && covered && (!loaded_valid || loaded_bank!=source_bank);
wire core_context_ready,core_in_ready;
wire core_in_valid=source_valid && source_row[0] && covered && !switching;
wire last_lane=(source_mask >> ({1'b0,lane}+3'd1))==0;
wire [31:0] token={24'd0,last_lane,source_slot,lane};
wire [31:0] core_out_id;
assign context_wait=switching;
assign context_accepted=switching && core_context_ready;
assign source_ready=ce && !reset && source_valid &&
 (!source_row[0] || !covered || (!switching && core_in_ready));
assign pixel_accepted=core_in_valid && core_in_ready;
assign out_key=core_out_id[6:0];
assign out_quad_last=core_out_id[7];
// Sign is already bit 11. Appending zeros expands S12F10 to S16F14 exactly.
wire [15:0] nx={normal[11:0],4'd0};
wire [15:0] ny={normal[23:12],4'd0};
wire [15:0] nz={normal[35:24],4'd0};
wire [35:0] row0={4'd0,ny,nx};
wire [35:0] row1={4'd0,source_data[15:0],nz};
wire [35:0] row2={20'd0,source_data[31:16]};
always @(posedge clk) begin
 if(reset) begin normal<=0;loaded_valid<=0;loaded_bank<=0;end
 else if(ce) begin
  if(source_ready && covered && !source_row[0]) normal<=source_data;
  if(retire_valid && loaded_valid && retire_bank==loaded_bank) loaded_valid<=0;
  if(context_accepted) begin loaded_valid<=1;loaded_bank<=source_bank;end
 end
end
gpu_v2_lighting core(
 .clk(clk),.reset(reset),.ce(ce),
 .context_valid(switching),.context_ready(core_context_ready),
 .context_epoch(context_epoch),.context_mode(context_mode),.context_code(context_code),
 .light_x(light_x),.light_y(light_y),.light_z(light_z),
 .ambient(ambient),.directional(directional),.ray_x(ray_x),.ray_y(ray_y),.ray_k(ray_k),
 .in_valid(core_in_valid),.in_ready(core_in_ready),.in_id(token),
 .in_row0(row0),.in_row1(row1),.in_row2(row2),
 .out_valid(out_valid),.out_ready(out_ready),.out_id(core_out_id),
 .out_epoch(out_epoch),.out_g(out_g),.out_h(out_h)
);
endmodule
