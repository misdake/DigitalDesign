`timescale 1ns/1ps
module tb;
 reg controller_clk=0,clk=0,phy_clk=0;
 wire sdram_clk=phy_clk;
 always #5.051 controller_clk=!controller_clk;
 always @(posedge controller_clk)clk=!clk;
 initial begin #13.259;forever begin phy_clk=!phy_clk;#5.051;end end
 reg [1:0] buttons=1;
 wire [5:0] leds;
 wire uart_tx;
 wire [63:0] sdram_read_data,sdram_write_data;
 wire sdram_read_valid,sdram_init_done,sdram_request_ready,sdram_done,sdram_write_data_ready;
 wire sdram_request_valid,sdram_write,sdram_write_data_valid;
 wire [20:0] sdram_address;
 wire [3:0] sdram_write_mask;
 wire [5:0] sdram_words;
 wire O_sdram_clk,O_sdram_cke,O_sdram_cs_n,O_sdram_cas_n,O_sdram_ras_n,O_sdram_wen_n;
 wire [3:0] O_sdram_dqm;
 wire [10:0] O_sdram_addr;
 wire [1:0] O_sdram_ba;
 wire [31:0] IO_sdram_dq;
 SdramTrafficProbe dut(.*);
 TangNano20KSdramNativeBridge108M54M bridge(
  .logic_clk(clk),.controller_clk(controller_clk),.sdram_clk(sdram_clk),.reset(|buttons),
  .request_valid(sdram_request_valid),.writing(sdram_write),.address(sdram_address),.words(sdram_words),
  .write_mask(sdram_write_mask),.write_data(sdram_write_data),.write_data_valid(sdram_write_data_valid),
  .request_ready(sdram_request_ready),.write_data_ready(sdram_write_data_ready),.read_data(sdram_read_data),
  .read_valid(sdram_read_valid),.done(sdram_done),.initialized(sdram_init_done),
  .O_sdram_clk(O_sdram_clk),.O_sdram_cke(O_sdram_cke),.O_sdram_cs_n(O_sdram_cs_n),
  .O_sdram_cas_n(O_sdram_cas_n),.O_sdram_ras_n(O_sdram_ras_n),.O_sdram_wen_n(O_sdram_wen_n),
  .O_sdram_dqm(O_sdram_dqm),.O_sdram_addr(O_sdram_addr),.O_sdram_ba(O_sdram_ba),.IO_sdram_dq(IO_sdram_dq)
 );
 LabPinModel #(.PERIOD_NS(10.102),.RETURN_MIN(1),.RETURN_MAX(4)) pin(
  .sclk(O_sdram_clk),.reset(|buttons),.cke(O_sdram_cke),.cs(O_sdram_cs_n),
  .ras(O_sdram_ras_n),.cas(O_sdram_cas_n),.we(O_sdram_wen_n),.dqm(O_sdram_dqm),
  .a(O_sdram_addr),.ba(O_sdram_ba),.dq(IO_sdram_dq),.cycle(),.refreshes()
 );
 integer f,phase,byte_index,b,cycles=0;
 reg [7:0] value;
 task receive_byte;
 begin
  @(negedge uart_tx);
  repeat(dut.UART_DIV+ dut.UART_DIV/2)@(posedge clk);
  for(b=0;b<8;b=b+1)begin value[b]=uart_tx;repeat(dut.UART_DIV)@(posedge clk);end
  if(uart_tx!==1)$fatal(1,"UART stop");
 end endtask
 always @(posedge clk)begin cycles=cycles+1;if(cycles>20000000)$fatal(1,"probe watchdog");end
 initial begin
  // The guard is mapped to logical byte 8192, beyond all test patterns.
  pin.seed_word(21'h200,32'h51a79bc3);
  repeat(4)@(negedge clk);buttons=0;
  f=$fopen("uart.hex","w");
  for(phase=0;phase<9;phase=phase+1)begin
   for(byte_index=0;byte_index<420;byte_index=byte_index+1)begin receive_byte();$fwrite(f,"%02x",value);end
   $fwrite(f,"\n");$fflush(f);
   if(dut.failed)$fatal(1,"probe pattern/timeout failure phase %0d",phase);
  end
  if(pin.physical_word(21'h200)!==32'h51a79bc3)$fatal(1,"image guard changed");
  $fclose(f);$display("PASS nine SDRAM traffic profiles and UART records");$finish;
 end
endmodule
