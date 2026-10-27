`timescale 1ns/1ps
// Portable static-input subset of the GW2A IODELAY simulation contract.
// The vendor functional model uses 25 ps per tap (0.8 ns for the fitted 32).
// This is not routed SDF/PVT qualification. Dynamic tap control is unsupported.
`ifdef STREAM_STATIC_INPUT_DELAY
module IODELAY #(parameter C_STATIC_DLY=32)(
 input DI,SDTAP,SETN,VALUE,output DO,DF
);
 assign #(0.025*C_STATIC_DLY) DO=DI;
 assign DF=(SETN&&C_STATIC_DLY==0)||(!SETN&&C_STATIC_DLY==127);
 initial begin
  if(C_STATIC_DLY<0||C_STATIC_DLY>127)$fatal(1,"IODELAY static tap range");
 end
 always @(SDTAP)if(SDTAP)$fatal(1,"dynamic IODELAY is outside the stream fixture contract");
endmodule
`endif
