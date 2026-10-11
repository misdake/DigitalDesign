// One lane issuer and return owner for MUL/VMULS/DOT/DOTADD/DOTSTORE.
// The RF read window, shared multiplier and DOTSTORE final write beat retain
// the old enabled-edge calendar. ACC keeps full signed64 modulo arithmetic.
module CpuV3FpuProductPath (
    input wire clk, abort, instr_complete,
    input wire [3:0] instr_opcode,
    input wire [15:0] word1_raw,
    input wire [5:0] base_a, base_b,
    input wire [31:0] rf_read_a_data, rf_read_b_data,
    output wire [8:0] rf_read_a_address, rf_read_b_address,
    output wire mul_in_valid,
    output wire signed [35:0] mul_in_a, mul_in_b,
    output wire [8:0] mul_in_tag,
    input wire mul_out_valid,
    input wire signed [63:0] mul_out_product,
    input wire [8:0] mul_out_tag,
    output wire rf_write_enable,
    output wire [8:0] rf_write_address,
    output wire [31:0] rf_write_data,
    output wire busy,
    output wire [63:0] acc_out
);
wire vector_op = instr_opcode == 4'hC;
wire scalar_op = instr_opcode == 4'hD;
wire [4:0] subop = word1_raw[7:3];
wire multiply_op = (vector_op && (subop == 5'h02 || subop == 5'h03)) ||
                   (scalar_op && word1_raw[9:4] == 6'h02);
wire dot_op = vector_op && (subop == 5'h0D || subop == 5'h0E || subop == 5'h0F);
wire load_now = instr_complete && (multiply_op || dot_op) && !abort;
wire [2:0] decoded_last = scalar_op ? 3'd0 :
    (word1_raw[9:8] == 2'b11 ? 3'd3 : word1_raw[9:8] + 3'd1);
wire [2:0] decoded_stride = dot_op ?
    (word1_raw[1:0] == 2'b01 ? 3'd3 :
     word1_raw[1:0] == 2'b10 ? 3'd4 : 3'd1) :
    (vector_op && subop == 5'h03 ? 3'd0 : 3'd1);

reg run_r = 0;
reg [2:0] lane_r = 0, last_lane_r = 0;
reg [5:0] base_a_r = 0, base_b_r = 0;
reg [2:0] stride_r = 1;
reg dot_r = 0, store_mode_r = 0;
reg [8:0] waddr_r = 0;
reg [3:0] outstanding_r = 0;
reg [2:0] acc_lane_r = 0;
reg signed [63:0] acc_r = 0;
reg store_r = 0;
reg [31:0] store_data_r = 0;

wire reading = run_r && !abort && lane_r <= last_lane_r;
// Reuse the two base fields as the next synchronous RF read cursors.
wire [5:0] read_a = base_a_r;
wire [5:0] read_b = base_b_r;
assign rf_read_a_address = load_now ? {3'd0,base_a} :
    (reading ? {3'd0,read_a} : 9'd0);
assign rf_read_b_address = load_now ? {3'd0,base_b} :
    (reading ? {3'd0,read_b} : 9'd0);
wire data_valid = run_r && !abort && lane_r >= 1 && lane_r-3'd1 <= last_lane_r;
wire own_return = mul_out_valid && outstanding_r != 0;
wire signed [63:0] acc_plus = acc_r + mul_out_product;

always @(posedge clk) begin
    if (abort) begin
        run_r <= 0;
        outstanding_r <= 0;
        store_mode_r <= 0;
        store_r <= 0;
        // Already accumulated ACC and previously written MUL lanes survive.
    end else if (load_now) begin
        run_r <= 1;
        lane_r <= 1;
        last_lane_r <= decoded_last;
        // T0 reads live bases; the first registered cursor names lane one.
        base_a_r <= base_a + 6'd1;
        base_b_r <= base_b + decoded_stride;
        stride_r <= decoded_stride;
        dot_r <= dot_op;
        store_mode_r <= dot_op && subop == 5'h0F;
        waddr_r <= {3'd0,word1_raw[15:10]};
        outstanding_r <= 0;
        acc_lane_r <= 0;
        store_r <= 0;
        if (dot_op && subop != 5'h0E) acc_r <= 0;
    end else begin
        if (run_r) begin
            if (reading) begin
                base_a_r <= base_a_r + 6'd1;
                base_b_r <= base_b_r + stride_r;
            end
            if (lane_r > last_lane_r + 3'd1) run_r <= 0;
            else lane_r <= lane_r + 3'd1;
            if (!dot_r) waddr_r <= waddr_r + 9'd1;
        end
        outstanding_r <= outstanding_r + {3'd0,data_valid} - {3'd0,own_return};
        if (own_return && dot_r) begin
            acc_r <= acc_plus;
            acc_lane_r <= acc_lane_r + 3'd1;
            if (store_mode_r && acc_lane_r == last_lane_r) begin
                store_data_r <= acc_plus[47:16];
                store_r <= 1;
                store_mode_r <= 0;
            end
        end
        if (store_r) begin
            store_r <= 0;
            acc_r <= 0;
        end
    end
end

assign mul_in_valid = data_valid;
assign mul_in_a = {{4{rf_read_a_data[31]}},rf_read_a_data};
assign mul_in_b = {{4{rf_read_b_data[31]}},rf_read_b_data};
assign mul_in_tag = dot_r ? {6'd0,lane_r-3'd1} : waddr_r;
assign rf_write_enable = ((own_return && !dot_r) || store_r) && !abort;
assign rf_write_address = store_r ? waddr_r : mul_out_tag;
assign rf_write_data = store_r ? store_data_r : mul_out_product[47:16];
assign busy = (load_now || run_r || outstanding_r != 0 || store_r) && !abort;
assign acc_out = acc_r;
endmodule
