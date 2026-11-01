`timescale 1ns/1ps
module tb;
 parameter NATIVE=1;
 localparam COUNT=192;
 reg clk=0,reset=1,ce=0,commit=1,offer=0,ready=0;
 always #5 clk=~clk;
 GSR GSR(.GSRI(1'b1));
 reg[161:0]inputs[0:COUNT-1];reg[189:0]gold[0:COUNT-1];
 reg[161:0]payload=0;reg[7:0]available=16;
 wire in_ready,accepted,out_valid,fault,proposal,bad,ww;
 wire[71:0]masks;wire[35:0]meta,wv;wire[6:0]wa;wire[1:0]cost;
 gpu_v2_runtime_coefficient_directweight dut(
 .clk(clk),.reset(reset),.proposal_ce(ce),.commit_allow(commit),.in_valid(offer),
 .in_payload(payload),.work_available(available),.in_ready(in_ready),.in_accept(accepted),
 .out_ready(ready),.out_valid(out_valid),.out_weights(masks),.out_meta(meta),
 .work_reserved(cost),.fault(fault),.proposal_ready(proposal),.preflight_bad(bad),
 .direct_weight_write(ww),.direct_weight_address(wa),.direct_weight_value(wv));
 gpu_v2_texture_runtime_cache_unique #(.NATIVE(NATIVE)) cache(
 .clk(clk),.reset(reset),.proposal_ce(1'b1),.commit_allow(1'b1),.abort(1'b0),
 .in_valid(1'b0),.in_payload(35'd0),.texture_invalidate(1'b0),
 .weight_write(ww),.weight_address(wa),.weight_value(wv),.next_valid(1'b0),.next_key(15'd0),
 .out_ready(1'b1),.mem_req_ready(1'b1),.mem_resp_valid(1'b0),.mem_resp_index(4'd0),
 .mem_resp_data(64'd0),.mem_resp_complete(1'b0),.mem_resp_ok(1'b1));
 wire bank_write;wire[6:0]bank_address;wire[35:0]bank_value;
 generate if(NATIVE)begin
 assign bank_write=cache.weights_native.bank.CEA;
 assign bank_address=cache.weights_native.bank.ADA[11:5];
 assign bank_value=cache.weights_native.bank.DI;
 end else begin
 assign bank_write=cache.weight_write;assign bank_address=cache.weight_address;
 assign bank_value=cache.weight_value;
 end endgenerate
 integer wall=0,sent=0,published=0,consumed=0,cohorts=0,phase=0,writes=0;
 integer age[0:COUNT-1];integer memory_index[0:127];reg[35:0]shadow[0:127];reg[127:0]dirty=0;
 integer i,ecost,queued,expect_advance,expect_ready,expect_write;
 integer fine=0,coarse=0,zeros=0,paused=0,blocked=0,full=0,wraps=0,overlap=0,old_full_pop=0,peak=0;
 integer capture_index;reg[6:0]expected_address;reg[35:0]expected_value;
 reg[148:0]held_arithmetic;
 wire[148:0]arithmetic_q={dut.input_stage,dut.horizontal_delay,dut.row_totals,dut.ph,
 dut.valid,dut.weight_key0,dut.weight_key1,dut.product[0],dut.product[1],dut.product[2],dut.fine_mask};
 reg[1023:0]input_file,gold_file;integer collision;
 initial begin
 if(!$value$plusargs("INPUT=%s",input_file)||!$value$plusargs("GOLD=%s",gold_file))$fatal(1,"input files");
 collision=$test$plusargs("COLLISION");
 $readmemh(input_file,inputs);$readmemh(gold_file,gold);
 for(i=0;i<COUNT;i=i+1)age[i]=0;
 repeat(3)@(negedge clk);reset=0;
 end
 always@(negedge clk)if(!reset)begin
 wall=wall+1;if(wall>12000)$fatal(1,"bounded direct-weight watchdog");
 ce=wall%13<10;commit=wall%17!=5&&wall%17!=6;
 ready=wall%101>=28&&wall%7!=1;
 available=wall%29<3?0:(wall%29<6?1:16);
 offer=sent<COUNT&&wall%7!=3;payload=sent<COUNT?inputs[sent]:162'd0;
 if(collision&&wall==8)begin
  force dut.meta_we=1;force dut.meta_re=1;force dut.cwr=0;force dut.meta_ra=0;
 end
 if(collision&&wall==9)begin
  release dut.meta_we;release dut.meta_re;release dut.cwr;release dut.meta_ra;
  if(fault!==0||bad!==0)$fatal(1,"collision diagnostic affected hardware");
  $display("PASS collision monitor-off fault/preflight stay zero");$finish;
 end
 end
 always@(posedge clk)if(!reset&&!collision)begin
 queued=published-consumed;
 ecost=offer?gold[sent][189:188]:0;
 expect_advance=ce&&commit&&queued<2;
 expect_ready=queued<2&&cohorts<3&&!phase&&available>=ecost;
 if(dut.advance!==expect_advance[0]||proposal!==expect_ready[0]||
    in_ready!==(expect_advance&&expect_ready)||accepted!==(offer&&expect_advance&&expect_ready))
  $fatal(1,"admission/old-credit wall=%0d",wall);
 if(out_valid!==(queued!=0)||fault!==0||bad!==0)$fatal(1,"publication/fault wall=%0d",wall);
 if(cost!==(accepted?gold[sent][189:188]:2'd0))$fatal(1,"reservation cost");
 if(out_valid)begin
  if(masks!==gold[consumed][179:108]||meta!==gold[consumed][107:72])$fatal(1,"ready masks/metadata wall=%0d cohort=%0d",wall,consumed);
 end
 expect_write=0;expected_address=0;expected_value=0;
 for(i=0;i<sent;i=i+1)if(expect_advance&&(age[i]==3||(age[i]==4&&gold[i][187])))begin
  expect_write=expect_write+1;expected_address=gold[i][186:180]^(age[i]==4?7'd1:7'd0);
  expected_value=age[i]==3?gold[i][35:0]:gold[i][71:36];
  if(age[i]==3)fine=fine+1;else coarse=coarse+1;
 end
 if(expect_write>1||ww!==(expect_write!=0)||bank_write!==(expect_write!=0))$fatal(1,"sole writer/count wall=%0d expected=%0d core=%b bank=%b",wall,expect_write,ww,bank_write);
 if(expect_write)begin
  if(wa!==expected_address||bank_address!==expected_address||wv!==expected_value||bank_value!==expected_value)
   $fatal(1,"weight value/address wall=%0d got=%h/%h expected=%h/%h",wall,wa,wv,expected_address,expected_value);
  writes=writes+1;if(expected_value==0)zeros=zeros+1;
  $display("W %0d %0d %h %h",wall,writes,wa,wv);
  shadow[expected_address]=expected_value;dirty[expected_address]=1;
 end
 if(ce&&commit&&out_valid&&ready)begin
  $display("O %0d %0d %h %h",wall,consumed,masks,meta);
  if(queued==2)old_full_pop=old_full_pop+1;
  consumed=consumed+1;
 end
 for(i=0;i<sent;i=i+1)if(expect_advance&&age[i]>0)begin
  if(age[i]==5)begin published=published+1;cohorts=cohorts-1;end
  age[i]=age[i]+1;
 end
 if(accepted)begin
  if(expect_write&&dut.v4)overlap=overlap+1;
  $display("A %0d %0d %h",wall,sent,payload);
  age[sent]=1;sent=sent+1;cohorts=cohorts+1;
  if(sent%16==0)wraps=wraps+1;
 end
 if(cohorts>peak)peak=cohorts;
 if(expect_advance)phase=phase^1;
 else paused=paused+1;
 if(offer&&!in_ready)blocked=blocked+1;if(queued==2)full=full+1;
 held_arithmetic=arithmetic_q;
 end
 // Observe actual bank storage after nonblocking writes, without forcing its read path.
 generate if(NATIVE)begin:check_bank
 task check_image;integer j;begin
  for(j=0;j<128;j=j+1)if(dirty[j]&&cache.weights_native.bank.ram_MEM[memory_index[j]+:36]!==shadow[j])$fatal(1,"final physical bank image");
 end endtask
 always@(posedge clk)if(!reset&&!collision)begin
  if(bank_write)capture_index=cache.weights_native.bank.addr_a;
  #1;
  if(expect_write)begin
   memory_index[expected_address]=capture_index;
   if(cache.weights_native.bank.ram_MEM[capture_index+:36]!==expected_value)$fatal(1,"physical bank write");
  end
 end
 end else begin:check_bank
 task check_image;integer j;begin
  for(j=0;j<128;j=j+1)if(dirty[j]&&cache.weights_portable.mem[j]!==shadow[j])$fatal(1,"final portable bank image");
 end endtask
 always@(posedge clk)if(!reset&&!collision)begin
  #1;if(expect_write&&cache.weights_portable.mem[expected_address]!==expected_value)$fatal(1,"portable bank write");
 end
 end endgenerate
 always@(posedge clk)if(!reset&&!collision)begin
 #2;if(!expect_advance&&arithmetic_q!==held_arithmetic)$fatal(1,"arithmetic/key changed while held");
 if(sent==COUNT&&consumed==COUNT)begin
  check_bank.check_image();
  if(fine!=COUNT||coarse==0||zeros==0||paused==0||blocked==0||full==0||wraps<4||overlap==0||old_full_pop==0||peak!=3)
   $fatal(1,"coverage F%0d C%0d Z%0d pause%0d block%0d full%0d wrap%0d overlap%0d fullpop%0d peak%0d",fine,coarse,zeros,paused,blocked,full,wraps,overlap,old_full_pop,peak);
  $display("PASS directweight cycles=%0d inputs=%0d writes=%0d fine=%0d coarse=%0d zero=%0d overlap=%0d fullpop=%0d",wall,sent,writes,fine,coarse,zeros,overlap,old_full_pop);$finish;
 end
 end
endmodule
