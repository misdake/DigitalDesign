// 统一准备执行器：固定PA18/PB36/PC54读口，所有反馈先写回PC。
// bbox和N/S/G共用PC、索引、axis与ROM；没有独立bbox操作数mux或退休队列。
module rp_prepare_core(
 input clk,reset,ce,start,input rgb,input[5:0]seed_sign,
 input[4:0]shift0,shift1,shift2,
 input[35:0]bq,input[53:0]cq,input[53:0]result,
 output a_re,output[6:0]a_ra,
 output b_re,output[3:0]b_ra,output b_we,output[3:0]b_wa,output[35:0]b_wd,
 output c_re,output[3:0]c_ra,output c_we,output[3:0]c_wa,output[53:0]c_wd,
 output wide_valid,wide_asign,output reg active,done,
 output bound_valid,output bound_empty,output reg[4:0]xlo,xhi,output reg[3:0]ylo,yhi,
 output attr_send,output[1:0]attr_index,output attr_bank,
 output attr0_take,attr1_take
);
 reg[5:0]pc;reg axis;reg[1:0]index,d_kind;reg sign0,nonzero0,empty;
 reg[2:0]search_bit;reg[4:0]prefix;reg delta_positive;
 wire[33:0]u;wire[5:0]op=u[5:0];reg[5:0]next_pc;
 wire normal_pair=index==0||(index==1&&!axis);
 wire last_pair=axis&&index==2,last_n=last_pair;
 wire[1:0]j=index==2?2'd0:index+1'b1;
 wire[1:0]next_index=index==2?2'd0:index+1'b1;
 wire next_axis=index==2?!axis:axis;
 function[3:0]weight_row;input a;input[1:0]v;begin
  if(a)weight_row=4'd9+v;else if(v==0)weight_row=0;else weight_row=4'd6+v;
 end endfunction
 function[3:0]geo_row;input a;input[1:0]v;begin geo_row=(a?4'd9:4'd6)+v;end endfunction
 wire candidate_accept=d_kind==2?!result[53]:result[53];
 wire[4:0]trial=prefix|(5'b00001<<search_bit);
 wire[4:0]final_prefix=candidate_accept?trial:prefix;
 wire[5:0]lower={1'b0,final_prefix}+6'd1;
 wire lower_empty=axis?lower>{2'd0,yhi}:lower>{1'b0,xhi};
 wire upper_empty=axis?{1'b0,final_prefix}<{2'd0,ylo}:{1'b0,final_prefix}<{1'b0,xlo};
 always @*begin
  next_pc=u[11:6];
  case(op)
   2:if((!nonzero0&&bq==0)||(nonzero0&&bq!=0&&sign0==bq[35]))next_pc=42;
   13:if(d_kind!=1)next_pc=16;
   16:begin
    if(d_kind==0)begin if(result[53])next_pc=22;else next_pc=20;end
    else if(d_kind==1&&!result[53])next_pc=20;
    else if(d_kind==2&&result[53])next_pc=22;
   end
   19:begin
    if(search_bit!=0)next_pc=17;
    else if((d_kind==1&&lower_empty)||(d_kind==2&&upper_empty))next_pc=22;
   end
   20:if(last_pair)next_pc=21;
   21:if(rgb)next_pc=40;
   39:if(last_n)next_pc=40;
   default:begin end
  endcase
 end
 rp_prepare_program program_store(clk,reset,ce,(start||active)&&!reset,start?6'd0:next_pc,u);
 function[6:0]alignment_row;input[1:0]v;begin
  case(v)0:alignment_row={2'b01,shift0};1:alignment_row={2'b01,shift1};default:alignment_row={2'b01,shift2};endcase
 end endfunction
 wire[2:0]factor_bit=op==16?(axis?3'd3:3'd4):search_bit-1'b1;
 wire factor_positive=op==16||candidate_accept;
 reg[6:0]ar;reg[3:0]br,cr;
 always @*begin
  ar=81;br=15;cr=6;
  case(u[15:12])
   1:ar={3'd0,weight_row(!axis,j)};
   2:ar={3'd0,weight_row(!axis,index)};
   3:ar=alignment_row(index);
   4:ar=7'd1+index;
   5:ar=7'd4+index;
   6:ar=alignment_row(next_index);
   7:ar=7'd1+next_index;
   8:ar=alignment_row(0);
   default:begin end
  endcase
  if(op==13)ar=80;
  if(op==16||op==19)ar={3'b100,!factor_positive,factor_bit};
  case(u[19:16])
   1:br=geo_row(!axis,index);
   2:br=geo_row(!axis,j);
   3:br=geo_row(axis,index);
   4:br=geo_row(axis,j);
   5:br=4'd12+index;
   6:br=4'd12+j;
   10:br=geo_row(next_axis,next_index);
   11:br=6;
   default:begin end
  endcase
  case(u[22:20])
   1:cr=11;
   2:if(seed_sign[(axis?3:0)+index])cr=7;
   3:if(index!=0)cr={3'd0,axis};
   4:if(index!=0)cr={2'd0,1'b1,axis};
   5:if(index!=0)cr=11;
   6:if(seed_sign[(next_axis?3:0)+next_index])cr=7;
   7:begin
    if(op==0)cr={1'b1,normal_pair,2'b10};
    else if(op==1)cr={1'b1,normal_pair,2'b00};
    else cr={1'b1,normal_pair,2'b01};
   end
   default:begin end
  endcase
 end
 // op19 used to update held async Q by writing PA15. Read the next
 // immutable factor instead, on the same enabled edge and only if consumed.
 assign a_re=active&&(u[24]||(op==19&&search_bit!=0))&&!reset;assign a_ra=ar;
 assign b_re=active&&u[25]&&!reset;assign b_ra=br;
 assign c_re=active&&u[26]&&!reset;assign c_ra=cr;
 assign attr0_take=active&&op==1&&!reset;
 assign attr1_take=active&&op==2&&!reset;
 assign wide_valid=active&&u[27]&&!(op==39&&last_n)&&!reset;
 assign wide_asign=u[28];
 wire[1:0]bw=u[30:29];wire[2:0]cw=u[33:31];
 assign b_we=active&&(bw==1||bw==2||(bw==3&&index==2))&&!reset;
 assign b_wa=bw==3?{3'd0,axis}:4'd15;
 assign b_wd=bw==2?{{18{result[34]}},result[34:17]}:result[35:0];
 assign c_we=active&&cw!=0&&!reset;
 assign c_wa=cw==2?{3'd0,axis}:cw==3?{2'd0,1'b1,axis}:4'd11;
 assign c_wd=result;
 assign bound_valid=active&&(op==21||op==22)&&!reset;
 assign bound_empty=empty||op==22;
 // C地址最高位在非C读指令中作为属性发布标志；42复用20的全部循环控制。
 assign attr_send=active&&u[23]&&!reset;
 assign attr_index=index;assign attr_bank=axis;
 always @(posedge clk)begin
  if(reset)begin active<=0;done<=0;pc<=0;axis<=0;index<=0;d_kind<=0;sign0<=0;nonzero0<=0;empty<=0;
   search_bit<=0;prefix<=0;delta_positive<=1;xlo<=0;xhi<=24;ylo<=0;yhi<=14;end
  else if(ce)begin
   done<=0;
   if(start)begin active<=1;pc<=0;axis<=0;index<=0;empty<=0;xlo<=0;xhi<=24;ylo<=0;yhi<=14;end
   else if(active)begin
    pc<=next_pc;
    case(op)
     1:begin sign0<=bq[35];nonzero0<=bq!=0;end
     8:begin if(result==0)d_kind<=0;else if(result[53])d_kind<=2;else d_kind<=1;end
     16:begin search_bit<=axis?3'd3:3'd4;prefix<=0;delta_positive<=1;end
     19:begin
      if(candidate_accept)prefix<=trial;delta_positive<=candidate_accept;
      if(search_bit!=0)search_bit<=search_bit-1'b1;
      else if(d_kind==1)begin
       if(axis)begin if(lower>{2'd0,ylo})ylo<=lower[3:0];end
       else if(lower>{1'b0,xlo})xlo<=lower[4:0];
      end else begin
       if(axis)begin if(final_prefix<{1'b0,yhi})yhi<=final_prefix[3:0];end
       else if(final_prefix<xhi)xhi<=final_prefix;
      end
     end
     20:begin axis<=!axis;if(axis)begin if(index==2)index<=0;else index<=index+1'b1;end end
     22:empty<=1;
     31:begin axis<=0;index<=0;end
     39:begin index<=next_index;axis<=next_axis;end
     40:begin active<=0;done<=1;end
     default:begin end
    endcase
   end
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&active)begin
  if(op!=pc&&!(pc==42&&op==20))$fatal(1,"统一准备ROM错位 pc%0d op%0d",pc,op);
  if(op==8&&result[53:35]!={19{result[35]}})$fatal(1,"bbox D越界");
  if((op==37||op==38||op==39)&&result[53:35]!={19{result[35]}})$fatal(1,"G/S越界");
 end
`endif
endmodule
