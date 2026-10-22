// Two enabled edges, II1. Form the final physical sector before queue storage.
// The first edge preadds the immutable base/prefix while capturing the member;
// the second adds the selected tile offset and writes one 35-bit group.
module packet_leaf_unique #(parameter SHORT_ALIGNMENT=1)(
 input clk,reset,ce,in_valid,input[75:0]in_member,input[1:0]in_tap,
 input[15:0]texture_base,input texture_mip,
 output in_ready,out_valid,output[34:0]out_packet,output reg fault
);
 reg[40:0]capture0;reg[15:0]level_base;reg[34:0]word0;reg[1:0]valid;
 assign in_ready=ce&&!fault;assign out_valid=valid[1]&&!fault;assign out_packet=word0;
 function[15:0]layer_offset;
  input[3:0]n;
  begin case(n)
   0:layer_offset=0;1:layer_offset=1;2:layer_offset=2;3:layer_offset=3;
   4:layer_offset=4;5:layer_offset=8;6:layer_offset=24;7:layer_offset=88;
   8:layer_offset=344;9:layer_offset=1368;10:layer_offset=5464;
   default:layer_offset=0;
  endcase end
 endfunction
 wire[15:0]prefix=texture_mip?layer_offset(in_member[67:64]):16'd0;
 wire x=capture0[39],y=capture0[40];wire[3:0]level=capture0[30:27];
 wire sx=(level<=3)||(capture0[19:17]!=7),sy=(level<=3)||(capture0[22:20]!=7);
 wire[6:0]tile_mask=(level<=3)?7'd0:(7'd127>>(4'd10-level));
 wire[6:0]tx=(capture0[9:3]+{6'd0,(x&&!sx)})&tile_mask;
 wire[6:0]ty=(capture0[16:10]+{6'd0,(y&&!sy)})&tile_mask;
 wire[13:0]tile_row={7'd0,ty}<<((level<3)?4'd0:(level-4'd3));
 wire[13:0]tile_offset=tile_row|{7'd0,tx};
 wire[15:0]sector=level_base+{2'd0,tile_offset};
 wire[3:0]masks={(x||sx)&&(y||sy),(!x||sx)&&(y||sy),(x||sx)&&(!y||sy),(!x||sx)&&(!y||sy)};
 wire higher=(!x&&!y)?(|capture0[2:0]):(x&&!y)?(|capture0[2:1]):(!x&&y)?capture0[2]:1'b0;
 wire first=capture0[37]&&!x&&!y,last=capture0[38]&&!higher;
 // Physical sector16, local XY6, owner8, weight-bank parity1, coverage4.
 wire[34:0]result={masks,level[0],capture0[36:35],capture0[34:31],last,first,
  capture0[22:20],capture0[19:17],sector};
 wire[3:0]active=in_member[39:36];
 always@(posedge clk)begin
  if(reset)begin valid<=0;fault<=0;end
  else if(ce&&!fault)begin
   if(in_valid&&!active[in_tap])fault<=1;
   else begin
    valid<={valid[0],in_valid};
    if(in_valid)begin capture0<={in_tap,in_member[75:37]};level_base<=texture_base+prefix;end
    if(valid[0])word0<=result;
   end
  end
 end
endmodule
