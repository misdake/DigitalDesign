// One scalar/vector lane descriptor and one result commit register.
// The scalar operand-read fast entry preserves its original T0 capture/T1 write.
module CpuV3FpuAluPath (
    input wire clk,
    input wire abort,
    input wire instr_complete,
    input wire [3:0] instr_opcode,
    input wire [15:0] word1_raw,
    input wire [5:0] base_a,
    input wire [5:0] base_b,
    input wire [31:0] alu_result,
    input wire alu_lt,
    input wire alu_eq,
    input wire alu_gt,
    output wire [3:0] alu_request_op,
    output wire [8:0] rf_read_a_address,
    output wire [8:0] rf_read_b_address,
    output wire rf_write_enable,
    output wire [8:0] rf_write_address,
    output wire [31:0] rf_write_data,
    output wire flag_lt,
    output wire flag_eq,
    output wire flag_gt,
    output wire vector_busy,
    output wire busy
);
wire [5:0] scalar_subop = word1_raw[9:4];
wire [4:0] vector_subop = word1_raw[7:3];
wire scalar_owned = scalar_subop != 6'h02 && scalar_subop != 6'h0C &&
                    scalar_subop != 6'h0D && scalar_subop != 6'h0E;
wire vector_owned = vector_subop == 5'h00 || vector_subop == 5'h01 ||
                    (vector_subop >= 5'h04 && vector_subop <= 5'h07) ||
                    vector_subop == 5'h0C;
wire scalar_load = instr_complete && instr_opcode == 4'hD && scalar_owned && !abort;
wire vector_load = instr_complete && instr_opcode == 4'hC && vector_owned && !abort;
wire scalar_cmp = scalar_subop == 6'h0B;

reg run_r = 0;
reg scalar_active_r = 0;
reg vector_owner_r = 0;
reg [2:0] lane_r = 0;
reg [2:0] last_lane_r = 0;
reg [5:0] base_a_r = 0;
reg [5:0] base_b_r = 0;
reg [5:0] fd_r = 0;
reg wr_enable_r = 0;
reg [8:0] wr_address_r = 0;
reg [31:0] wr_data_r = 0;
reg flag_lt_r = 0;
reg flag_eq_r = 0;
reg flag_gt_r = 0;

wire [2:0] decoded_last_lane = word1_raw[9:8] == 2'b11 ? 3'd3 : word1_raw[9:8] + 3'd1;
wire reading = run_r && !abort && lane_r <= last_lane_r;
wire data_valid = run_r && !abort && lane_r >= 3'd1 && lane_r - 3'd1 <= last_lane_r;
wire [5:0] read_index_a = base_a_r + lane_r;
wire [5:0] read_index_b = base_b_r + lane_r;
wire [5:0] write_index = fd_r + {3'b000,lane_r - 3'd1};
assign rf_read_a_address = vector_load ? {3'b000,base_a} : reading ? {3'b000,read_index_a} : 9'd0;
assign rf_read_b_address = vector_load ? {3'b000,base_b} : reading ? {3'b000,read_index_b} : 9'd0;

function [3:0] vector_alu_op;
    input [4:0] subop;
    begin
        case (subop)
            5'h00: vector_alu_op = 4'h0;
            5'h01: vector_alu_op = 4'h1;
            5'h04: vector_alu_op = 4'h3;
            5'h05: vector_alu_op = 4'h4;
            5'h06: vector_alu_op = 4'h5;
            5'h07: vector_alu_op = 4'h6;
            5'h0C: vector_alu_op = 4'hF;
            default: vector_alu_op = 4'h2;
        endcase
    end
endfunction
assign alu_request_op = (vector_load || vector_owner_r) ? vector_alu_op(vector_subop) : scalar_subop[3:0];

always @(posedge clk) begin
    if (abort) begin
        run_r <= 0;
        scalar_active_r <= 0;
        vector_owner_r <= 0;
        wr_enable_r <= 0;
    end else begin
        scalar_active_r <= scalar_load;
        if (scalar_load) begin
            vector_owner_r <= 0;
            wr_enable_r <= !scalar_cmp;
            wr_address_r <= {3'b000,word1_raw[15:10]};
            wr_data_r <= alu_result;
            if (scalar_cmp) begin
                flag_lt_r <= alu_lt;
                flag_eq_r <= alu_eq;
                flag_gt_r <= alu_gt;
            end
        end else if (vector_load) begin
            vector_owner_r <= 1;
            run_r <= 1;
            lane_r <= 3'd1;
            last_lane_r <= decoded_last_lane;
            base_a_r <= base_a;
            base_b_r <= base_b;
            fd_r <= word1_raw[15:10];
            wr_enable_r <= 0;
        end else if (run_r) begin
            wr_enable_r <= data_valid;
            if (data_valid) begin
                wr_address_r <= {3'b000,write_index};
                wr_data_r <= alu_result;
            end
            if (lane_r > last_lane_r + 3'd1)
                run_r <= 0;
            else
                lane_r <= lane_r + 3'd1;
        end else begin
            wr_enable_r <= 0;
            vector_owner_r <= 0;
        end
    end
end

assign rf_write_enable = wr_enable_r && !abort;
assign rf_write_address = wr_address_r;
assign rf_write_data = wr_data_r;
assign flag_lt = flag_lt_r;
assign flag_eq = flag_eq_r;
assign flag_gt = flag_gt_r;
assign vector_busy = (vector_load || (vector_owner_r && (run_r || wr_enable_r))) && !abort;
// Preserve the scalar countdown's pre-edge busy level during abort; the vector
// owner's busy and both owners' writes still cancel combinationally.
assign busy = scalar_load || scalar_active_r || vector_busy;
endmodule
