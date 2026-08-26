// Tang Nano 20K 1280x720p60 DVI-compatible HDMI bring-up pattern.
// Button1 overlays a white 32-pixel grid; Button2 selects a grayscale ramp.
module HdmiColorBars (
    input wire clk,
    input wire [1:0] buttons,
    output wire [5:0] leds,
    output wire tmds_clk_p,
    output wire tmds_clk_n,
    output wire [2:0] tmds_data_p,
    output wire [2:0] tmds_data_n
);

wire serial_clk;
wire pixel_clk;
wire pll_locked;

`ifdef __ICARUS__
assign serial_clk = clk;
assign pixel_clk = clk;
assign pll_locked = 1'b1;
`else
HdmiPll720p u_pll (
    .clkin(clk),
    .clkout(serial_clk),
    .lock(pll_locked)
);

CLKDIV u_pixel_divider (
    .RESETN(pll_locked),
    .HCLKIN(serial_clk),
    .CLKOUT(pixel_clk),
    .CALIB(1'b1)
);
defparam u_pixel_divider.DIV_MODE = "5";
defparam u_pixel_divider.GSREN = "false";
`endif

reg [2:0] reset_sync = 3'b000;
always @(posedge pixel_clk)
    reset_sync <= {reset_sync[1:0], pll_locked};
wire video_reset = ~reset_sync[2];

reg [1:0] button_meta = 0;
reg [1:0] button_sync = 0;
always @(posedge pixel_clk) begin
    button_meta <= buttons;
    button_sync <= button_meta;
end

// Reference timing used by the validated Sipeed/Gowin example. Active video
// follows sync and back porch; the remaining samples form the front porch.
reg [10:0] h_count = 0;
reg [9:0] v_count = 0;
always @(posedge pixel_clk) begin
    if (video_reset) begin
        h_count <= 0;
        v_count <= 0;
    end else if (h_count == 11'd1649) begin
        h_count <= 0;
        if (v_count == 10'd749)
            v_count <= 0;
        else
            v_count <= v_count + 1'b1;
    end else begin
        h_count <= h_count + 1'b1;
    end
end

wire hsync = h_count < 11'd40;
wire vsync = v_count < 10'd5;
wire active = h_count >= 11'd260 && h_count < 11'd1540 &&
              v_count >= 10'd25 && v_count < 10'd745;
wire [10:0] active_x = h_count - 11'd260;
wire [9:0] active_y = v_count - 10'd25;

reg [23:0] rgb_next;
always @* begin
    if (!active) begin
        rgb_next = 24'h000000;
    end else if (button_sync[1]) begin
        rgb_next = {active_x[7:0], active_x[7:0], active_x[7:0]};
    end else if (button_sync[0] &&
                 (active_x[4:0] == 0 || active_y[4:0] == 0)) begin
        rgb_next = 24'hffffff;
    end else begin
        case (active_x / 11'd160)
            0: rgb_next = 24'hffffff;
            1: rgb_next = 24'hffff00;
            2: rgb_next = 24'h00ffff;
            3: rgb_next = 24'h00ff00;
            4: rgb_next = 24'hff00ff;
            5: rgb_next = 24'hff0000;
            6: rgb_next = 24'h0000ff;
            default: rgb_next = 24'h000000;
        endcase
    end
end

// Break the timing-generator/pattern path from the TMDS disparity path. All
// four video signals receive the same one-pixel delay.
reg [23:0] rgb = 0;
reg active_pipe = 0;
reg hsync_pipe = 0;
reg vsync_pipe = 0;
always @(posedge pixel_clk) begin
    if (video_reset) begin
        rgb <= 0;
        active_pipe <= 0;
        hsync_pipe <= 0;
        vsync_pipe <= 0;
    end else begin
        rgb <= rgb_next;
        active_pipe <= active;
        hsync_pipe <= hsync;
        vsync_pipe <= vsync;
    end
end

wire [9:0] blue_symbol;
wire [9:0] green_symbol;
wire [9:0] red_symbol;
HdmiTmdsEncoder u_blue (
    .clk(pixel_clk), .reset(video_reset), .de(active_pipe),
    .control({vsync_pipe, hsync_pipe}), .data(rgb[7:0]), .symbol(blue_symbol)
);
HdmiTmdsEncoder u_green (
    .clk(pixel_clk), .reset(video_reset), .de(active_pipe),
    .control(2'b00), .data(rgb[15:8]), .symbol(green_symbol)
);
HdmiTmdsEncoder u_red (
    .clk(pixel_clk), .reset(video_reset), .de(active_pipe),
    .control(2'b00), .data(rgb[23:16]), .symbol(red_symbol)
);

wire [3:0] serialized;
`ifdef __ICARUS__
assign serialized = {pixel_clk, red_symbol[0], green_symbol[0], blue_symbol[0]};
assign tmds_clk_p = serialized[3];
assign tmds_clk_n = ~serialized[3];
assign tmds_data_p = serialized[2:0];
assign tmds_data_n = ~serialized[2:0];
`else
HdmiSerializer10 u_blue_serializer (
    .pixel_clk(pixel_clk), .serial_clk(serial_clk), .reset(1'b0),
    .data(blue_symbol), .serial(serialized[0])
);
HdmiSerializer10 u_green_serializer (
    .pixel_clk(pixel_clk), .serial_clk(serial_clk), .reset(1'b0),
    .data(green_symbol), .serial(serialized[1])
);
HdmiSerializer10 u_red_serializer (
    .pixel_clk(pixel_clk), .serial_clk(serial_clk), .reset(1'b0),
    .data(red_symbol), .serial(serialized[2])
);
HdmiSerializer10 u_clock_serializer (
    .pixel_clk(pixel_clk), .serial_clk(serial_clk), .reset(1'b0),
    .data(10'b0000011111), .serial(serialized[3])
);

ELVDS_OBUF u_tmds_clock (
    .I(serialized[3]), .O(tmds_clk_p), .OB(tmds_clk_n)
);
ELVDS_OBUF u_tmds_blue (
    .I(serialized[0]), .O(tmds_data_p[0]), .OB(tmds_data_n[0])
);
ELVDS_OBUF u_tmds_green (
    .I(serialized[1]), .O(tmds_data_p[1]), .OB(tmds_data_n[1])
);
ELVDS_OBUF u_tmds_red (
    .I(serialized[2]), .O(tmds_data_p[2]), .OB(tmds_data_n[2])
);
`endif

reg [7:0] frame_count = 0;
always @(posedge pixel_clk)
    if (!video_reset && h_count == 11'd1649 && v_count == 10'd749)
        frame_count <= frame_count + 1'b1;

assign leds = {pll_locked, ~video_reset, frame_count[5], frame_count[4],
               button_sync[1], button_sync[0]};

endmodule

module HdmiTmdsEncoder (
    input wire clk,
    input wire reset,
    input wire de,
    input wire [1:0] control,
    input wire [7:0] data,
    output reg [9:0] symbol
);
integer i;
reg [3:0] data_ones;
reg [3:0] qm_ones;
reg [8:0] qm;
reg signed [5:0] disparity = 0;
wire signed [5:0] qm_delta = $signed({1'b0, qm_ones, 1'b0}) - 6'sd8;

always @* begin
    data_ones = data[0] + data[1] + data[2] + data[3] +
                data[4] + data[5] + data[6] + data[7];
    qm[0] = data[0];
    if (data_ones > 4 || (data_ones == 4 && data[0] == 0)) begin
        for (i = 1; i < 8; i = i + 1)
            qm[i] = ~(qm[i - 1] ^ data[i]);
        qm[8] = 0;
    end else begin
        for (i = 1; i < 8; i = i + 1)
            qm[i] = qm[i - 1] ^ data[i];
        qm[8] = 1;
    end
    qm_ones = qm[0] + qm[1] + qm[2] + qm[3] +
              qm[4] + qm[5] + qm[6] + qm[7];
end

always @(posedge clk) begin
    if (reset) begin
        disparity <= 0;
        symbol <= 10'b1101010100;
    end else if (!de) begin
        disparity <= 0;
        case (control)
            2'b00: symbol <= 10'b1101010100;
            2'b01: symbol <= 10'b0010101011;
            2'b10: symbol <= 10'b0101010100;
            default: symbol <= 10'b1010101011;
        endcase
    end else if (disparity == 0 || qm_ones == 4) begin
        symbol[9] <= ~qm[8];
        symbol[8] <= qm[8];
        symbol[7:0] <= qm[8] ? qm[7:0] : ~qm[7:0];
        if (qm[8])
            disparity <= disparity + qm_delta;
        else
            disparity <= disparity - qm_delta;
    end else if ((disparity > 0 && qm_ones > 4) ||
                 (disparity < 0 && qm_ones < 4)) begin
        symbol <= {1'b1, qm[8], ~qm[7:0]};
        disparity <= disparity - qm_delta + (qm[8] ? 6'sd2 : 6'sd0);
    end else begin
        symbol <= {1'b0, qm[8], qm[7:0]};
        disparity <= disparity + qm_delta - (qm[8] ? 6'sd0 : 6'sd2);
    end
end
endmodule

`ifndef __ICARUS__
module HdmiSerializer10 (
    input wire pixel_clk,
    input wire serial_clk,
    input wire reset,
    input wire [9:0] data,
    output wire serial
);
OSER10 u_serializer (
    .Q(serial),
    .D0(data[0]), .D1(data[1]), .D2(data[2]), .D3(data[3]), .D4(data[4]),
    .D5(data[5]), .D6(data[6]), .D7(data[7]), .D8(data[8]), .D9(data[9]),
    .PCLK(pixel_clk), .FCLK(serial_clk), .RESET(reset)
);
defparam u_serializer.GSREN = "false";
defparam u_serializer.LSREN = "true";
endmodule

module HdmiPll720p (
    input wire clkin,
    output wire clkout,
    output wire lock
);
wire ground = 1'b0;
wire unused_p;
wire unused_d;
wire unused_d3;
rPLL u_pll (
    .CLKOUT(clkout), .LOCK(lock), .CLKOUTP(unused_p),
    .CLKOUTD(unused_d), .CLKOUTD3(unused_d3),
    .RESET(ground), .RESET_P(ground), .CLKIN(clkin), .CLKFB(ground),
    .FBDSEL(6'b0), .IDSEL(6'b0), .ODSEL(6'b0), .PSDA(4'b0),
    .DUTYDA(4'b0), .FDLY(4'b0)
);
defparam u_pll.FCLKIN = "27";
defparam u_pll.DYN_IDIV_SEL = "false";
defparam u_pll.IDIV_SEL = 3;
defparam u_pll.DYN_FBDIV_SEL = "false";
defparam u_pll.FBDIV_SEL = 54;
defparam u_pll.DYN_ODIV_SEL = "false";
defparam u_pll.ODIV_SEL = 2;
defparam u_pll.PSDA_SEL = "0000";
defparam u_pll.DYN_DA_EN = "true";
defparam u_pll.DUTYDA_SEL = "1000";
defparam u_pll.CLKOUT_FT_DIR = 1'b1;
defparam u_pll.CLKOUTP_FT_DIR = 1'b1;
defparam u_pll.CLKOUT_DLY_STEP = 0;
defparam u_pll.CLKOUTP_DLY_STEP = 0;
defparam u_pll.CLKFB_SEL = "internal";
defparam u_pll.CLKOUT_BYPASS = "false";
defparam u_pll.CLKOUTP_BYPASS = "false";
defparam u_pll.CLKOUTD_BYPASS = "false";
defparam u_pll.DYN_SDIV_SEL = 2;
defparam u_pll.CLKOUTD_SRC = "CLKOUT";
defparam u_pll.CLKOUTD3_SRC = "CLKOUT";
defparam u_pll.DEVICE = "GW2AR-18C";
endmodule
`endif
