// Two enabled edges, II1. External16x16 tiles; physical256B half-tile Group35.
// Each mip owns complete16x16 tiles; small mips repeat periodically in padding.
module packet_leaf_unique #(parameter SHORT_ALIGNMENT=1)(
 input clk,reset,ce,in_valid,input[75:0]in_member,input[1:0]in_tap,
 input[15:0]texture_base,input texture_mip,
 output in_ready,out_valid,output[34:0]out_packet,output reg fault
);
 reg[40:0]capture0;reg[14:0]level_base;reg[34:0]word0;reg[1:0]valid;
 assign in_ready=ce&&!fault;assign out_valid=valid[1]&&!fault;assign out_packet=word0;
 function[14:0]layer_half_offset;
  input[3:0]n;
  begin case(n)
   0:layer_half_offset=0;1:layer_half_offset=2;2:layer_half_offset=4;3:layer_half_offset=6;
   4:layer_half_offset=8;5:layer_half_offset=10;6:layer_half_offset=18;7:layer_half_offset=50;
   8:layer_half_offset=178;9:layer_half_offset=690;10:layer_half_offset=2738;
   default:layer_half_offset=0;
  endcase end
 endfunction
 wire[14:0]prefix=texture_mip?layer_half_offset(in_member[67:64]):15'd0;
 wire x=capture0[39],y=capture0[40];wire[3:0]level=capture0[30:27];
 wire sx=(level<=4)||(capture0[19:16]!=15),sy=(level<=3)||(capture0[22:20]!=7);
 wire[5:0]xmask=(level<=4)?6'd0:(6'd63>>(4'd10-level));
 wire[6:0]ymask=(level<=3)?7'd0:(7'd127>>(4'd10-level));
 wire[5:0]tx=(capture0[8:3]+{5'd0,(x&&!sx)})&xmask;
 wire[6:0]ty=(capture0[15:9]+{6'd0,(y&&!sy)})&ymask;
 wire[11:0]macro_row={6'd0,ty[6:1]}<<((level<4)?4'd0:(level-4'd4));
 wire[11:0]macro_tile=macro_row|{6'd0,tx};
 wire[14:0]half_line=level_base+{2'd0,macro_tile,ty[0]};
 wire[3:0]masks={(x||sx)&&(y||sy),(!x||sx)&&(y||sy),(x||sx)&&(!y||sy),(!x||sx)&&(!y||sy)};
 wire higher=(!x&&!y)?(|capture0[2:0]):(x&&!y)?(|capture0[2:1]):(!x&&y)?capture0[2]:1'b0;
 wire first=capture0[37]&&!x&&!y,last=capture0[38]&&!higher;
 // Physical256B line15, localXY7, owner8, mip parity1, coverage4.
 wire[34:0]result={masks,level[0],capture0[36:35],capture0[34:31],last,first,
  capture0[22:20],capture0[19:16],half_line};
 wire[3:0]active=in_member[39:36];
 always@(posedge clk)begin
  if(reset)begin valid<=0;fault<=0;end
  else if(ce&&!fault)begin
   if(in_valid&&!active[in_tap])fault<=1;
   else begin
    valid<={valid[0],in_valid};
    if(in_valid)begin capture0<={in_tap,in_member[75:37]};level_base<=texture_base[15:1]+prefix;end
    if(valid[0])word0<=result;
   end
  end
 end
`ifndef SYNTHESIS
 always@(posedge clk)if(!reset&&ce&&in_valid&&texture_base[0])$fatal(1,"Texture16x8 base must be256B aligned");
`endif
endmodule
