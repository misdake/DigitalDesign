// One-cycle compute units with one ordinary return calendar.
module CpuV3Fpu (
 input wire clk, abort, word_valid, input wire [15:0] word,
 output wire busy, flag_lt, flag_eq, flag_gt, instr_complete,
 input wire ext_access, ext_write_enable, input wire [8:0] ext_write_address,
 input wire [31:0] ext_write_data, input wire [8:0] ext_read_address,
 output wire [31:0] ext_read_data, input wire integer_in_valid,
 input wire [15:0] integer_in_a, integer_in_b, output wire integer_out_valid,
 output wire [31:0] integer_out_product
);
wire read_valid, fe_instr_complete;
wire [8:0] fe_read_a_address, fe_read_b_address;
wire [15:0] word0_raw, word1_raw;
wire [3:0] instr_opcode;
CpuV3FpuFrontend frontend (
 .clk(clk), .abort(abort), .word_valid(word_valid), .word(word),
 .read_valid(read_valid), .rf_read_a_address(fe_read_a_address),
 .rf_read_b_address(fe_read_b_address), .word0_raw(word0_raw),
 .word1_raw(word1_raw), .instr_opcode(instr_opcode), .instr_complete(fe_instr_complete)
);
wire [5:0] base_a = instr_opcode == 4'hE ? word0_raw[7:2] : word0_raw[11:6];
wire [5:0] base_b = word0_raw[5:0], fd = word1_raw[15:10];
wire [5:0] scalar_subop = word1_raw[9:4];
wire [4:0] vector_subop = word1_raw[7:3];
wire scalar_op = instr_opcode == 4'hD, vector_op = instr_opcode == 4'hC;
wire scalar_alu_owned = scalar_subop != 2 && scalar_subop != 12 && scalar_subop != 13 && scalar_subop != 14;
wire vector_alu_owned = vector_subop == 0 || vector_subop == 1 ||
 (vector_subop >= 4 && vector_subop <= 7) || vector_subop == 12;
wire multiply_op = (scalar_op && scalar_subop == 2) || (vector_op && (vector_subop == 2 || vector_subop == 3));
wire dot_r = vector_op && (vector_subop == 13 || vector_subop == 14 || vector_subop == 15);
wire product_r = multiply_op || dot_r;
wire scalar_load = fe_instr_complete && scalar_op && (scalar_alu_owned || multiply_op) && !abort;
wire vector_load = fe_instr_complete && vector_op && (vector_alu_owned || product_r) && !abort;
wire normal_load = scalar_load || vector_load;
wire [2:0] stride = dot_r ? (word1_raw[1:0] == 1 ? 3'd3 : word1_raw[1:0] == 2 ? 3'd4 : 3'd1) :
 vector_op && vector_subop == 3 ? 3'd0 : 3'd1;
wire broadcast_b = vector_op && vector_subop == 3;
wire cmp_op = scalar_op && scalar_subop == 11;
// Install descriptors on word1 acceptance, before the execution/load edge.
// read_valid identifies word0; the other accepted word is necessarily word1.
wire prepare = word_valid && !read_valid && !abort;
wire [1:0] prepared_last = word[9:8] == 3 ? 2'd3 : word[9:8] + 2'd1;
wire prepared_dot = vector_op && (word[7:3] == 13 || word[7:3] == 14 || word[7:3] == 15);
wire [2:0] prepared_stride = prepared_dot ? (word[1:0] == 1 ? 3'd3 : word[1:0] == 2 ? 3'd4 : 3'd1) :
 vector_op && word[7:3] == 3 ? 3'd0 : 3'd1;


// The ISA permits exact-base aliases or disjoint destination ranges.
// All ordinary operations consume ready lane zero at load, prefetching lane one.
reg run_r = 0;
reg [1:0] lane_r = 0, last_lane_r = 0;
reg [5:0] read_a_r = 0, read_b_r = 0;
reg [6:0] destination_r = 0;
wire issue = (normal_load || run_r) && !abort;
wire issue_last = lane_r == last_lane_r;
wire [6:0] issue_destination = destination_r;
reg return_valid = 0, return_last = 0;
reg [6:0] return_destination = 0;

reg signed [63:0] acc_r = 0;

reg flag_lt_r = 0, flag_eq_r = 0, flag_gt_r = 0;
wire own_return = return_valid && product_r && !abort;
wire [31:0] rf_read_a_data, rf_read_b_data;
wire sf_busy, sf_write_enable;
wire [8:0] sf_read_a_address, sf_read_b_address, sf_write_address;
wire [31:0] sf_write_data;
wire sf_mul_in_valid;
wire signed [35:0] sf_mul_in_a, sf_mul_in_b;
wire [8:0] sf_mul_in_tag;
wire [8:0] rf_read_a_address = sf_busy ? sf_read_a_address :
 (normal_load || run_r) ? {3'd0,read_a_r} : {3'd0,base_a};
wire [8:0] rf_read_b_address = sf_busy ? sf_read_b_address :
 (normal_load || run_r) ? {3'd0,read_b_r} :
 ext_access ? ext_read_address : {3'd0,base_b};
wire rf_read_b_enable = !((vector_load || run_r) && broadcast_b);

function [3:0] vector_alu_op;
 input [4:0] op;
 begin case(op)
  0:vector_alu_op=0; 1:vector_alu_op=1;
  4:vector_alu_op=3; 5:vector_alu_op=4;
  6:vector_alu_op=5; 7:vector_alu_op=6;
  12:vector_alu_op=15; default:vector_alu_op=2;
 endcase end
endfunction
wire [3:0] alu_request_op = vector_op ? vector_alu_op(vector_subop) : scalar_subop[3:0];
wire [31:0] shared_alu_result;
wire shared_alu_lt, shared_alu_eq, shared_alu_gt;
CpuV3FpuScalarAlu shared_alu (
 .a(rf_read_a_data), .b(rf_read_b_data), .op(alu_request_op),
 .result(shared_alu_result), .flag_lt(shared_alu_lt), .flag_eq(shared_alu_eq), .flag_gt(shared_alu_gt)
);

wire mul_in_valid = integer_in_valid || sf_mul_in_valid || (issue && product_r);
wire sf_range_operand = sf_mul_in_valid && sf_mul_in_tag[0];
wire sf_interp_operand = sf_mul_in_valid && !sf_range_operand;
wire [31:0] mul_a_word = integer_in_valid ? {16'd0,integer_in_a} :
 sf_interp_operand ? {{22{sf_mul_in_a[9]}},sf_mul_in_a[9:0]} : rf_read_a_data;
wire [31:0] mul_b_word = integer_in_valid ? {16'd0,integer_in_b} :
 sf_range_operand ? 32'hA2F9836E : sf_interp_operand ? {23'd0,sf_mul_in_b[8:0]} : rf_read_b_data;
wire mul_b_negative = !integer_in_valid && !sf_mul_in_valid && rf_read_b_data[31];
wire signed [35:0] mul_in_a = {{4{mul_a_word[31]}},mul_a_word};
wire signed [35:0] mul_in_b = {{4{mul_b_negative}},mul_b_word};
wire pipe_out_valid;
wire signed [63:0] mul_out_product;
// A blocking instruction owns all of its returns. Only the external integer
// pulse needs to be delayed; ordinary destination metadata shares ALU's slot.
reg integer_owner_r = 0;
always @(posedge clk) if (mul_in_valid) integer_owner_r <= integer_in_valid;
assign integer_out_valid = pipe_out_valid && integer_owner_r;
assign integer_out_product = mul_out_product[31:0];
wire mul_out_valid = pipe_out_valid && !integer_owner_r;
wire [8:0] mul_in_tag = 0;
wire [8:0] mul_out_tag;
CpuV3FpuExecMul mul_pipe (
 .clk(clk), .abort(abort), .in_valid(mul_in_valid), .in_a(mul_in_a), .in_b(mul_in_b), .in_tag(9'd0),
 .out_valid(pipe_out_valid), .out_product(mul_out_product), .out_tag(mul_out_tag), .ordinary_pending()
);
wire signed [63:0] acc_plus = acc_r + mul_out_product;
always @(posedge clk) begin
 if (abort) begin
  run_r <= 0; return_valid <= 0;
 end else begin
  return_valid <= issue && product_r;
  if (issue) begin
   return_destination <= issue_destination;
   return_last <= issue_last;
   if (cmp_op) begin flag_lt_r <= shared_alu_lt; flag_eq_r <= shared_alu_eq; flag_gt_r <= shared_alu_gt; end
  end
  if (prepare) begin
   lane_r <= 0;
   last_lane_r <= vector_op ? prepared_last : 2'd0;
   destination_r <= {1'b0,word[15:10]};
   read_a_r <= base_a + 6'd1;
   read_b_r <= base_b + prepared_stride;
  end else if (issue) begin
   run_r <= !issue_last;
   lane_r <= lane_r + 2'd1;
   read_a_r <= read_a_r + 6'd1;
   read_b_r <= read_b_r + stride;
   if (!dot_r) destination_r <= destination_r + 7'd1;
  end
  if (normal_load && dot_r && vector_subop != 14) acc_r <= 0;
  if (own_return && dot_r) begin
   if (return_last && vector_subop == 15) acc_r <= 0;
   else acc_r <= acc_plus;
  end
 end
end

wire write_alu = issue && !product_r && !cmp_op && !ext_access;
wire write_normal = own_return && multiply_op && !ext_access;
wire write_dot = own_return && dot_r && return_last && vector_subop == 15 && !ext_access;
wire write_special = sf_write_enable && !ext_access;
wire write_external = ext_access && ext_write_enable;
wire [8:0] normal_destination = {2'd0,return_destination};
wire [5:0] special_destination = fd + sf_write_address[0];
wire rf_write_enable = write_alu || write_normal || write_dot || write_special || write_external;
wire [8:0] rf_write_address = ({9{write_alu}} & {3'd0,issue_destination[5:0]}) | ({9{write_normal}} & normal_destination) | ({9{write_dot}} & {3'd0,fd}) |
 ({9{write_special}} & {3'd0,special_destination}) | ({9{write_external}} & ext_write_address);
wire [31:0] normal_data = mul_out_product[47:16];
wire [31:0] rf_write_data = ({32{write_alu}} & shared_alu_result) | ({32{write_normal}} & normal_data) | ({32{write_dot}} & acc_plus[47:16]) |
 ({32{write_special}} & sf_write_data) | ({32{write_external}} & ext_write_data);
CpuV3FpuExecRf rf (
 .clk(clk), .write_enable(rf_write_enable), .write_address(rf_write_address), .write_data(rf_write_data),
 .read_a_address(rf_read_a_address), .read_b_address(rf_read_b_address), .read_b_enable(rf_read_b_enable),
 .read_a_data(rf_read_a_data), .read_b_data(rf_read_b_data)
);
CpuV3FpuExecSpecial special_path (
 .clk(clk), .abort(abort), .instr_complete(fe_instr_complete), .instr_opcode(instr_opcode), .word1_raw(word1_raw),
 .rf_read_a_data(rf_read_a_data), .rf_read_b_data(rf_read_b_data), .rf_read_a_address(sf_read_a_address),
 .rf_read_b_address(sf_read_b_address), .rf_write_enable(sf_write_enable), .rf_write_address(sf_write_address),
 .rf_write_data(sf_write_data), .busy(sf_busy), .r_wait(), .w_wait(), .x_wait(),
 .mul_in_valid(sf_mul_in_valid), .mul_in_a(sf_mul_in_a), .mul_in_b(sf_mul_in_b), .mul_in_tag(sf_mul_in_tag),
 .mul_out_valid(mul_out_valid), .mul_out_product(mul_out_product), .mul_out_tag(9'd0)
);
assign busy = (normal_load || run_r || return_valid || sf_busy) && !abort;
assign flag_lt = flag_lt_r; assign flag_eq = flag_eq_r; assign flag_gt = flag_gt_r;
assign instr_complete = fe_instr_complete; assign ext_read_data = rf_read_b_data;
endmodule
