// Two independent bounds axes share the fixed PA/PB/PC ports and DSP calendar.
// PB0/1 and PC0/1 are temporary axis rows, dead before analytic LOD starts.
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
 reg[5:0]pc;reg axis;reg[1:0]index,d_kind,aux_kind;
 reg sign0,nonzero0,empty;reg[1:0]eligible;
 reg search_main,dual_search,aux_pending;
 reg[2:0]search_bit;reg[4:0]prefix;reg[3:0]aux_prefix;
 wire[33:0]u;wire[5:0]op=u[5:0];reg[5:0]next_pc;
 wire normal_pair=index==0||(index==1&&!axis);
 wire last_pair=index==2,last_n=axis&&index==2;
 wire[1:0]j=index==2?2'd0:index+1'b1;
 wire[1:0]next_index=index==2?2'd0:index+1'b1;
 wire next_axis=index==2?!axis:axis;
 wire dual_math=&eligible;
 function[3:0]weight_row;input a;input[1:0]v;begin
  if(a)weight_row=4'd9+v;else if(v==0)weight_row=0;else weight_row=4'd6+v;
 end endfunction
 function[3:0]geo_row;input a;input[1:0]v;begin geo_row=(a?4'd9:4'd6)+v;end endfunction
 function[1:0]kind;input[53:0]v;begin
  if(v==0)kind=0;else if(v[53])kind=2;else kind=1;
 end endfunction
 wire pair_eligible=!((!nonzero0&&bq==0)||(nonzero0&&bq!=0&&sign0==bq[35]));
 wire endpoint_fail=(d_kind==0||d_kind==2)&&result[53];
 wire endpoint_search=(d_kind==1&&result[53])||(d_kind==2&&!result[53]);
 wire aux_fail=(aux_kind==0||aux_kind==2)&&result[53];
 wire aux_search=(aux_kind==1&&result[53])||(aux_kind==2&&!result[53]);
 wire candidate_accept=d_kind==2?!result[53]:result[53];
 wire aux_accept=aux_kind==2?!result[53]:result[53];
 wire[4:0]trial=prefix|(5'b00001<<search_bit);
 wire[4:0]final_prefix=candidate_accept?trial:prefix;
 wire[3:0]aux_trial=aux_prefix|(4'b0001<<search_bit);
 wire[3:0]aux_final=aux_accept?aux_trial:aux_prefix;
 wire[5:0]lower={1'b0,final_prefix}+6'd1;
 wire lower_empty=axis?lower>{2'd0,yhi}:lower>{1'b0,xhi};
 wire upper_empty=axis?{1'b0,final_prefix}<{2'd0,ylo}:{1'b0,final_prefix}<{1'b0,xlo};
 wire main_empty=d_kind==1?lower_empty:upper_empty;
 wire[4:0]aux_lower={1'b0,aux_final}+5'd1;
 wire aux_empty=aux_kind==1?aux_lower>{1'b0,yhi}:aux_final<ylo;
 wire pair_finish=(op==3&&axis&&eligible==0)||
  (op==16&&!dual_math&&!endpoint_fail&&!endpoint_search)||
  (op==17&&!aux_fail&&!search_main&&!aux_search)||
  (op==20&&search_bit==0&&!empty&&!main_empty);
 always @*begin
  next_pc=u[11:6];
  case(op)
   3:begin if(!axis)next_pc=0;else if(eligible==0)next_pc=21;end
   16:begin
    if(endpoint_fail)next_pc=22;
    else if(!dual_math)begin if(endpoint_search)next_pc=18;else next_pc=21;end
   end
   17:begin
    if(aux_fail)next_pc=22;else if(search_main||aux_search)next_pc=18;else next_pc=21;
   end
   20:if(search_bit!=0)next_pc=18;else if(empty||main_empty)next_pc=22;else next_pc=21;
   21:if(rgb)next_pc=40;
   39:if(last_n)next_pc=40;
   default:begin end
  endcase
  if(pair_finish)next_pc=last_pair?6'd21:6'd0;
 end
 rp_prepare_program program_store(clk,reset,ce,(start||active)&&!reset,start?6'd0:next_pc,u);
 function[6:0]alignment_row;input[1:0]v;begin
  case(v)0:alignment_row={2'b01,shift0};1:alignment_row={2'b01,shift1};default:alignment_row={2'b01,shift2};endcase
 end endfunction
 wire aux_issue=op==5||op==7||op==10||op==12||op==15||op==19;
 wire c_axis=axis^aux_issue;
 wire search_start=(op==16&&!dual_math&&endpoint_search)||(op==17&&(search_main||aux_search));
 wire search_axis=op==17&&!search_main?!axis:axis;
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
   9:ar={3'd0,weight_row(axis,j)};
   10:ar={3'd0,weight_row(axis,index)};
   11:if(dual_math)ar={3'd0,weight_row(axis,j)};else ar={3'd0,weight_row(!axis,index)};
   12:if(d_kind==1)ar=80;else ar=50;
   13:if(aux_kind==1)ar=80;else ar=50;
   default:begin end
  endcase
  // The final attribute slot primes the first arithmetic input directly.
  if(op==3)ar={3'd0,weight_row(eligible[0]?1'b1:1'b0,j)};
  if(op==16||op==17)ar=search_axis?7'd67:7'd68;
  if(op==18)ar={3'b100,aux_pending&&!aux_accept,search_bit-3'd1};
  if(op==20)ar={3'b100,!candidate_accept,search_bit-3'd1};
  case(u[19:16])
   1:br=geo_row(!axis,index);
   2:br=geo_row(!axis,j);
   3:br=geo_row(axis,index);
   4:br=geo_row(axis,j);
   5:br=4'd12+index;
   6:br=4'd12+j;
   7:br=geo_row(!axis,index);
   8:br=geo_row(!axis,j);
   10:br=geo_row(next_axis,next_index);
   11:br=6;
   12:if(op==4)begin
     if(dual_math)br=geo_row(!axis,index);else br=geo_row(axis,j);
    end else if(dual_math)br=4'd12+index;else br=4'd12+j;
   13:br={3'd0,axis};
   14:br={3'd0,!axis};
   default:begin end
  endcase
  if(op==3)br=geo_row(!eligible[0],index);
  if(op==16||op==17)br={3'd0,search_axis};
  case(u[22:20])
   1:cr={3'd0,c_axis};
   2:if(seed_sign[(c_axis?3:0)+index])cr=7;
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
 wire aux_prefetch=op==5||op==6||op==10||op==11||op==14;
 wire search_aux=dual_search&&search_bit!=0;
 wire read_gate=op==3?(axis&&eligible!=0):
  (op==16||op==17)?search_start:op==18?search_aux:op==20?search_bit!=0:!aux_prefetch||dual_math;
 assign a_re=active&&u[24]&&read_gate&&!reset;assign a_ra=ar;
 assign b_re=active&&u[25]&&read_gate&&!reset;assign b_ra=br;
 assign c_re=active&&u[26]&&(!aux_issue||(op==19?search_aux:dual_math))&&!reset;assign c_ra=cr;
 assign attr0_take=active&&op==1&&!reset;
 assign attr1_take=active&&op==2&&!reset;
 assign wide_valid=active&&u[27]&&(!aux_issue||(op==19?search_aux:dual_math))&&!(op==39&&last_n)&&!reset;
 assign wide_asign=u[28];
 wire[1:0]bw=u[30:29];wire[2:0]cw=u[33:31];
 assign b_we=active&&(bw==1||bw==2||(bw==3&&index==2))&&(op!=9||dual_math)&&!reset;
 assign b_wa=op<31?{3'd0,axis^(op==9)}:bw==3?{3'd0,axis}:4'd15;
 assign b_wd=bw==2?{{18{result[34]}},result[34:17]}:result[35:0];
 wire aux_write=op<31&&cw==4;
 assign c_we=active&&cw!=0&&(!aux_write||(op==18?aux_pending:dual_math))&&!reset;
 assign c_wa=aux_write?{3'd0,!axis}:cw==2?{3'd0,axis}:cw==3?{2'd0,1'b1,axis}:op<31?{3'd0,axis}:4'd11;
 assign c_wd=result;
 assign bound_valid=active&&(op==21||op==22)&&!reset;
 assign bound_empty=empty||op==22;
 assign attr_send=active&&u[23]&&!reset;
 assign attr_index=index;assign attr_bank=axis;
 always @(posedge clk)begin
  if(reset)begin active<=0;done<=0;pc<=0;axis<=0;index<=0;d_kind<=0;aux_kind<=0;
   sign0<=0;nonzero0<=0;empty<=0;eligible<=0;search_main<=0;dual_search<=0;aux_pending<=0;
   search_bit<=0;prefix<=0;aux_prefix<=0;xlo<=0;xhi<=24;ylo<=0;yhi<=14;end
  else if(ce)begin
   done<=0;
   if(start)begin active<=1;pc<=0;axis<=0;index<=0;empty<=0;eligible<=0;aux_pending<=0;xlo<=0;xhi<=24;ylo<=0;yhi<=14;end
   else if(active)begin
    pc<=next_pc;
    case(op)
     1:begin sign0<=bq[35];nonzero0<=bq!=0;end
     2:eligible[axis]<=pair_eligible;
     3:begin if(!axis)axis<=1;else axis<=!eligible[0];end
     8:d_kind<=kind(result);
     9:if(dual_math)aux_kind<=kind(result);
     16:search_main<=endpoint_search;
     17:begin dual_search<=search_main&&aux_search;if(!search_main)begin axis<=!axis;d_kind<=aux_kind;end end
     18:if(aux_pending)begin
      aux_prefix<=aux_final;
      if(search_bit==0)begin
       if(aux_kind==1)begin if(aux_lower>{1'b0,ylo})ylo<=aux_lower[3:0];end
       else if(aux_final<yhi)yhi<=aux_final;
       if(aux_empty)empty<=1;
      end
     end
     20:begin
      if(candidate_accept)prefix<=trial;
      aux_pending<=search_aux;
      if(search_bit!=0)search_bit<=search_bit-1'b1;
      else if(d_kind==1)begin
       if(axis)begin if(lower>{2'd0,ylo})ylo<=lower[3:0];end
       else if(lower>{1'b0,xlo})xlo<=lower[4:0];
      end else begin
       if(axis)begin if(final_prefix<{1'b0,yhi})yhi<=final_prefix[3:0];end
       else if(final_prefix<xhi)xhi<=final_prefix;
      end
     end
     22:empty<=1;
     31:begin axis<=0;index<=0;end
     39:begin index<=next_index;axis<=next_axis;end
     40:begin active<=0;done<=1;end
     default:begin end
    endcase
    if(search_start)begin
     search_bit<=search_axis?3'd3:3'd4;prefix<=0;aux_prefix<=0;aux_pending<=0;
     if(op==16)dual_search<=0;
    end
    if(pair_finish)begin axis<=0;index<=index+1'b1;eligible<=0;aux_pending<=0;end
   end
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&active)begin
  if(op!=pc)$fatal(1,"preparation ROM alignment pc%0d op%0d",pc,op);
  if((op==8||(op==9&&dual_math))&&result[53:35]!={19{result[35]}})$fatal(1,"bbox D overflow");
  if((op==37||op==38||op==39)&&result[53:35]!={19{result[35]}})$fatal(1,"G/S overflow");
  if(op==18&&aux_pending&&!dual_search)$fatal(1,"unexpected auxiliary retire");
 end
`endif
endmodule
