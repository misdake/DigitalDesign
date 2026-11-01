// Immutable-command six-row owner24/result36 queue.
module gpu_v2_direct_coordinate_store #(parameter CAP=6, NATIVE=@NATIVE@)(
 input wire clk,reset,proposal_ce,commit_allow,
 input wire issue,returned,take,
 input wire [37:0] owner_data,
 input wire [35:0] result_data,
 output wire in_ready,out_valid,bad,
 output wire [37:0] out_owner,
 output wire [35:0] out_result,
 output wire owner_write,result_write,consume,
 output wire [3:0] write_row,return_row,read_row,
 output wire [4:0] occupied,completed);
 wire [23:0] canonical_in={owner_data[36:31],owner_data[30:27],owner_data[22:19],owner_data[18],owner_data[17:9]};
 wire [23:0] canonical_out;
 wire [3:0] n=canonical_out[13:10];
 wire [3:0] n1=(n==0)?4'd0:n-4'd1;
 assign out_owner={canonical_out[8:0]==0,canonical_out[23:18],canonical_out[17:14],n1,n,canonical_out[9],canonical_out[8:0],~canonical_out[8:0]};
 wire owner_bad=issue && ((owner_data[8:0]!=~owner_data[17:9]) || owner_data[22:19]>10 ||
   owner_data[26:23]!=((owner_data[22:19]==0)?4'd0:owner_data[22:19]-4'd1) ||
   owner_data[37]!=(owner_data[17:9]==0));
 wire ce=proposal_ce&&commit_allow;
 localparam A=CAP>1?$clog2(CAP):1;
 localparam Q=$clog2(CAP+1);
 reg[A-1:0] wp,rp,cp;
 reg[Q-1:0] count,ready;
 assign in_ready=!reset && count<CAP;
 assign out_valid=!reset && ready!=0;
 assign bad=1'b0;
 assign owner_write=!reset && ce && !bad && issue;
 assign result_write=!reset && ce && !bad && returned;
 assign consume=!reset && ce && !bad && take;
 assign write_row=wp;assign return_row=rp;assign read_row=cp;
 assign occupied=count;assign completed=ready;
 generate if(NATIVE) begin:sram
   wire[23:0] padded_owner=canonical_in;
   wire[23:0] owner_read;
   wire[3:0] wa=wp,ra=rp,ca=cp;
   genvar i;
   for(i=0;i<6;i=i+1) begin:owner_bank
     RAM16SDP4 ram(.CLK(clk),.WRE(owner_write),.WAD(wa),.RAD(ca),
       .DI(padded_owner[i*4+:4]),.DO(owner_read[i*4+:4]));
   end
   for(i=0;i<9;i=i+1) begin:result_bank
     RAM16SDP4 ram(.CLK(clk),.WRE(result_write),.WAD(ra),.RAD(ca),
       .DI(result_data[i*4+:4]),.DO(out_result[i*4+:4]));
   end
   assign canonical_out=owner_read;
 end else begin:ff
   (* syn_ramstyle="registers" *) reg[23:0] owners[0:CAP-1];
   (* syn_ramstyle="registers" *) reg[35:0] results[0:CAP-1];
   genvar i;
   for(i=0;i<CAP;i=i+1) begin:row
     always @(posedge clk) begin
       if(owner_write && wp==i) owners[i]<=canonical_in;
       if(result_write && rp==i) results[i]<=result_data;
     end
   end
   assign canonical_out=owners[cp];assign out_result=results[cp];
 end endgenerate
 always @(posedge clk) begin
   if(reset) begin wp<=0;rp<=0;cp<=0;count<=0;ready<=0;end
   else if(ce && !bad) begin
     if(issue) begin if(wp==CAP-1) wp<=0;else wp<=wp+1'b1;end
     if(returned) begin if(rp==CAP-1) rp<=0;else rp<=rp+1'b1;end
     if(take) begin if(cp==CAP-1) cp<=0;else cp<=cp+1'b1;end
     case({issue,take}) 2'b10:count<=count+1'b1;2'b01:count<=count-1'b1;default:count<=count;endcase
     case({returned,take}) 2'b10:ready<=ready+1'b1;2'b01:ready<=ready-1'b1;default:ready<=ready;endcase
   end
 end

`ifndef SYNTHESIS
 always @(posedge clk)if(!reset&&((1'b1)&&(!reset && proposal_ce && ((issue && count==CAP) || (returned && count==ready) || (take && ready==0)))))$fatal(1,"GPU contract: bad");
`endif
endmodule
