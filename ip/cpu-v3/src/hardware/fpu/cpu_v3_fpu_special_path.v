// Experimental B special-function controller: all range and LUT interpolation
// products use the existing two-stage shared signed-36 multiplier. No local
// multiplication operator or MULT18 resource remains here.
//
// RCP/RSQRT: T0 operand; T1 packed LUT address; T2 issue delta*residue;
// T3 registered product; T4 direct correction+scale+sign into the RF write.
// SINCOS: T0 range issue; T2 phase return; T3 q/f; T4 first LUT address;
// T5 first interpolation issue + second LUT address; T6 second issue (dual);
// T7 first direct RF write; T8 second direct RF write (dual).
// Compared with the matched baseline, each special instruction is one beat
// slower. The architectural values, interpolation truncation and saturation
// are unchanged. Endpoint/sign travel in tag bits 1/0; the LUT base travels
// through matching two metadata registers. One blocking instruction holds
// rescale, destination, quadrant and mode context until its final write.
// Tag bit8 is the external integer owner; bit7 interpolation; bit6 SINCOS.
// SINCOS range uses tag0; RCP/RSQRT uses 0x080. Abort masks all writes and
// clears context/metadata validity; the shared pipe cancels all in-flight tags.
module CpuV3FpuSpecialPath (
    input wire clk,
    input wire abort,
    input wire instr_complete,
    input wire [3:0] instr_opcode,
    input wire [15:0] word1_raw,
    input wire [31:0] rf_read_a_data,
    input wire [31:0] rf_read_b_data,
    output wire [8:0] rf_read_a_address,
    output wire [8:0] rf_read_b_address,
    output wire rf_write_enable,
    output wire [8:0] rf_write_address,
    output wire [31:0] rf_write_data,
    output wire busy,
    output wire [3:0] r_wait,
    output wire [3:0] w_wait,
    output wire [3:0] x_wait,
    output wire mul_in_valid,
    output wire signed [35:0] mul_in_a,
    output wire signed [35:0] mul_in_b,
    output wire [8:0] mul_in_tag,
    input wire mul_out_valid,
    input wire signed [63:0] mul_out_product,
    input wire [8:0] mul_out_tag
);

wire [5:0] subop = word1_raw[9:4];
wire [5:0] fd = word1_raw[15:10];
wire [1:0] mode = word1_raw[1:0];
wire mode_single = mode[0] ^ mode[1];

wire is_scalar = (instr_opcode == 4'hD);
wire is_rcp = is_scalar && (subop == 6'h0C);
wire is_rsqrt = is_scalar && (subop == 6'h0D);
wire is_sincos = is_scalar && (subop == 6'h0E);
wire is_special = is_rcp || is_rsqrt || is_sincos;

wire load_now = instr_complete && is_special && !abort;

wire [31:0] x0 = rf_read_a_data;
wire x0_negative = x0[31];
wire [31:0] x0_magnitude = x0_negative ? (~x0 + 32'd1) : x0;

reg p0_valid = 1'b0;
reg p0_rcp = 1'b0;
reg p0_negative = 1'b0;
reg [31:0] p0_magnitude = 32'd0;
reg [5:0] p0_fd = 6'd0;

wire p0_zero = (p0_magnitude == 32'd0);

wire [5:0] clz = clz32(p0_magnitude);

wire [31:0] normalized = p0_magnitude << clz[4:0];
wire [6:0] norm_index = normalized[30:24];
wire [8:0] norm_residue = normalized[23:15];

wire rcp_left = (clz >= 6'd15);
wire [4:0] rcp_shift = rcp_left
    ? (clz[4:0] - 5'd15)
    : (5'd15 - clz[4:0]);

wire rsqrt_odd = !clz[0];
wire rsqrt_left = (clz > 6'd15);
wire [5:0] rsqrt_distance = rsqrt_left
    ? (clz - 6'd15)
    : (6'd15 - clz);
wire [4:0] rsqrt_shift = rsqrt_left
    ? ((rsqrt_distance + 6'd1) >> 1)
    : (rsqrt_distance >> 1);

wire [8:0] table_address = p0_rcp
    ? {2'b01, norm_index}
    : (rsqrt_odd ? {2'b10, norm_index} : {2'b01, norm_index});

reg s1_valid = 1'b0;
reg [8:0] s1_residue = 9'd0;
reg [4:0] s1_shift = 5'd0;
reg s1_left = 1'b0;
reg s1_zero = 1'b0;

reg interp_wait = 1'b0;
reg meta_s1_valid = 1'b0;
reg [16:0] meta_s1_base = 17'd0;
reg [16:0] meta_s2_base = 17'd0;
wire rcp_return = mul_out_valid && mul_out_tag == 9'h080;
wire sc_return = mul_out_valid && mul_out_tag[7:6] == 2'b11;
wire [16:0] s2_interpolated;

localparam [35:0] SINCOS_K = 36'h0_A2F9836E;

reg sc_valid = 1'b0;
reg [3:0] sc_stage = 4'd0;
reg [5:0] sc_fd = 6'd0;
reg [1:0] sc_mode = 2'b00;
reg [1:0] sc_q = 2'd0;
reg [15:0] sc_f = 16'd0;
wire signed [17:0] sc_result_reg;

reg [17:0] sc_phase_reg = 18'd0;

always @(posedge clk) begin
    if (mul_out_valid && mul_out_tag == 9'd0)
        sc_phase_reg <= mul_out_product[49:32];
end

wire [16:0] sc_u_sin = sc_q[0]
    ? (17'h10000 - {1'b0, sc_f})
    : {1'b0, sc_f};
wire [16:0] sc_u_cos = sc_q[0]
    ? {1'b0, sc_f}
    : (17'h10000 - {1'b0, sc_f});
wire sc_single = sc_mode[0] ^ sc_mode[1];
wire sc_first_is_cos = sc_mode[1] && !sc_mode[0];
wire sc_interp_is_cos = sc_first_is_cos || (!sc_single && (sc_stage == 4'd6));
wire [16:0] sc_interp_u = sc_interp_is_cos ? sc_u_cos : sc_u_sin;
wire signed [17:0] sc_residue_ext = $signed({10'b0, sc_interp_u[7:0]});

wire [16:0] sc_current = rf_read_a_data[16:0];
wire signed [9:0] sc_delta_raw = $signed(rf_read_a_data[26:17]);
wire signed [17:0] sc_delta_ext = {{8{sc_delta_raw[9]}}, sc_delta_raw};

wire signed [17:0] mul_x;
wire signed [17:0] mul_y;
wire [16:0] interval_current;
wire sc_result_sign;
wire interp_issue = s1_valid || (sc_valid &&
    (sc_stage == 4'd5 || (!sc_single && sc_stage == 4'd6)));
wire range_issue = load_now && is_sincos;
assign mul_in_valid = (range_issue || interp_issue) && !abort;
assign mul_in_a = range_issue ? $signed({{4{x0[31]}}, x0})
    : $signed({{18{mul_x[17]}}, mul_x});
assign mul_in_b = range_issue ? $signed(SINCOS_K)
    : $signed({{18{mul_y[17]}}, mul_y});
assign mul_in_tag = range_issue ? 9'd0 :
    (sc_valid ? {1'b0,1'b1,1'b1,4'b0,sc_interp_u[16],sc_result_sign} : 9'h080);
always @(posedge clk) begin
    if (abort) begin
        meta_s1_valid <= 1'b0;
    end else begin
        meta_s1_valid <= interp_issue;
        if (interp_issue) meta_s1_base <= sc_valid ? sc_current : interval_current;
        if (meta_s1_valid) meta_s2_base <= meta_s1_base;
    end
end

wire [17:0] sc_phase18 = sc_phase_reg;

wire [31:0] packed_interval = p0_rcp ? rf_read_a_data : rf_read_b_data;
assign interval_current = packed_interval[16:0];
wire signed [9:0] interval_delta = $signed(packed_interval[26:17]);
wire signed [17:0] interp_delta = {{8{interval_delta[9]}}, interval_delta};
wire signed [17:0] interp_residue = $signed({9'b0, s1_residue});

assign mul_x = sc_valid ? sc_delta_ext : interp_delta;
assign mul_y = sc_valid ? sc_residue_ext : interp_residue;
wire signed [35:0] mul_product = mul_out_product[35:0];

wire [16:0] sc_addr_u = ((sc_stage == 4'd5) || sc_first_is_cos)
    ? sc_u_cos
    : sc_u_sin;
wire [8:0] sc_lut_address = 9'd256 + {1'b0, sc_addr_u[15:8]};

wire signed [35:0] interp_shifted = mul_product >>> 9;
wire signed [9:0] interp_correction = interp_shifted[9:0];
wire signed [17:0] interp_sum =
    $signed({1'b0, meta_s2_base}) + {{8{interp_correction[9]}}, interp_correction};

assign s2_interpolated = interp_sum[16:0];

wire signed [35:0] sc_interp_shifted = mul_product >>> 8;
wire signed [17:0] sc_interp_correction = sc_interp_shifted[17:0];
wire signed [17:0] sc_interp_sum =
    $signed({1'b0, meta_s2_base}) + sc_interp_correction;
wire [16:0] sc_interp_mag = mul_out_tag[1] ? 17'h10000 : sc_interp_sum[16:0];
assign sc_result_sign = sc_interp_is_cos ? (sc_q[1] ^ sc_q[0]) : sc_q[1];
wire signed [17:0] sc_positive_result = $signed({1'b0, sc_interp_mag});
wire signed [17:0] sc_signed_result = mul_out_tag[0]
    ? -sc_positive_result
    : sc_positive_result;

assign sc_result_reg = sc_signed_result;

wire [31:0] right_scaled = {15'b0, s2_interpolated} >> s1_shift;
wire [32:0] rcp_left_wide = {16'b0, s2_interpolated} << s1_shift;
wire rcp_left_overflow = |rcp_left_wide[32:31];
wire [31:0] rcp_left_scaled = rcp_left_overflow
    ? 32'h7FFFFFFF
    : {1'b0, rcp_left_wide[30:0]};
wire [31:0] rsqrt_left_scaled = {15'b0, s2_interpolated} << s1_shift;
wire [31:0] scaled = s1_left
    ? (p0_rcp ? rcp_left_scaled : rsqrt_left_scaled)
    : right_scaled;
wire [31:0] rcp_value = p0_negative ? (~scaled + 32'd1) : scaled;
wire [31:0] out_rcp = s1_zero
    ? (p0_negative ? 32'h80000001 : 32'h7FFFFFFF)
    : rcp_value;
wire [31:0] out_rsqrt = s1_zero ? 32'h00000000 : scaled;
wire [31:0] result = p0_rcp ? out_rcp : out_rsqrt;

wire sc_write = sc_valid && sc_return && !abort;
wire [5:0] sc_write_index = sc_fd + ((sc_stage == 4'd8) ? 6'd1 : 6'd0);

assign rf_read_a_address = (p0_valid && p0_rcp) ? table_address
                         : (sc_valid && (sc_stage == 4'd4
                             || (!sc_single && (sc_stage == 4'd5))))
                             ? sc_lut_address
                             : 9'd0;
assign rf_read_b_address = (p0_valid && !p0_rcp) ? table_address : 9'd0;

always @(posedge clk) begin
    if (abort) begin
        p0_valid <= 1'b0;
        s1_valid <= 1'b0;
        interp_wait <= 1'b0;
        sc_valid <= 1'b0;
        sc_stage <= 4'd0;
    end else if (load_now) begin
        if (is_sincos) begin
            sc_valid <= 1'b1;
            sc_stage <= 4'd1;
            sc_fd <= fd;
            sc_mode <= mode;
            p0_valid <= 1'b0;
            s1_valid <= 1'b0;
            interp_wait <= 1'b0;
        end else begin
            sc_valid <= 1'b0;
            p0_valid <= 1'b1;
            p0_rcp <= is_rcp;
            p0_negative <= x0_negative;
            p0_magnitude <= x0_magnitude;
            p0_fd <= fd;
            s1_valid <= 1'b0;
            interp_wait <= 1'b0;
        end
    end else if (sc_valid) begin
        case (sc_stage)
            4'd3: begin
                sc_q <= sc_phase18[17:16];
                sc_f <= sc_phase18[15:0];
            end
            4'd7: if (sc_single) begin
                sc_valid <= 1'b0;
                sc_stage <= 4'd0;
            end
            4'd8: begin
                sc_valid <= 1'b0;
                sc_stage <= 4'd0;
            end
            default: ;
        endcase
        if ((sc_stage != 4'd8) && !((sc_stage == 4'd7) && sc_single))
            sc_stage <= sc_stage + 4'd1;
    end else if (p0_valid) begin
        p0_valid <= 1'b0;
        s1_valid <= 1'b1;
        s1_residue <= norm_residue;
        s1_shift <= p0_rcp ? rcp_shift : rsqrt_shift;
        s1_left <= p0_rcp ? rcp_left : rsqrt_left;
        s1_zero <= p0_rcp ? p0_zero : (p0_negative || p0_zero);
        interp_wait <= 1'b0;
    end else if (s1_valid) begin
        interp_wait <= 1'b1;
        s1_valid <= 1'b0;
    end else if (rcp_return) begin
        interp_wait <= 1'b0;
    end

end

assign rf_write_enable = (rcp_return || sc_write) && !abort;
assign rf_write_address = sc_write ? {3'b000, sc_write_index} : {3'b000, p0_fd};
assign rf_write_data = sc_write
    ? {{14{sc_result_reg[17]}}, sc_result_reg}
    : result;

assign busy = (load_now || p0_valid || s1_valid || interp_wait || sc_valid) && !abort;

assign r_wait = 4'd0;
assign w_wait = load_now ? (is_sincos ? (mode_single ? 4'd8 : 4'd9)
    : 4'd5) : 4'd0;
assign x_wait = load_now ? (is_sincos ? (mode_single ? 4'd8 : 4'd9)
    : 4'd5) : 4'd0;

function [5:0] clz32;
    input [31:0] value;
    reg [5:0] count;
    reg [15:0] v16;
    reg [7:0] v8;
    reg [3:0] v4;
    reg [1:0] v2;
    begin
        if (value == 32'd0) begin
            clz32 = 6'd32;
        end else begin
            count = 6'd0;
            if (value[31:16] == 16'd0) begin
                count = count + 6'd16;
                v16 = value[15:0];
            end else begin
                v16 = value[31:16];
            end
            if (v16[15:8] == 8'd0) begin
                count = count + 6'd8;
                v8 = v16[7:0];
            end else begin
                v8 = v16[15:8];
            end
            if (v8[7:4] == 4'd0) begin
                count = count + 6'd4;
                v4 = v8[3:0];
            end else begin
                v4 = v8[7:4];
            end
            if (v4[3:2] == 2'd0) begin
                count = count + 6'd2;
                v2 = v4[1:0];
            end else begin
                v2 = v4[3:2];
            end
            if (v2[1] == 1'b0)
                count = count + 6'd1;
            clz32 = count;
        end
    end
endfunction

endmodule
