// Single- and dual-axis bounds share fixed PA/PB/PC ports and one DSP calendar.
// PB0/1 and PC0/1 are temporary axis rows, dead before analytic LOD starts.
module rp_prepare_core(
 input clk,reset,ce,start,input input_take,normalize_wait,
 output[5:0]operation,output[3:0]import_row,output[2:0]import_kind,input rgb,input[5:0]seed_sign,
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
 reg axis;reg[1:0]index,d_kind,aux_kind;
 reg sign0,nonzero0,empty;reg[1:0]eligible;
 reg search_main,dual_search,aux_pending;
 reg[2:0]search_bit;reg[4:0]prefix;reg[3:0]aux_prefix;
 wire[35:0]u;wire[5:0]op=u[5:0];wire[5:0]pc=op;reg[5:0]next_pc;
 assign operation=op;assign import_row=u[24:21];assign import_kind=u[14:12];
 wire last_pair=index==2,last_n=axis&&index==2;
 wire[1:0]next_index=index==2?2'd0:index+1'b1;
 wire next_axis=index==2?!axis:axis;
 wire dual_math=&eligible;
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
  if(op>=48)next_pc=input_take?u[11:6]:op;
  case(op)
   23:next_pc=normalize_wait?6'd23:6'd24;
   3:begin if(!axis)next_pc=0;else if(eligible==0)next_pc=21;else if(!dual_math)next_pc=41;end
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
 reg fetch_axis;reg[1:0]fetch_index;
 always @*begin
  fetch_axis=axis;fetch_index=index;
  case(op)
   3:if(!axis)fetch_axis=1;else fetch_axis=!eligible[0];
   17:if(!search_main)fetch_axis=!axis;
   31:begin fetch_axis=0;fetch_index=0;end
   39:begin fetch_axis=next_axis;fetch_index=next_index;end
   default:begin end
  endcase
  if(pair_finish)begin fetch_axis=0;fetch_index=index+1'b1;end
  // op3 only prefetches arithmetic for the selected primary. Its actual
  // attribute bank remains the sequential axis, outside this ROM address.
  if(next_pc==3)fetch_axis=eligible[0];
 end
 rp_prepare_program program_store(clk,reset,ce,(start||active)&&!reset,
  start?9'd48:{fetch_index,fetch_axis,next_pc},u);
 wire aux_issue=op==5||op==7||op==10||op==12||op==15||op==19;
 wire c_axis=axis^aux_issue;
 wire search_start=(op==16&&!dual_math&&endpoint_search)||(op==17&&(search_main||aux_search));
 wire search_axis=op==17&&!search_main?!axis:axis;
 // A previous edge pair has already bounded the live tile interval. Searching
 // above its high endpoint cannot change that intersection. A dual search
 // reserves one more primary bit because Y launches one bit behind X.
 function[2:0]high_bit;input[4:0]v;begin
  if(v[4])high_bit=4;else if(v[3])high_bit=3;else if(v[2])high_bit=2;
  else if(v[1])high_bit=1;else high_bit=0;
 end endfunction
 wire[2:0]x_search_bit=high_bit(xhi),y_search_bit=high_bit({1'b0,yhi});
 wire[2:0]main_search_bit=search_axis?y_search_bit:x_search_bit;
 wire[2:0]paired_search_bit=y_search_bit+3'd1;
 wire[2:0]initial_search_bit=(op==17&&search_main&&aux_search&&paired_search_bit>main_search_bit)?paired_search_bit:main_search_bit;
 wire[4:0]a_code=u[16:12];wire[3:0]c_code=u[24:21];
 reg[3:0]port_enable;
 always @*case(u[27:25])
  0:port_enable=0;1:port_enable=2;2:port_enable=3;3:port_enable=4;
  4:port_enable=6;5:port_enable=12;6:port_enable=13;7:port_enable=15;
 endcase
 reg[6:0]ar;reg[3:0]br,cr;
 always @*begin
  ar={2'd0,a_code};br=u[20:17];cr=c_code;
  if(a_code>=16&&a_code<=18)begin
   case(a_code[1:0])
    0:ar={2'b01,shift0};1:ar={2'b01,shift1};default:ar={2'b01,shift2};
   endcase
  end
  case(a_code)
   19:ar={4'b1000,initial_search_bit};
   20:ar={3'b100,aux_pending&&!aux_accept,search_bit-3'd1};
   21:ar={3'b100,!candidate_accept,search_bit-3'd1};
   22:if(d_kind==1)ar=80;else ar=50;
   23:if(aux_kind==1)ar=80;else ar=50;
   24:ar=80;25:ar=81;
   default:begin end
  endcase
  if(op==17&&!search_main)br={3'd0,!axis};
  if(c_code==5)cr={3'b011,seed_sign[(c_axis?3:0)+index]};
  if(c_code==15)cr={3'b011,seed_sign[(next_axis?3:0)+next_index]};
 end
 wire aux_prefetch=op==5||op==6||op==10||op==11||op==14;
 wire search_aux=dual_search&&search_bit!=0;
 wire read_gate=op==3?(axis&&eligible!=0):
  (op==16||op==17)?search_start:op==18?search_aux:op==20?search_bit!=0:!aux_prefetch||dual_math;
 assign a_re=active&&port_enable[0]&&read_gate&&!reset;assign a_ra=ar;
 assign b_re=active&&port_enable[1]&&read_gate&&!reset;assign b_ra=br;
 assign c_re=active&&port_enable[2]&&(!aux_issue||(op==19?search_aux:dual_math))&&!reset;assign c_ra=cr;
 assign attr0_take=active&&op==1&&!reset;
 assign attr1_take=active&&op==2&&!reset;
 assign wide_valid=active&&port_enable[3]&&(!aux_issue||(op==19?search_aux:dual_math))&&!(op==39&&last_n)&&!reset;
 assign wide_asign=u[28];
 wire[1:0]b_dest=u[31:30];wire[2:0]c_dest=u[35:33];
 assign b_we=active&&b_dest!=0&&(op!=9||dual_math)&&!reset;
 assign b_wa=b_dest==3?4'd15:{3'd0,b_dest[1]};
 assign b_wd=b_dest==3?{{18{result[34]}},result[34:17]}:result[35:0];
 assign c_we=active&&c_dest!=0&&(c_dest<6||(op==18?aux_pending:dual_math))&&!reset;
 reg[3:0]c_row;
 always @*case(c_dest)
  1,6:c_row=0;2,7:c_row=1;3:c_row=2;4:c_row=3;default:c_row=11;
 endcase
 assign c_wa=c_row;
 assign c_wd=result;
 assign bound_valid=active&&(op==21||op==22)&&!reset;
 assign bound_empty=empty||op==22;
 assign attr_send=active&&u[29]&&!reset;
 assign attr_index=index;assign attr_bank=axis;
 always @(posedge clk)begin
  if(reset)begin active<=0;done<=0;axis<=0;index<=0;d_kind<=0;aux_kind<=0;
   sign0<=0;nonzero0<=0;empty<=0;eligible<=0;search_main<=0;dual_search<=0;aux_pending<=0;
   search_bit<=0;prefix<=0;aux_prefix<=0;xlo<=0;xhi<=24;ylo<=0;yhi<=14;end
  else if(ce)begin
   done<=0;
   if(start)begin active<=1;axis<=0;index<=0;empty<=0;eligible<=0;aux_pending<=0;xlo<=0;xhi<=24;ylo<=0;yhi<=14;end
   else if(active)begin
    case(op)
     1:begin sign0<=bq[35];nonzero0<=bq!=0;end
     2:eligible[axis]<=pair_eligible;
     3:begin if(!axis)axis<=1;else axis<=!eligible[0];end
     8,45:d_kind<=kind(result);
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
     24:begin active<=0;done<=1;end
     default:begin end
    endcase
    if(search_start)begin
     search_bit<=initial_search_bit;prefix<=0;aux_prefix<=0;aux_pending<=0;
     if(op==16)dual_search<=0;
    end
    if(pair_finish)begin axis<=0;index<=index+1'b1;eligible<=0;aux_pending<=0;end
   end
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&active)begin
  if((op==8||op==45||(op==9&&dual_math))&&result[53:35]!={19{result[35]}})$fatal(1,"bbox D overflow");
  if((op==37||op==38||op==39)&&result[53:35]!={19{result[35]}})$fatal(1,"G/S overflow");
  if(op==18&&aux_pending&&!dual_search)$fatal(1,"unexpected auxiliary retire");
 end
`endif
endmodule
