wire video_serial_clock;
wire video_pixel_clock;
wire video_locked;
TangNano20KVideoPll u_video_pll (
    .clkin(clk), .serial_clock(video_serial_clock),
    .pixel_clock(video_pixel_clock), .locked(video_locked)
);

wire logic_clk;
wire controller_clk;
wire sdram_phy_clk;
wire sdram_pll_locked;
wire sdram_request_valid;
wire sdram_write;
wire [20:0] sdram_address;
wire [5:0] sdram_words;
wire [3:0] sdram_write_mask;
wire [63:0] sdram_write_data;
wire sdram_write_data_valid;
wire sdram_request_ready;
wire sdram_write_data_ready;
wire [63:0] sdram_read_data;
wire sdram_read_valid;
wire sdram_done;
wire sdram_init_done;

TangNano20KSdramPll108M54M u_sdram_pll (
    .clkin(clk), .controller_clk(controller_clk), .logic_clk(logic_clk),
    .sdram_clk(sdram_phy_clk), .locked(sdram_pll_locked)
);

TangNano20KSdramNativeBridge108M54M u_sdram_bridge (
    .logic_clk(logic_clk), .controller_clk(controller_clk),
    .sdram_clk(sdram_phy_clk), .reset(!sdram_pll_locked || (|buttons)),
    .request_valid(sdram_request_valid), .writing(sdram_write),
    .address(sdram_address), .words(sdram_words),
    .write_mask(sdram_write_mask), .write_data(sdram_write_data),
    .write_data_valid(sdram_write_data_valid),
    .request_ready(sdram_request_ready),
    .write_data_ready(sdram_write_data_ready),
    .read_data(sdram_read_data), .read_valid(sdram_read_valid),
    .done(sdram_done), .initialized(sdram_init_done),
    .O_sdram_clk(O_sdram_clk), .O_sdram_cke(O_sdram_cke),
    .O_sdram_cs_n(O_sdram_cs_n), .O_sdram_cas_n(O_sdram_cas_n),
    .O_sdram_ras_n(O_sdram_ras_n), .O_sdram_wen_n(O_sdram_wen_n),
    .O_sdram_dqm(O_sdram_dqm), .O_sdram_addr(O_sdram_addr),
    .O_sdram_ba(O_sdram_ba), .IO_sdram_dq(IO_sdram_dq)
);
