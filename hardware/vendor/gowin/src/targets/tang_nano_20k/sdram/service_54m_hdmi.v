wire video_serial_clock;
wire video_pixel_clock;
wire video_locked;
TangNano20KVideoPll720p u_video_pll(
    .clkin(clk), .serial_clock(video_serial_clock),
    .pixel_clock(video_pixel_clock), .locked(video_locked)
);

wire logic_clk;
wire sdram_clk_180;
wire sdram_pll_locked;
reg [1:0] sdram_reset_sync = 2'b00;
wire [31:0] sdram_read_data;
wire sdram_read_valid;
wire sdram_init_done;
wire sdram_command_ack;
wire sdram_command_valid;
wire [2:0] sdram_command;
wire sdram_precharge;
wire [20:0] sdram_address;
wire [3:0] sdram_write_mask;
wire [31:0] sdram_write_data;
wire [7:0] sdram_burst_length;
reg [3:0] sdram_read_phase = 0;
always @(posedge logic_clk) begin
    if (sdram_command_valid && sdram_command == 3'b101) sdram_read_phase <= 1;
    else if (sdram_read_phase != 0 && sdram_read_phase < 11) sdram_read_phase <= sdram_read_phase + 1'b1;
    else sdram_read_phase <= 0;
end
assign sdram_read_valid = sdram_read_phase >= 3 && sdram_read_phase <= 10;
TangNano20KSdramPll54M u_sdram_pll(.clkin(clk),.logic_clk(logic_clk),.sdram_clk(sdram_clk_180),.locked(sdram_pll_locked));
always @(posedge logic_clk) sdram_reset_sync <= {sdram_reset_sync[0],sdram_pll_locked};
SDRAM_Controller_HS_QN88 u_sdram_controller(
 .O_sdram_clk(O_sdram_clk),.O_sdram_cke(O_sdram_cke),.O_sdram_cs_n(O_sdram_cs_n),
 .O_sdram_cas_n(O_sdram_cas_n),.O_sdram_ras_n(O_sdram_ras_n),.O_sdram_wen_n(O_sdram_wen_n),
 .O_sdram_dqm(O_sdram_dqm),.O_sdram_addr(O_sdram_addr),.O_sdram_ba(O_sdram_ba),.IO_sdram_dq(IO_sdram_dq),
 .I_sdrc_rst_n(sdram_reset_sync[1]),.I_sdrc_clk(logic_clk),.I_sdram_clk(sdram_clk_180),
 .I_sdrc_cmd_en(sdram_command_valid),.I_sdrc_cmd(sdram_command),.I_sdrc_precharge_ctrl(sdram_precharge),
 .I_sdram_power_down(1'b0),.I_sdram_selfrefresh(1'b0),.I_sdrc_addr(sdram_address),
 .I_sdrc_dqm(sdram_write_mask),.I_sdrc_data(sdram_write_data),.I_sdrc_data_len(sdram_burst_length),
 .O_sdrc_data(sdram_read_data),.O_sdrc_init_done(sdram_init_done),.O_sdrc_cmd_ack(sdram_command_ack));
