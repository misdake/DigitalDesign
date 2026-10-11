// Selected Member76 capture77; canonical Group72 remains unchanged.
module packet_leaf #(parameter SHORT_ALIGNMENT=0)(
 input clk,reset,ce,in_valid,input[75:0] in_member,input[1:0] in_tap,
 output in_ready,out_valid,output[71:0] out_packet,output reg fault
);
 reg[76:0] capture0;
 reg[71:0] word0,word1,word2,word3,word4,word5,word6,word7;
 reg[8:0] valid;
 assign in_ready=ce&&!fault;
 assign out_valid=(SHORT_ALIGNMENT?valid[1]:valid[8])&&!fault;
 assign out_packet=SHORT_ALIGNMENT?word0:word7;
 wire x=capture0[75],y=capture0[76];wire[3:0] level=capture0[66:63];
 wire sx=(level<=3)||(capture0[55:53]!=7),sy=(level<=3)||(capture0[58:56]!=7);
 wire[6:0] tile_mask=(level<=3)?7'd0:(7'd127>>(4'd10-level));
 wire[6:0] tx=(capture0[45:39]+{6'd0,(x&&!sx)})&tile_mask;
 wire[6:0] ty=(capture0[52:46]+{6'd0,(y&&!sy)})&tile_mask;
 wire[3:0] masks={ (x||sx)&&(y||sy),(!x||sx)&&(y||sy),(x||sx)&&(!y||sy),(!x||sx)&&(!y||sy)};
 wire higher=(!x&&!y)?(|capture0[38:36]):(x&&!y)?(|capture0[38:37]):(!x&&y)?capture0[38]:1'b0;
 wire first=capture0[73]&&!x&&!y,last=capture0[74]&&!higher;
 wire[71:0] result={capture0[72:71],capture0[70:67],last,first,
   masks[3]?capture0[35:27]:9'd0,masks[2]?capture0[26:18]:9'd0,
   masks[1]?capture0[17:9]:9'd0,masks[0]?capture0[8:0]:9'd0,
   capture0[58:56],capture0[55:53],ty,tx,level,capture0[62:59]};
 wire[3:0] active=in_member[39:36];
 always @(posedge clk)begin
  if(reset)begin valid<=0;fault<=0;end
  else if(ce&&!fault)begin
   if(in_valid&&!active[in_tap])fault<=1;
   else begin
    valid<={valid[7:0],in_valid};
    if(in_valid)capture0<={in_tap,in_member[75:37],in_member[35:0]};
    if(valid[0])word0<=result;
    if(!SHORT_ALIGNMENT)begin
     if(valid[1])word1<=word0;if(valid[2])word2<=word1;if(valid[3])word3<=word2;
     if(valid[4])word4<=word3;if(valid[5])word5<=word4;if(valid[6])word6<=word5;if(valid[7])word7<=word6;
    end
   end
  end
 end
endmodule
