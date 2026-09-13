module TangNano20KVideoPll(input wire clkin,output wire serial_clock,output wire pixel_clock,output wire locked);
// 800x480@60 (H 1056 / V 525), 33.3 MHz pixel clock.
//
// Gowin rPLL static-parameter encoding (UG286): IDIV=IDIV_SEL+1,
// FBDIV=FBDIV_SEL+1 (both 1..64), CLKOUT=CLKIN*FBDIV/IDIV, VCO=CLKOUT*ODIV.
// ODIV does not divide CLKOUT; it only sets the VCO. CLKDIV DIV_MODE "5"
// divides by five, and the GW2AR-18C OSER10 uses FCLK = 5*PCLK (DDR).
//
//   IDIV_SEL=5 -> IDIV=6    PFD = 27/6 = 4.5 MHz   (>= 3)
//   FBDIV_SEL=36 -> FBDIV=37
//   CLKOUT  = 27*37/6       = 166.5 MHz  -> OSER10 FCLK
//   ODIV_SEL=4   -> VCO     = 666 MHz    (within 500..1250)
//   CLKDIV /5    -> PCLK    = 33.3 MHz   -> 60.065 Hz at 1056x525
wire unused_p,unused_d,unused_d3;
rPLL p(.CLKOUT(serial_clock),.LOCK(locked),.CLKOUTP(unused_p),.CLKOUTD(unused_d),.CLKOUTD3(unused_d3),
 .RESET(1'b0),.RESET_P(1'b0),.CLKIN(clkin),.CLKFB(1'b0),.FBDSEL(6'b0),.IDSEL(6'b0),.ODSEL(6'b0),
 .PSDA(4'b0),.DUTYDA(4'b0),.FDLY(4'b0));
defparam p.FCLKIN="27"; defparam p.IDIV_SEL=5; defparam p.FBDIV_SEL=36; defparam p.ODIV_SEL=4;
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