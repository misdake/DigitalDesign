module TangNano20KVideoPll720p(input wire clkin,output wire serial_clock,output wire pixel_clock,output wire locked);
wire unused_p,unused_d,unused_d3;
rPLL p(.CLKOUT(serial_clock),.LOCK(locked),.CLKOUTP(unused_p),.CLKOUTD(unused_d),.CLKOUTD3(unused_d3),
 .RESET(1'b0),.RESET_P(1'b0),.CLKIN(clkin),.CLKFB(1'b0),.FBDSEL(6'b0),.IDSEL(6'b0),.ODSEL(6'b0),
 .PSDA(4'b0),.DUTYDA(4'b0),.FDLY(4'b0));
defparam p.FCLKIN="27"; defparam p.IDIV_SEL=3; defparam p.FBDIV_SEL=54; defparam p.ODIV_SEL=2;
defparam p.DYN_IDIV_SEL="false"; defparam p.DYN_FBDIV_SEL="false"; defparam p.DYN_ODIV_SEL="false";
defparam p.PSDA_SEL="0000"; defparam p.DYN_DA_EN="true"; defparam p.DUTYDA_SEL="1000";
defparam p.CLKOUT_FT_DIR=1'b1; defparam p.CLKOUTP_FT_DIR=1'b1;
defparam p.CLKOUT_DLY_STEP=0; defparam p.CLKOUTP_DLY_STEP=0;
defparam p.CLKFB_SEL="internal"; defparam p.CLKOUT_BYPASS="false";
defparam p.CLKOUTP_BYPASS="false"; defparam p.CLKOUTD_BYPASS="false";
defparam p.DYN_SDIV_SEL=2; defparam p.CLKOUTD_SRC="CLKOUT"; defparam p.CLKOUTD3_SRC="CLKOUT";
defparam p.DEVICE="GW2AR-18C";
CLKDIV d(.RESETN(locked),.HCLKIN(serial_clock),.CLKOUT(pixel_clock),.CALIB(1'b1));
defparam d.DIV_MODE="5"; defparam d.GSREN="false";
endmodule
