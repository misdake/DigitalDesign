`timescale 1ns/1ps
module world_matrix_prepare_tb;
@GSR@
reg clk=0,reset=1,ce=1,start_v=0,dma_we=0;
reg[9:0]dma_addr=0;
reg[63:0]dma_data=0;
wire dma_ready;
reg[31:0]view[0:31],world[0:31],gold[0:29];
integer test_id,i,writes,active_edges,wall_edges;
shared_frontend #(.SP_COMPACT(1),.RESULT_COMPACT(1),.SOURCE_READ(1),.WORLD_PREPARE(1)) dut(
 .clk(clk),.reset(reset),.ce(ce),.start_v(start_v),.start_t(1'b0),.meshlet_reset(1'b0),
 .vertex_billboard(test_id>=16),.billboard_vertical(test_id>=24),.vertex_count(7'd1),.uniform_base(6'd20),.config_base(9'd129),
 .world_transform(1'b1),.vertex_base(10'd0),.triangle_descriptor(32'd0),.cull_negative(1'b0),
 .dma_we(dma_we),.dma_addr(dma_addr),.dma_data(dma_data),.dma_ready(dma_ready),
 .output_ready(1'b1),.packet_ready(1'b0),.word_ready(1'b1),.source_re(1'b0),.source_addr(10'd0));
always #5 clk=~clk;
always @(posedge clk)if(!reset&&dut.matrix_write)begin
 if(dma_we)$fatal(1,"matrix store collided with DMA");
 if(dut.matrix_data!==gold[dut.control[67:63]])
  $fatal(1,"matrix case%0d index%0d got%h wanted%h",test_id,dut.control[67:63],dut.matrix_data,gold[dut.control[67:63]]);
 writes=writes+1;
end
task upload(input[9:0]address,input[63:0]value);
begin
 @(negedge clk);dma_addr=address;dma_data=value;dma_we=1;
 @(posedge clk);if(!dma_ready)$fatal(1,"DMA rejected");
 @(negedge clk);dma_we=0;
end
endtask
initial begin
 for(test_id=0;test_id<32;test_id=test_id+1)begin
  reset=1;ce=1;dma_we=0;start_v=0;
  repeat(3)@(negedge clk);reset=0;
  $readmemh($sformatf("view-%0d.hex",test_id),view);
  $readmemh($sformatf("world-%0d.hex",test_id),world);
  $readmemh($sformatf("gold-%0d.hex",test_id),gold);
  for(i=0;i<16;i=i+1)begin
   upload(320+i,{world[2*i+1],world[2*i]});
   upload(368+i,{view[2*i+1],view[2*i]});
  end
  writes=0;active_edges=0;wall_edges=0;
  @(negedge clk);start_v=1;
  @(negedge clk);start_v=0;
  if(!dut.matrix_active)$fatal(1,"matrix entry did not start");
  while(dut.matrix_active)begin
   ce=test_id<2?1:((wall_edges%7)!=2&&(wall_edges%7)!=4);
   dma_we=test_id>=2&&((wall_edges%5)==1||(wall_edges%5)==2);
   dma_addr=128;dma_data=64'h1234;
   @(posedge clk);if(dut.step)active_edges=active_edges+1;
   #1;@(negedge clk);wall_edges=wall_edges+1;
   if(wall_edges>300)$fatal(1,"matrix watchdog");
  end
  dma_we=0;ce=1;
  if(writes!=(test_id>=16?19:24))$fatal(1,"matrix write count%0d",writes);
  if(active_edges!=(test_id>=16?65:97))$fatal(1,"matrix enabled edges%0d",active_edges);
  if(dut.ub!=22)$fatal(1,"effective uniform was not installed");
 end
 $display("PASS World preparation");$finish;
end
initial begin #1000000;$fatal(1,"wall watchdog");end
endmodule
