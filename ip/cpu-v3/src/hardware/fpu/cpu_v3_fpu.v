// Unified FPU operand-port calendar and result commit owner.
module CpuV3Fpu (
    input wire clk,
    input wire abort,
    input wire word_valid,
    input wire [15:0] word,
    output wire busy,
    output wire flag_lt,
    output wire flag_eq,
    output wire flag_gt,
    output wire instr_complete,
    input wire ext_access,
    input wire ext_write_enable,
    input wire [8:0] ext_write_address,
    input wire [31:0] ext_write_data,
    input wire [8:0] ext_read_address,
    output wire [31:0] ext_read_data,
    // CPU integer owner: unsigned 16-bit operands; reserved tag bit 8.
    input wire integer_in_valid,
    input wire [15:0] integer_in_a,
    input wire [15:0] integer_in_b,
    output wire integer_out_valid,
    output wire [31:0] integer_out_product
);

// Front-end leaf: two-word pair decode and operand-address generation.
wire read_valid;
wire [8:0] fe_read_a_address;
wire [8:0] fe_read_b_address;
wire [15:0] word0_raw;
wire [15:0] word1_raw;
wire [3:0] instr_opcode;
wire fe_instr_complete;

CpuV3FpuFrontend frontend (
    .clk(clk),
    .word_valid(word_valid),
    .word(word),
    .abort(abort),
    .read_valid(read_valid),
    .rf_read_a_address(fe_read_a_address),
    .rf_read_b_address(fe_read_b_address),
    .word0_raw(word0_raw),
    .word1_raw(word1_raw),
    .instr_opcode(instr_opcode),
    .instr_complete(fe_instr_complete)
);

// Shared 36x36 multiply pipe. The core serializes instructions, so the
// multiply, dot and SINCOS range-reduction owners are never active at once.
// The operand buses are signed 36 bits; the ordinary multiply/dot paths
// sign-extend their architectural signed-32 operands, and SINCOS drives the
// positive 36-bit K constant. The integer owner has first priority, followed
// by the special path (range at its T0) and ordinary multiply/dot. Their
// valid lifetimes are serialized; tags retain explicit return ownership.
wire product_mul_in_valid;
wire [8:0] product_mul_in_tag;
wire sf_mul_in_valid;
wire signed [35:0] sf_mul_in_a;
wire signed [35:0] sf_mul_in_b;
wire [8:0] sf_mul_in_tag;
wire [31:0] rf_read_a_data, rf_read_b_data;
wire mul_in_valid = integer_in_valid || sf_mul_in_valid || product_mul_in_valid;
// Ordinary products and range reduction consume the same RF A word. Only
// interpolation substitutes a signed ten-bit delta. Form extension bits
// after source selection instead of selecting independent wide owner buses.
wire sf_range_operand = sf_mul_in_valid && sf_mul_in_tag == 9'h081;
wire sf_interp_operand = sf_mul_in_valid && !sf_range_operand;
wire [31:0] mul_a_word = integer_in_valid ? {16'd0,integer_in_a} :
    sf_interp_operand ? {{22{sf_mul_in_a[9]}},sf_mul_in_a[9:0]} : rf_read_a_data;
wire signed [35:0] mul_in_a = {{4{mul_a_word[31]}},mul_a_word};
wire [31:0] mul_b_word = integer_in_valid ? {16'd0,integer_in_b} :
    sf_range_operand ? 32'hA2F9836E :
    sf_interp_operand ? {23'd0,sf_mul_in_b[8:0]} : rf_read_b_data;
wire mul_b_negative = !integer_in_valid && !sf_mul_in_valid && rf_read_b_data[31];
wire signed [35:0] mul_in_b = {{4{mul_b_negative}},mul_b_word};
wire [8:0] mul_in_tag = integer_in_valid ? 9'h100 :
    sf_mul_in_valid ? sf_mul_in_tag :
    product_mul_in_tag;
wire pipe_out_valid;
wire ordinary_product_pending;
wire signed [63:0] mul_out_product;
wire [8:0] mul_out_tag;
wire mul_out_valid = pipe_out_valid && !mul_out_tag[8];
assign integer_out_valid = pipe_out_valid && mul_out_tag[8];
// The last product/tag registers retain this result during core hold; no
// new instruction can acquire the pipe until integer commit has retired.
assign integer_out_product = mul_out_product[31:0];


CpuV3FpuMulPipe mul_pipe (
    .clk(clk),
    .abort(abort),
    .in_valid(mul_in_valid),
    .in_a(mul_in_a),
    .in_b(mul_in_b),
    .in_tag(mul_in_tag),
    .out_valid(pipe_out_valid),
    .out_product(mul_out_product),
    .out_tag(mul_out_tag),
    .ordinary_pending(ordinary_product_pending)
);



// One physical 2R1W port calendar. The accepted word0 already retains the
// architectural source addresses; do not duplicate those in holding registers.
wire [5:0] base_a = instr_opcode == 4'hE ? word0_raw[7:2] : word0_raw[11:6];
wire [5:0] base_b = word0_raw[5:0];
wire [5:0] scalar_subop = word1_raw[9:4];
wire [4:0] vector_subop = word1_raw[7:3];
wire scalar_op = instr_opcode == 4'hD;
wire vector_op = instr_opcode == 4'hC;
wire scalar_alu_owned = scalar_subop != 6'h02 && scalar_subop != 6'h0C &&
                        scalar_subop != 6'h0D && scalar_subop != 6'h0E;
wire vector_alu_owned = vector_subop == 5'h00 || vector_subop == 5'h01 ||
    (vector_subop >= 5'h04 && vector_subop <= 5'h07) || vector_subop == 5'h0C;
wire multiply_op = (vector_op && (vector_subop == 5'h02 || vector_subop == 5'h03)) ||
                   (scalar_op && scalar_subop == 6'h02);
wire dot_op = vector_op && (vector_subop == 5'h0D || vector_subop == 5'h0E || vector_subop == 5'h0F);
wire scalar_load = fe_instr_complete && scalar_op && scalar_alu_owned && !abort;
wire vector_load = fe_instr_complete && vector_op && vector_alu_owned && !abort;
wire product_load = fe_instr_complete && (multiply_op || dot_op) && !abort;
wire normal_load = vector_load || product_load;
wire [2:0] decoded_last = scalar_op ? 3'd0 :
    (word1_raw[9:8] == 2'b11 ? 3'd3 : word1_raw[9:8] + 3'd1);
wire [2:0] decoded_stride = dot_op ?
    (word1_raw[1:0] == 2'b01 ? 3'd3 : word1_raw[1:0] == 2'b10 ? 3'd4 : 3'd1) :
    (vector_op && vector_subop == 5'h03 ? 3'd0 : 3'd1);

reg run_r = 0, scalar_active_r = 0, product_r = 0, vector_r = 0;
reg dot_r = 0, store_mode_r = 0;
reg [2:0] lane_r = 0, last_lane_r = 0, stride_r = 1;
reg [5:0] read_a_r = 0, read_b_r = 0;
reg [8:0] destination_r = 0;
reg signed [63:0] acc_r = 0;
// ALU results and the final DOTSTORE result have disjoint lifetimes and the
// same next-edge consumer. One commit slot preserves both existing schedules.
reg commit_valid = 0;
reg [8:0] commit_address = 0;
reg [31:0] commit_data = 0;
reg flag_lt_r = 0, flag_eq_r = 0, flag_gt_r = 0;
wire reading = run_r && !abort && lane_r <= last_lane_r;
wire data_valid = run_r && !abort && lane_r >= 1 && lane_r-3'd1 <= last_lane_r;
// Ordinary destinations are 0..66 (Fd <= 63, at most four lanes); special
// tags all set bit7, and integer tags set bit8. Keep all nine address bits.
wire own_return = mul_out_valid && !mul_out_tag[7];
wire signed [63:0] acc_plus = acc_r + mul_out_product;
wire vector_busy = (vector_load || (vector_r && (run_r || commit_valid))) && !abort;
wire product_busy = (product_load || (product_r &&
    (run_r || ordinary_product_pending || commit_valid))) && !abort;
wire normal_ports = vector_busy || product_busy;
wire [8:0] normal_address_a = normal_load ? {3'b0,base_a} :
    reading ? {3'b0,read_a_r} : 9'd0;
wire [8:0] normal_address_b = normal_load ? {3'b0,base_b} :
    reading ? {3'b0,read_b_r} : 9'd0;
wire sf_busy;
wire [8:0] sf_read_a_address, sf_read_b_address;
wire sf_write_enable;
wire [8:0] sf_write_address;
wire [31:0] sf_write_data;
wire [8:0] rf_read_a_address = sf_busy ? sf_read_a_address :
    normal_ports ? normal_address_a : {3'b0,base_a};
wire [8:0] rf_read_b_address = sf_busy ? sf_read_b_address :
    normal_ports ? normal_address_b : ext_access ? ext_read_address : {3'b0,base_b};

function [3:0] vector_alu_op;
    input [4:0] subop;
    begin
        case (subop)
            5'h00: vector_alu_op=4'h0;
            5'h01: vector_alu_op=4'h1;
            5'h04: vector_alu_op=4'h3;
            5'h05: vector_alu_op=4'h4;
            5'h06: vector_alu_op=4'h5;
            5'h07: vector_alu_op=4'h6;
            5'h0C: vector_alu_op=4'hF;
            default: vector_alu_op=4'h2;
        endcase
    end
endfunction
wire [3:0] alu_request_op = vector_op ? vector_alu_op(vector_subop) : scalar_subop[3:0];
wire [31:0] shared_alu_result;
wire shared_alu_lt, shared_alu_eq, shared_alu_gt;
CpuV3FpuScalarAlu shared_alu (
    .a(rf_read_a_data), .b(rf_read_b_data), .op(alu_request_op),
    .result(shared_alu_result), .flag_lt(shared_alu_lt),
    .flag_eq(shared_alu_eq), .flag_gt(shared_alu_gt)
);

always @(posedge clk) begin
    if (abort) begin
        run_r <= 0;
        scalar_active_r <= 0;
        commit_valid <= 0;
        store_mode_r <= 0;
    end else begin
        scalar_active_r <= scalar_load;
        commit_valid <= 0;
        // This is the sole destination context for ALU, product and special
        // instructions. SpecialPath receives no private destination field.
        if (fe_instr_complete) destination_r <= {3'b0,word1_raw[15:10]};
        if (scalar_load) begin
            product_r <= 0;
            vector_r <= 0;
            run_r <= 0;
            commit_valid <= scalar_subop != 6'h0B;
            commit_address <= {3'b0,word1_raw[15:10]};
            commit_data <= shared_alu_result;
            if (scalar_subop == 6'h0B) begin
                flag_lt_r <= shared_alu_lt;
                flag_eq_r <= shared_alu_eq;
                flag_gt_r <= shared_alu_gt;
            end
        end
        if (normal_load) begin
            run_r <= 1;
            product_r <= product_load;
            vector_r <= vector_load;
            lane_r <= 1;
            last_lane_r <= decoded_last;
            read_a_r <= base_a + 6'd1;
            read_b_r <= base_b + decoded_stride;
            stride_r <= decoded_stride;
            dot_r <= dot_op;
            store_mode_r <= dot_op && vector_subop == 5'h0F;
            if (dot_op && vector_subop != 5'h0E) acc_r <= 0;
        end else begin
            if (run_r) begin
                if (reading) begin
                    read_a_r <= read_a_r + 6'd1;
                    read_b_r <= read_b_r + stride_r;
                end
                if (lane_r > last_lane_r + 3'd1) run_r <= 0;
                else lane_r <= lane_r + 3'd1;
                if (data_valid && !dot_r) destination_r <= destination_r + 9'd1;
                if (data_valid && !product_r) begin
                    commit_valid <= 1;
                    commit_address <= {3'b0,destination_r[5:0]};
                    commit_data <= shared_alu_result;
                end
            end
            if (own_return && dot_r) begin
                acc_r <= acc_plus;
                if (store_mode_r && mul_out_tag[2:0] == last_lane_r) begin
                    commit_data <= acc_plus[47:16];
                    commit_address <= destination_r;
                    commit_valid <= 1;
                    store_mode_r <= 0;
                end
            end
            if (commit_valid && product_r && dot_r) acc_r <= 0;
        end
    end
end

assign product_mul_in_valid = data_valid && product_r;
assign product_mul_in_tag = dot_r ? {6'd0,lane_r-3'd1} : destination_r;

// Exactly one live producer can use the final write port. Keep direct MUL
// and special returns direct; only the existing ALU/DOTSTORE edge is staged.
wire write_commit = commit_valid && !abort && !ext_access;
wire write_product = own_return && !dot_r && !abort && !ext_access;
wire write_special = sf_write_enable && !ext_access;
wire write_external = ext_access && ext_write_enable;
wire [5:0] special_destination = destination_r[5:0] + sf_write_address[5:0];
wire rf_write_enable = write_commit | write_product | write_special | write_external;
wire [8:0] rf_write_address =
    ({9{write_commit}} & commit_address) |
    ({9{write_product}} & mul_out_tag) |
    ({9{write_special}} & {3'b0,special_destination}) |
    ({9{write_external}} & ext_write_address);
wire [31:0] rf_write_data =
    ({32{write_commit}} & commit_data) |
    ({32{write_product}} & mul_out_product[47:16]) |
    ({32{write_special}} & sf_write_data) |
    ({32{write_external}} & ext_write_data);
CpuV3FpuRegisterRam rf (
    .clk(clk), .write_enable(rf_write_enable),
    .write_address(rf_write_address), .write_data(rf_write_data),
    .read_a_address(rf_read_a_address), .read_b_address(rf_read_b_address),
    .read_a_data(rf_read_a_data), .read_b_data(rf_read_b_data)
);
assign flag_lt = flag_lt_r;
assign flag_eq = flag_eq_r;
assign flag_gt = flag_gt_r;
assign busy = scalar_load || scalar_active_r || vector_busy || product_busy || sf_busy;
assign instr_complete = fe_instr_complete;
assign ext_read_data = rf_read_b_data;

CpuV3FpuSpecialPath special_path (
    .clk(clk),
    .abort(abort),
    .instr_complete(fe_instr_complete),
    .instr_opcode(instr_opcode),
    .word1_raw({6'b0, word1_raw[9:0]}),
    .rf_read_a_data(rf_read_a_data),
    .rf_read_b_data(rf_read_b_data),
    .rf_read_a_address(sf_read_a_address),
    .rf_read_b_address(sf_read_b_address),
    .rf_write_enable(sf_write_enable),
    .rf_write_address(sf_write_address),
    .rf_write_data(sf_write_data),
    .busy(sf_busy),
    .r_wait(),
    .w_wait(),
    .x_wait(),
    .mul_in_valid(sf_mul_in_valid),
    .mul_in_a(sf_mul_in_a),
    .mul_in_b(sf_mul_in_b),
    .mul_in_tag(sf_mul_in_tag),
    .mul_out_valid(mul_out_valid),
    .mul_out_product(mul_out_product),
    .mul_out_tag(mul_out_tag)
);


endmodule
