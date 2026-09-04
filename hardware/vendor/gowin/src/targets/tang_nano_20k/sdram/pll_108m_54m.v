module TangNano20KSdramPll108M54M (
    input wire clkin,
    output wire controller_clk,
    output wire logic_clk,
    output wire sdram_clk,
    output wire locked
);
wire clkoutd3_unused;

// 27 MHz * 4 = 108 MHz. CLKOUTD is the exact divide-by-two CPU clock;
// CLKOUTP is the 180-degree physical SDRAM clock.
rPLL rpll_inst (
    .CLKOUT(controller_clk), .LOCK(locked), .CLKOUTP(sdram_clk),
    .CLKOUTD(logic_clk), .CLKOUTD3(clkoutd3_unused),
    .RESET(1'b0), .RESET_P(1'b0), .CLKIN(clkin), .CLKFB(1'b0),
    .FBDSEL(6'b0), .IDSEL(6'b0), .ODSEL(6'b0),
    .PSDA(4'b0), .DUTYDA(4'b0), .FDLY(4'b0000)
);
defparam rpll_inst.FCLKIN = "27";
defparam rpll_inst.IDIV_SEL = 0;
defparam rpll_inst.FBDIV_SEL = 3;
defparam rpll_inst.ODIV_SEL = 8;
defparam rpll_inst.DYN_IDIV_SEL = "false";
defparam rpll_inst.DYN_FBDIV_SEL = "false";
defparam rpll_inst.DYN_ODIV_SEL = "false";
defparam rpll_inst.PSDA_SEL = "1000";
defparam rpll_inst.DYN_DA_EN = "false";
defparam rpll_inst.DUTYDA_SEL = "1000";
defparam rpll_inst.CLKOUT_FT_DIR = 1'b1;
defparam rpll_inst.CLKOUTP_FT_DIR = 1'b1;
defparam rpll_inst.CLKOUT_DLY_STEP = 0;
defparam rpll_inst.CLKOUTP_DLY_STEP = 0;
defparam rpll_inst.CLKFB_SEL = "internal";
defparam rpll_inst.CLKOUT_BYPASS = "false";
defparam rpll_inst.CLKOUTP_BYPASS = "false";
defparam rpll_inst.CLKOUTD_BYPASS = "false";
defparam rpll_inst.DYN_SDIV_SEL = 2;
defparam rpll_inst.CLKOUTD_SRC = "CLKOUT";
defparam rpll_inst.CLKOUTD3_SRC = "CLKOUT";
defparam rpll_inst.DEVICE = "GW2AR-18C";
endmodule
