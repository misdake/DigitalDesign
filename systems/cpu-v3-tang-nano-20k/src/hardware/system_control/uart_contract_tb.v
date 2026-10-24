// Independent clock-by-clock 8N1 oracle, including terminal-edge writes.
module tb;
localparam integer BIT_CYCLES=__CLOCKS_PER_BIT__;
localparam integer BYTE_COUNT=BIT_CYCLES<=469 ? 256 : 3;
reg clk=0,reset=1;
always #5 clk=~clk;
reg device_write_enable=0;
reg [15:0] device_write_data=0;
wire uart_tx;
wire [15:0] device_read_data;
__MODULE__ dut(.clk(clk),.reset(reset),.device_index(3'd0),.device_channel(4'd3),
 .device_read_enable(1'b1),.device_write_enable(device_write_enable),
 .device_write_data(device_write_data),.dcache_maintenance_ready(1'b1),
 .dcache_maintenance_done(1'b0),.dcache_maintenance_error(1'b0),
 .watch_read_data(16'd0),.device_read_data(device_read_data),.uart_tx(uart_tx));
integer n,elapsed,bit_index;
reg [7:0] value;
reg expected_tx;
initial begin
 repeat((BYTE_COUNT+3)*(10*BIT_CYCLES+3)+100) @(posedge clk);
 $fatal(1,"bounded UART contract period=%0d",BIT_CYCLES);
end
task check_idle;
 begin
  if(uart_tx!==1'b1 || device_read_data!==16'd0)$fatal(1,"UART idle");
 end
endtask
task launch;
 input [7:0] data;
 begin
  @(negedge clk);device_write_enable=1;device_write_data={8'b0,data};
  @(posedge clk);#1;
  if(uart_tx!==1'b0 || device_read_data!==16'd1)$fatal(1,"UART start");
 end
endtask
initial begin
 @(posedge clk);#1;check_idle;
 @(negedge clk);reset=0;
 for(n=0;n<BYTE_COUNT;n=n+1)begin
  value=BIT_CYCLES<=469 ? n : n==0 ? 8'h00 : n==1 ? 8'hff : 8'ha5;
  launch(value);
  for(elapsed=1;elapsed<=10*BIT_CYCLES;elapsed=elapsed+1)begin
   @(negedge clk);
   // Both writes must be dropped, including the edge that clears busy.
   device_write_enable=elapsed==BIT_CYCLES+1 || elapsed==10*BIT_CYCLES;
   device_write_data={8'b0,~value};
   @(posedge clk);#1;
   bit_index=elapsed/BIT_CYCLES;
   if(bit_index==0)expected_tx=0;
   else if(bit_index<=8)expected_tx=value[bit_index-1];
   else expected_tx=1;
   if(uart_tx!==expected_tx || device_read_data!==(elapsed<10*BIT_CYCLES ? 16'd1 : 16'd0))
    $fatal(1,"UART byte=%h clock=%0d period=%0d tx=%b busy=%h",value,elapsed,BIT_CYCLES,uart_tx,device_read_data);
  end
  @(negedge clk);device_write_enable=0;
  @(posedge clk);#1;check_idle;
 end
 launch(8'h5a);
 @(negedge clk);device_write_enable=0;
 repeat(BIT_CYCLES+1)@(posedge clk);
 @(negedge clk);reset=1;
 @(posedge clk);#1;check_idle;
 @(negedge clk);reset=0;
 @(posedge clk);#1;check_idle;
 $display("UART_CONTRACT_PASS period=%0d bytes=%0d",BIT_CYCLES,BYTE_COUNT);$finish;
end
endmodule
