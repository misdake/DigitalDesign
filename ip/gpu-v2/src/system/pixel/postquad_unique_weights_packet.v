// Private Group40 encoder. Generic Member76/Group72 packet_leaf remains unchanged.
module packet_leaf_unique #(parameter SHORT_ALIGNMENT=1)(
 input clk,reset,ce,in_valid,input[75:0] in_member,input[1:0] in_tap,
 output in_ready,out_valid,output[39:0] out_packet,output reg fault
);
 reg[40:0]capture0;reg[39:0]word0;reg[1:0]valid;
 assign in_ready=ce&&!fault;assign out_valid=valid[1]&&!fault;assign out_packet=word0;
 wire x=capture0[39],y=capture0[40];wire[3:0]level=capture0[30:27];
 wire sx=(level<=3)||(capture0[19:17]!=7),sy=(level<=3)||(capture0[22:20]!=7);
 wire[6:0]tile_mask=(level<=3)?7'd0:(7'd127>>(4'd10-level));
 wire[6:0]tx=(capture0[9:3]+{6'd0,(x&&!sx)})&tile_mask;
 wire[6:0]ty=(capture0[16:10]+{6'd0,(y&&!sy)})&tile_mask;
 wire[3:0]masks={(x||sx)&&(y||sy),(!x||sx)&&(y||sy),(x||sx)&&(!y||sy),(!x||sx)&&(!y||sy)};
 wire higher=(!x&&!y)?(|capture0[2:0]):(x&&!y)?(|capture0[2:1]):(!x&&y)?capture0[2]:1'b0;
 wire first=capture0[37]&&!x&&!y,last=capture0[38]&&!higher;
 wire[39:0]result={masks,capture0[36:35],capture0[34:31],last,first,
 capture0[22:20],capture0[19:17],ty,tx,level,capture0[26:23]};
 wire[3:0]active=in_member[39:36];
 always@(posedge clk)begin
  if(reset)begin valid<=0;fault<=0;end
  else if(ce&&!fault)begin
   if(in_valid&&!active[in_tap])fault<=1;
   else begin
    valid<={valid[0],in_valid};
    if(in_valid)capture0<={in_tap,in_member[75:37]};
    if(valid[0])word0<=result;
   end
  end
 end
endmodule
