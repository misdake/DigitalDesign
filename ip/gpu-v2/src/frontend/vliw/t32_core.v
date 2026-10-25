// Bounded-F16 T calendar shares V's RF/MAC fields and physical arithmetic.
// The only T-specific arithmetic is a shared exact 4/1-bit normalization step
// and a narrow toward-zero correction. There is no instruction transaction FSM.
module shared_t32_core #(parameter CROSS_BITS=27)(input clk,reset,ce,start,active,input[8:0]pc,input[71:0]ins,
 input cull,input[17:0]refs,input[35:0]pq,work_q,input[31:0]rf_b,unpacked,
 input[53:0]acc,input[53:0]mac_y,
 output reg[8:0]next_pc,output pos_re,output[8:0]pos_addr,
 output[3:0]rf_aa,rf_ba,output rf_we,output[3:0]rf_wa,output reg[31:0]rf_wd,
 output work_re,work_we,output[3:0]work_ra,work_wa,output[35:0]work_wd,
 output mac_en,output mac_constant,mac_clear,acc_capture,output[31:0]mac_a,output reg[35:0]mac_b,output[53:0]mac_c,
 output acc_we,output reg[53:0]acc_wd,
 output coefficient_we,output[3:0]coefficient_row,output reg[53:0]coefficient_data,
 output publish,done,output reg negative);
 wire run=active&&!reset;
 wire[2:0]norm=ins[8:6],flow=ins[42:40];
 wire[1:0]bsrc=ins[4:3],write_source=ins[30:29];
 reg any_front,sticky,align_negative;reg[2:0]cross_groups;wire[4:0]cross_q={cross_groups,2'b00};reg[4:0]qmax;reg[6:0]det_exp;
 // A signed value fits iff the retained sign prefix is all zeros or
 // all ones. Explicit common prefixes avoid arithmetic comparator chains.
 wire any34=|acc[53:34],all34=&acc[53:34];
 wire any31=any34||(|acc[33:31]),all31=all34&&(&acc[33:31]);
 wire any29=any31||(|acc[30:29]),all29=all31&&(&acc[30:29]);
 wire any28=any29||acc[28],all28=all29&&acc[28];
 wire any27=any28||acc[27],all27=all28&&acc[27];
 wire any26=any27||acc[26],all26=all27&&acc[26];
 wire cross_more=any26&&!all26;
 wire det_more=(any31&&!all31)||(acc[31:0]==32'h80000000);
 wire cross_coarse=any29&&!all29;
 wire norm_more=norm==1?cross_coarse:norm==3?det_more:norm==4?!acc[30]:1'b0;
 wire det_coarse=any34&&!all34;
 wire coarse=norm==1?1'b1:norm==3?det_coarse:(acc[30:27]==0);
 wire[2:0]distance=coarse?3'd4:3'd1;
 // Keep three guard bits in the existing work row. The final exact shift
 // is performed by the already scheduled common-exponent DSP alignment.
 wire[1:0]cross_tail=(any28&&!all28)?2'd3:(any27&&!all27)?2'd2:cross_more?2'd1:2'd0;
 wire[4:0]cross_final_q={cross_groups,cross_tail};
 wire magnitude=norm==3&&!det_more;
 // One exponent incrementer serves initial bias, right and left normalization.
 // All constants are seven-bit modular increments, including -1 and -4.
 wire[6:0]exp_base=ins[34]?{2'd0,qmax}:det_exp;
 wire[6:0]exp_step=ins[34]?7'd30:(norm==4?(coarse?7'd124:7'd127):(coarse?7'd4:7'd1));
 wire[6:0]exp_next=exp_base+exp_step;
 wire[31:0]corrected=(acc[31:0]^{32{magnitude&&acc[53]}})+{31'd0,(acc[53]&&(magnitude?!sticky:sticky))};
 wire check_reject=flow==5?((acc==0)||(acc[53]&&cull)):!(any_front||(rf_we&&rf_wa==9&&!unpacked[31]&&(|unpacked[30:0])));
 assign rf_aa=ins[24:21];
 assign rf_ba=ins[28:25];assign rf_wa=ins[20:17];assign rf_we=run&&write_source!=0;
 always @*begin
  rf_wd=unpacked;
  if(ins[2])rf_wd=-$signed(unpacked);
  if(write_source==3)case(ins[16:15])
   0:rf_wd=-32'sd399;1:rf_wd=-32'sd239;2:rf_wd=32'd512;
   default:if(ins[2])rf_wd=-32'sd1;else rf_wd=32'd1;
  endcase
 end
 reg[5:0]vertex_ref;
 always @*case(ins[10:9])0:vertex_ref=refs[5:0];1:vertex_ref=refs[11:6];default:vertex_ref=refs[17:12];endcase
 assign pos_re=run&&ins[59];assign pos_addr={1'b0,ins[5:4],vertex_ref};
 assign work_re=run&&ins[53];assign work_ra=ins[58:55];assign work_wa={1'b0,ins[62:60]};
 assign work_we=run&&ins[54]&&(!ins[68]||!norm_more);
 assign work_wd=ins[70]?{6'd0,{3{mac_y[52]}},mac_y[52:26]}:{norm==1?cross_q:5'd0,1'b0,corrected[29:0]};
 wire[2:0]align_groups=qmax[4:2]-work_q[35:33];
 wire[4:0]align_distance={align_groups,qmax[1:0]};
 assign mac_en=run&&ins[31]&&(!ins[68]||!norm_more);assign mac_constant=run&&ins[69];
 assign mac_a=32'h04000000>>align_distance;
 assign mac_c=ins[70]?{28'd0,{26{align_negative}}}:mac_clear?54'd0:acc;
 assign mac_clear=ins[33:32]==1;assign acc_capture=run&&ins[33:32]!=0&&(!ins[68]||!norm_more);
 wire[35:0]rf_word={{9{rf_b[26]}},rf_b[26:0]};
 always @*begin
  if(bsrc[0])mac_b=rf_word;
  else mac_b={{6{work_q[29]}},work_q[29:0]};
 end
 assign acc_we=run&&(((norm!=0)&&norm_more)||magnitude);
 always @*begin
  acc_wd=acc;
  if(magnitude)acc_wd={22'd0,corrected};
  else if(norm==4)begin
   if(coarse)acc_wd={23'd0,acc[26:0],4'd0};else acc_wd={23'd0,acc[29:0],1'b0};
  end else begin
   if(coarse)acc_wd={{4{acc[53]}},acc[53:4]};else acc_wd={acc[53],acc[53:1]};
  end
  next_pc=pc+1'b1;
  if(norm!=0&&norm_more)next_pc=pc;
 end
 assign coefficient_we=run&&ins[67];assign coefficient_row=ins[66:63];
 always @*case(ins[37:35])
  1:coefficient_data={1'b0,qmax,12'd0,{(36-CROSS_BITS){work_q[CROSS_BITS-1]}},work_q[CROSS_BITS-1:0]};
  2:coefficient_data={{(54-CROSS_BITS){work_q[CROSS_BITS-1]}},work_q[CROSS_BITS-1:0]};
  3:coefficient_data={16'd0,det_exp,acc[30:0]};
  default:coefficient_data={{16{mac_y[37]}},mac_y[37:0]};
 endcase
 assign publish=run&&flow==6;
 assign done=run&&(flow==6||((flow==2||flow==5)&&check_reject));
 always @(posedge clk)begin
  if(reset)begin any_front<=0;negative<=0;sticky<=0;cross_groups<=0;align_negative<=0;qmax<=0;det_exp<=0;end
  else if(ce)begin
   if(start)begin any_front<=0;negative<=0;cross_groups<=0;qmax<=0;end
   else if(run)begin
    if(rf_we&&(rf_wa==3||rf_wa==6||rf_wa==9))any_front<=any_front||(!unpacked[31]&&(|unpacked[30:0]));
    if(ins[9]&&norm==0)sticky<=0;
    if(ins[11])cross_groups<=0;
    if(ins[14])qmax<=0;
    if(ins[69])align_negative<=work_q[29];
    if(flow==5)negative<=acc[53];
    if(ins[34]||(norm_more&&(norm==3||norm==4)))det_exp<=exp_next;
    if(norm!=0)begin
     if(norm_more)begin
      if(norm!=4)begin
       if(coarse)sticky<=sticky||(|acc[3:0]);else sticky<=sticky||acc[0];
       case(norm)
        1:cross_groups<=cross_groups+1'b1;
        default:begin end
       endcase
      end
     end else if(norm==1&&cross_final_q>qmax)qmax<=cross_final_q;
    end
   end
  end
 end
`ifdef __ICARUS__
 always @(posedge clk)if(ce&&rf_we&&write_source==1&&unpacked[31:25]!={7{unpacked[25]}})
  $fatal(1,"triangle position exceeds bounded F16 range");
`endif
endmodule
