// Selected adjacent-tap profile; no retained successor tiles or same flags.
module membership_leaf #(parameter SHORT_ALIGNMENT=0)(
 input clk,reset,ce,in_valid,
 input[8:0] in_w0,in_w1,in_w2,in_w3,
 input[9:0] in_c0,in_c1,in_c2,in_c3,
 input[3:0] in_slot,in_level,input[5:0] in_key,
 input in_fine,in_last_fine,
 output in_ready,out_valid,output[75:0] out_member,output reg fault
);
 reg[71:0] common0;reg[75:0] member1;
 reg[75:0] tail0,tail1,tail2,tail3,tail4;
 reg[6:0] valid;
 assign in_ready=ce&&!fault;
 assign out_valid=(SHORT_ALIGNMENT?valid[1]:valid[6])&&!fault;
 assign out_member=SHORT_ALIGNMENT?member1:tail4;
 wire sx=(common0[63:60]<=3)||(common0[52:50]!=7);
 wire sy=(common0[63:60]<=3)||(common0[55:53]!=7);
 wire n0=|common0[8:0],n1=|common0[17:9],n2=|common0[26:18],n3=|common0[35:27];
 wire[3:0] emit={n3&&!((n0&&sx&&sy)||(n1&&sy)||(n2&&sx)),
   n2&&!((n0&&sy)||(n1&&sx&&sy)),n1&&!(n0&&sx),n0};
 wire[75:0] result={common0[71:36],emit,common0[35:0]};
 always @(posedge clk) begin
  if(reset)begin valid<=0;fault<=0;end
  else if(ce&&!fault)begin
   if(in_valid&&(in_level>10 || !(|{in_w0,in_w1,in_w2,in_w3})))fault<=1;
   else begin
    valid<={valid[5:0],in_valid};
    if(in_valid)common0<={in_last_fine||!in_fine,in_fine,in_key[1:0],in_key[5:2],in_level,in_slot,in_c2[2:0],in_c0[2:0],in_c2[9:3],in_c0[9:3],in_w3,in_w2,in_w1,in_w0};
    if(valid[0])member1<=result;
    if(!SHORT_ALIGNMENT)begin
     if(valid[1])tail0<=member1;if(valid[2])tail1<=tail0;if(valid[3])tail2<=tail1;
     if(valid[4])tail3<=tail2;if(valid[5])tail4<=tail3;
    end
   end
  end
 end
endmodule
