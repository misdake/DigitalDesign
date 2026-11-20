`timescale 1ns/1ps
module tb;
 parameter CASE=1,TRUSTED=0;
`ifndef PORTABLE
 GSR GSR(.GSRI(1'b1));
`endif
 reg clk=0;always #5 clk=~clk;
 reg reset=1,ce=1,start=0;
 integer cycles=0;
 always @(negedge clk)begin
  cycles=cycles+1;
  if(cycles>12000)$fatal(1,"control watchdog");
 end
 task tick;begin @(posedge clk);#1;end endtask
 task boot;begin repeat(3)tick;reset=0;repeat(2)tick;end endtask
 generate if(CASE<10)begin:front
  reg start_v=0,start_t=0,dma_we=0;
  reg[6:0]vertex_count=1;
  reg[5:0]uniform_base=0;
  reg[9:0]dma_addr=0;
  wire[2:0]error;wire busy,done_v,done_t;
  shared_frontend #(.RESULT_COMPACT(1),.SP_COMPACT(1),.SOURCE_READ(1)) dut(
`ifdef GPU_V2_COMMAND_PORTS
   .mesh_color(32'hffffffff),
`endif
   .clk(clk),.reset(reset),.ce(ce),.start_v(start_v),.start_t(start_t),.meshlet_reset(1'b0),
   .vertex_billboard(1'b0),.vertex_count(vertex_count),.uniform_base(uniform_base),.config_base(9'd0),
   .vertex_base(10'd0),.triangle_descriptor(32'd0),.cull_negative(1'b0),
   .dma_we(dma_we),.dma_addr(dma_addr),.dma_data(64'd0),.output_ready(1'b1),
   .packet_ready(1'b1),.word_ready(1'b1),.source_re(1'b0),.source_addr(10'd0),
   .busy(busy),.done_v(done_v),.done_t(done_t),.error(error));
  always @(negedge clk)if(!reset)$display("TRACE F %0d %b %b %b %b %h %h %h",cycles,ce,busy,done_v,done_t,error,dut.pc,dut.published_vertices);
  initial begin
   boot;
   case(CASE)
    1:start_t=1;
    2:begin vertex_count=65;start_v=1;end
    3:begin uniform_base=32;start_v=1;end
    4:begin dma_we=1;dma_addr=512;end
   endcase
   ce=0;repeat(2)tick;
   if(error!==0)$fatal(1,"frontend accepted an event while CE=0");
   ce=1;tick;start_v=0;start_t=0;dma_we=0;
   if(TRUSTED)begin
    if(error!==0)$fatal(1,"trusted frontend latched generic error");
   end else begin
    case(CASE)
     1:if(error!==2||!done_t)$fatal(1,"reference rejection changed");
     2:if(error!==3||!done_v)$fatal(1,"capacity rejection changed");
     3:if(error!==4||!done_v)$fatal(1,"compact rejection changed");
     4:if(error!==4)$fatal(1,"DMA rejection changed");
    endcase
   end
   $display("PASS frontend case %0d",CASE);$finish;
  end
 end else if(CASE<20)begin:prepare
  reg in_valid=0;reg[4:0]in_index=0;
  wire in_ready,busy;wire[3:0]error;
  integer wait_cycles;
  rp_prepare dut(.clk(clk),.reset(reset),.ce(ce),.start(start),.execute_grant(1'b1),
   .descriptor(32'd0),.operand_value(18'd0),.cfg_far_z(32'h7fffffff),
   .cfg_tex_u(5'd0),.cfg_tex_v(5'd0),.cfg_max_mip(5'd0),.cfg_bias(8'd0),
   .in_valid(in_valid),.in_ready(in_ready),.in_index(in_index),.in_data(54'd1),
   .pw_q(90'd0),.wide_result_data(54'd0),.busy(busy),.error(error));
  always @(negedge clk)if(!reset)$display("TRACE P %0d %b %b %b %h %h %b",cycles,ce,busy,in_ready,error,dut.operation,dut.core.ce);
  initial begin
   boot;start=1;tick;start=0;
   wait_cycles=0;
   while(in_ready!==1&&wait_cycles<40)begin tick;wait_cycles=wait_cycles+1;end
   if(!busy||!in_ready)$fatal(1,"prepare did not reach import");
   ce=0;
   if(CASE==10)start=1;
   else begin in_valid=1;in_index=dut.expected+1'b1;end
   repeat(2)tick;
   if(error!==0)$fatal(1,"prepare accepted an event while CE=0");
   ce=1;tick;start=0;in_valid=0;
   if(TRUSTED)begin
    if(error!==0||!in_ready||dut.core.ce!==1)$fatal(1,"trusted prepare changed specialized continuation");
   end else begin
    if(error!==(CASE==10?4'd8:4'd1))$fatal(1,"prepare error latch changed");
    if(in_ready!==0)$fatal(1,"prepare ready ignored sticky error");
    if(dut.core.ce!==0)$fatal(1,"prepare core CE ignored sticky error");
    repeat(3)tick;
    if(in_ready!==0||dut.core.ce!==0)$fatal(1,"prepare sticky stop released");
   end
   $display("PASS prepare case %0d",CASE);$finish;
  end
 end else begin:meshlet
  reg dma_we=0;reg[9:0]dma_addr=0;reg[63:0]dma_data=0;
  wire ready,busy,done,dma_ready;wire[3:0]error;
  integer f,rc,a,n;reg[63:0]value;
  shared_meshlet dut(
`ifdef GPU_V2_COMMAND_PORTS
   .mesh_color(32'hffffffff),.cp_re(1'b0),.cp_addr(9'd0),
`endif
   .clk(clk),.reset(reset),.ce(ce),.start(start),.slot(1'b0),
   .dma_we(dma_we),.dma_addr(dma_addr),.dma_data(dma_data),.output_ready(1'b1),
   .packet_ready(1'b1),.word_ready(1'b1),.ready(ready),.busy(busy),.done(done),
   .dma_ready(dma_ready),.error(error));
  always @(negedge clk)if(!reset)$display("TRACE M %0d %b %b %b %h %h %h %h",cycles,ce,ready,done,error,dut.state,dut.index_cursor,dut.published_vertices);
  task put;input integer addr;input[63:0]data;begin
   n=0;while(!dma_ready&&n<40)begin tick;n=n+1;end
   if(!dma_ready)$fatal(1,"meshlet DMA stalled");
   dma_we=1;dma_addr=addr;dma_data=data;tick;dma_we=0;
  end endtask
  initial begin
   boot;
   if(CASE==20)begin put(256,64'h00140041);put(257,0);put(259,0);end
   else begin
    f=$fopen("dma.txt","r");if(!f)$fatal(1,"missing real meshlet DMA");
    while(!$feof(f))begin rc=$fscanf(f,"%d %h",a,value);if(rc==2)put(a,value);end
    $fclose(f);
   end
   n=0;while(!ready&&n<40)begin tick;n=n+1;end
   if(!ready)$fatal(1,"meshlet never ready");
   start=1;tick;start=0;
   if(CASE==20)begin
    while(dut.state!=7)tick;
   end else begin
    while(!(dut.state==11&&dut.index_cursor[7]))begin
     tick;if(error)$fatal(1,"meshlet failed before cursor overflow");
    end
   end
   ce=0;repeat(2)tick;
   if(error!==0)$fatal(1,"meshlet accepted an event while CE=0");
   ce=1;tick;
   if(TRUSTED)begin if(error!==0)$fatal(1,"trusted meshlet latched error");end
   else if(error!==(CASE==20?4'd5:4'd6)||!done)$fatal(1,"meshlet rejection changed");
   $display("PASS meshlet case %0d",CASE);$finish;
  end
 end endgenerate
endmodule
