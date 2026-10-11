// Only ingress/branch source/Basic storage, not branch results or global join.
module gpu_v2_quad_ingress(
 input wire clk, reset, ce,
 input wire in_valid,
 input wire [22:0] in_begin,
 input wire [31:0] in_basic,
 input wire [35:0] in_light, in_uv,
 output wire in_ready,
 output wire advance,
 output wire [2:0] in_row,
 output wire published,
 output wire [4:0] published_slot,
 output wire [22:0] published_begin,
 output wire basic_write,
 output wire [7:0] basic_write_address,
 output wire [23:0] basic_write_data,
 input wire basic_read,
 input wire [7:0] basic_read_address,
 output wire basic_return_valid,
 output wire [23:0] basic_return_data,
 output wire basic_collision,
 input wire light_ready, sample_ready,
 output wire light_valid, sample_valid,
 output wire [35:0] light_data, sample_data,
 output wire [2:0] light_row,
 output wire [1:0] sample_row,
 output wire light_last, sample_last,
 output wire [9:0] light_descriptor,
 output wire [10:0] sample_descriptor,
 input wire rop_row7_accepted,
 input wire [4:0] rop_slot,
 output wire [4:0] retire_slot,
 output wire [5:0] global_used,
 output wire [1:0] light_used,
 output wire [3:0] sample_used,
 output reg fault
);
 reg [2:0] row;
 reg [22:0] held_begin;
 reg [5:0] alloc_ptr, publish_ptr, retire_ptr;
 reg [1:0] light_alloc, light_consume;
 reg [3:0] sample_alloc, sample_consume;
 // Small branch-local descriptors have one asynchronous consumer read port.
 // No arbitrary global-status read port or per-row duplicated descriptor.
 (* syn_ramstyle="distributed_ram" *) reg [9:0] light_desc[0:1];
 (* syn_ramstyle="distributed_ram" *) reg [10:0] sample_desc[0:7];
 wire [22:0] current_begin = row == 0 ? in_begin : held_begin;
 wire live = |current_begin[3:0];
 wire lit = !current_begin[5];
 wire textured = !current_begin[6];
 assign global_used = alloc_ptr-retire_ptr;
 assign light_used = light_alloc-light_consume;
 assign sample_used = sample_alloc-sample_consume;
 wire proposed_ready = !reset && !fault && ce && (row != 0 || !live ||
  (global_used < 32 && (!lit || light_used < 2) && (!textured || sample_used < 8)));
 wire [4:0] slot = row == 0 ? alloc_ptr[4:0] : alloc_ptr[4:0]-5'd1;
 // Preflight uses the proposed raw-CE operation, never a gated commit enable.
 // A malformed concurrent external event cannot partially commit this edge.
 wire proposed_push = in_valid && proposed_ready;
 wire bad_retire = rop_row7_accepted &&
  (!ce || retire_ptr == publish_ptr || rop_slot != retire_ptr[4:0]);
 wire bad_collision = ce && basic_read && proposed_push && live &&
  basic_read_address == {slot,row};
 wire bad_depth = proposed_push && live && row[0] && |in_basic[31:16];
 wire bad_edge = bad_retire || bad_collision || bad_depth;
 assign advance = !reset && ce && !fault && !bad_edge;
 assign in_ready = proposed_ready && advance;
 wire push = in_valid && in_ready;
 assign in_row = row;
 assign published = push && live && row == 7;
 assign published_slot = slot;
 assign published_begin = current_begin;
 assign basic_write = push && live;
 assign basic_write_address = {slot,row};
 assign basic_write_data = in_basic[23:0];
 assign retire_slot = retire_ptr[4:0];
 assign light_descriptor = light_desc[light_consume[0]];
 assign sample_descriptor = sample_desc[sample_consume[2:0]];
 wire [3:0] light_row_w, sample_row_w;
 assign light_row = light_row_w[2:0];
 assign sample_row = sample_row_w[1:0];
 wire light_input_ready, sample_input_ready;
 gpu_v2_quad_source_rows #(.DEPTH(2),.ROWS(8),.WIDTH(36),.SYNC(1)) light_queue(
  .clk(clk),.reset(reset),.ce(advance),
  .in_valid(push && live && lit),.in_data(in_light),.in_ready(light_input_ready),
  .out_ready(light_ready),.out_valid(light_valid),.out_data(light_data),
  .out_row(light_row_w),.out_last(light_last),.published(),.read_request(),.read_address());
 gpu_v2_quad_source_rows #(.DEPTH(8),.ROWS(4),.WIDTH(36),.SYNC(1)) sample_queue(
  .clk(clk),.reset(reset),.ce(advance),
  .in_valid(push && live && textured && row[0]),.in_data(in_uv),.in_ready(sample_input_ready),
  .out_ready(sample_ready),.out_valid(sample_valid),.out_data(sample_data),
  .out_row(sample_row_w),.out_last(sample_last),.published(),.read_request(),.read_address());
 wire bank_collision, bank_return_valid;
 assign basic_collision = bad_collision || bank_collision;
 assign basic_return_valid = !reset && bank_return_valid;
 gpu_v2_pixel_row_store basic_bank(
  .clk(clk),.reset(reset),.read_en(advance && basic_read),.write_en(basic_write),
  .read_address(basic_read_address),.write_address(basic_write_address),
  .write_data(basic_write_data),.return_valid(bank_return_valid),
  .return_data(basic_return_data),.collision(bank_collision));
 always @(posedge clk) begin
  if(reset) begin
   row<=0;held_begin<=0;alloc_ptr<=0;publish_ptr<=0;retire_ptr<=0;
   light_alloc<=0;light_consume<=0;sample_alloc<=0;sample_consume<=0;fault<=0;
  end else begin
   if(bad_edge || bank_collision) fault<=1;
   if(advance && rop_row7_accepted) retire_ptr<=retire_ptr+1'b1;
   if(advance && light_valid && light_ready && light_last) light_consume<=light_consume+1'b1;
   if(advance && sample_valid && sample_ready && sample_last) sample_consume<=sample_consume+1'b1;
   if(push) begin
    if(row == 0) begin
     held_begin<=in_begin;
     if(live) begin
      alloc_ptr<=alloc_ptr+1'b1;
      if(lit) begin
       light_desc[light_alloc[0]]<={current_begin[4:0],slot};
       light_alloc<=light_alloc+1'b1;
      end
      if(textured) begin
       sample_desc[sample_alloc[2:0]]<={current_begin[7],current_begin[4:0],slot};
       sample_alloc<=sample_alloc+1'b1;
      end
     end
    end
    if(row == 7) begin row<=0; if(live) publish_ptr<=publish_ptr+1'b1; end
    else row<=row+1'b1;
   end
   if(push && live && lit && !light_input_ready) fault<=1;
   if(push && live && textured && row[0] && !sample_input_ready) fault<=1;
  end
 end
endmodule
