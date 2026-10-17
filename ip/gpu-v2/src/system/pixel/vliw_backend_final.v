// Two reserved Final result slots. Arithmetic/rounding and DSP sharing unchanged.
module gpu_v2_final_ordered_two_cycle(
 input wire shade_phase,input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,
 input wire shared_rop_offer,input wire[50:0]shared_spec_product,output wire shared_spec_ce,output wire[8:0]shared_spec_a,output wire[23:0]shared_spec_b,
 input wire [23:0] in_tint,in_tex,in_spec,
 input wire [8:0] in_g,in_h,
 output wire in_ready,proposal_ready,in_accept,advance,preflight_bad,fault,
 input wire out_ready,
 output wire out_valid,out_fire,output wire [23:0] out_rgb,
 output wire [3:0] credits,queued
);
 localparam IDLE=2'd0,SPEC=2'd1,BASE=2'd2;
 reg stopped;reg[1:0]q_count;reg q_head,q_tail;
 reg arithmetic_live;
 wire [1:0] phase=!arithmetic_live?IDLE:(shade_phase?SPEC:BASE);
 reg[23:0]result0,result1;
 // g/h/spec are the parent's immutable live loan.
 wire running=ce&&!reset&&!fault;
 // Occupied capacity is the old queued results plus the one arithmetic
 // token. An output ACK never supplies same-edge admission credit.
 wire[2:0]reserved_total={1'b0,q_count}+{2'd0,arithmetic_live};
 wire old_slot_free=(q_count==0)||(!arithmetic_live&&q_count==1);
 wire base_valid=(phase==BASE);
 wire credit_bad=(q_count==3)||(arithmetic_live&&q_count==2);
 assign fault=stopped||abort;
 assign proposal_ready=!shade_phase&&!reset&&!fault&&old_slot_free;
 assign in_ready=running&&proposal_ready;
 assign preflight_bad=proposal_ce&&!reset&&!fault&&(credit_bad||(proposal_ready&&in_valid&&(in_h>9'd256)));
 assign advance=running&&commit_allow&&!preflight_bad;
 assign in_accept=advance&&in_valid&&proposal_ready;
 assign out_valid=!reset&&!fault&&(q_count!=0||base_valid);
 assign out_fire=advance&&out_ready&&out_valid;
 wire[23:0]color;
 wire[23:0]head_rgb=q_head?result1:result0;
 assign out_rgb=q_count!=0?head_rgb:color;
 assign credits={1'b0,reserved_total};
 assign queued={2'd0,q_count}+{3'd0,base_valid};
 wire queue_pop=out_fire&&(q_count!=0);
 // With no queued result, BASE may be consumed directly without any FIFO write.
 // Otherwise the older head always wins and the just-computed BASE appends.
 wire queue_push=advance&&base_valid&&!(q_count==0&&out_fire);
 assign shared_spec_ce=advance&&in_accept;assign shared_spec_a=in_h;assign shared_spec_b=in_spec;
 wire mul_ce=advance&&(in_accept||(phase==SPEC));

 genvar c;
 generate for(c=0;c<3;c=c+1)begin:channels
  reg [8:0] a;
  reg [7:0] b;
  wire [16:0] product;

  reg [7:0] sh;
  wire [15:0] plus128=product[15:0]+16'd128;
  wire [15:0] folded=plus128+{8'd0,plus128[15:8]};
  wire [9:0] sum={1'b0,product[16:8]}+{2'b0,sh};
  wire saturated=(|sum[9:8]);
  wire [7:0] rounded=sum[7:0];
  assign color[c*8+:8]=saturated?8'd255:rounded;
  always @*begin
   a={1'b0,in_tint[c*8+:8]};b=in_tex[c*8+:8];
   case(phase)
    SPEC:begin a=in_g;b=folded[15:8];end

   endcase
  end
  gpu_v2_final_two_cycle_mul9 multiplier(.clk(clk),.ce(mul_ce),.a(a),.b(b),.product(product));
  wire [16:0] spec_product;
  assign spec_product=shared_spec_product[c*17+:17];
  always @(posedge clk)if(advance)begin

   if(phase==SPEC)sh<=spec_product[15:8];
  end
 end endgenerate
 always @(posedge clk)begin
  if(reset)begin stopped<=0;arithmetic_live<=0;q_count<=0;q_head<=0;q_tail<=0;end
  else begin
   if(abort||preflight_bad)stopped<=1;
   if(advance)begin
    if(!shade_phase)arithmetic_live<=in_accept;
    if(queue_push)begin
     if(q_tail)result1<=color;else result0<=color;
     q_tail<=!q_tail;
    end
    if(queue_pop)q_head<=!q_head;
    case({queue_push,queue_pop})
     2'b10:q_count<=q_count+1'b1;
     2'b01:q_count<=q_count-1'b1;
    endcase
   end
  end
 end
`ifndef SYNTHESIS
 reg check_reservation;reg[2:0]expected_reservation;
 always @(posedge clk)begin
  if(reset)begin check_reservation<=0;expected_reservation<=0;end
  else begin
   if(check_reservation&&reserved_total!==expected_reservation)$fatal(1,"Final reservation conservation");
   check_reservation<=1;
   expected_reservation<=reserved_total+{2'd0,in_accept}-{2'd0,out_fire};
  end
 end
 always @(posedge clk)if(!reset)begin
  if(reserved_total>2)$fatal(1,"Final old reservation exceeds two slots");
  if(base_valid&&q_count==2)$fatal(1,"Final BASE has no reserved result slot");
  if(in_accept&&!old_slot_free)$fatal(1,"Final borrowed output ACK credit");
  if(queue_push&&q_count==2)$fatal(1,"Final result queue overflow");
  if(queue_pop&&q_count==0)$fatal(1,"Final result queue underflow");
 end
`endif
endmodule
