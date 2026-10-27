`timescale 1ns/1ps
// CL3 108/54 MHz functional pin model; physical IO qualification is separate.
module tb;
GSR GSR(.GSRI(1'b1));
localparam [15:0] PROBE_HALT=__PROBE_HALT__;
reg clk=0,source_clk=0,phy_clk=0,pixel_clock=0,serial_clock=0;
wire controller_clk;assign #(2.271) controller_clk=source_clk;
always #4.629629 source_clk=!source_clk;
always @(posedge controller_clk)clk=!clk;
initial begin #12.430556;forever begin phy_clk=!phy_clk;#4.629629;end end
always #15.015 pixel_clock=!pixel_clock;
always #3.003 serial_clock=!serial_clock;
reg reset=1;
initial begin repeat(8)@(negedge controller_clk);reset=0;end
reg [1:0]buttons=0;
reg flash_miso=1;
wire [1:0]leds;wire uart_tx,flash_clk,flash_cs_n,flash_mosi;
wire ds2_dat=1'b1;
wire ds2_clk,ds2_cmd,ds2_att;
wire tmds_clk_p,tmds_clk_n;wire [2:0]tmds_data_p,tmds_data_n;
wire video_locked=1,sdram_clock_ready=1;
wire [63:0]sdram_read_data,sdram_write_data;
wire sdram_read_valid,sdram_init_done,sdram_request_ready,sdram_stream_active;
wire sdram_write_done,sdram_write_data_ready,sdram_request_valid,sdram_next_valid;
wire sdram_request_continuation;
wire [20:0]sdram_next_address,sdram_address;
wire sdram_write,sdram_write_data_valid;wire [5:0]sdram_words;
wire [3:0]sdram_write_mask;
CpuV3System dut(.*);
wire O_sdram_clk,O_sdram_cke,O_sdram_cs_n,O_sdram_cas_n,O_sdram_ras_n,O_sdram_wen_n;
wire [3:0]O_sdram_dqm;wire [10:0]O_sdram_addr;wire [1:0]O_sdram_ba;wire [31:0]IO_sdram_dq;
TangNano20KSdramNativeBridge108M54M #(.PREPARE_NEXT(1),.COLD_START_ONLY(1),.CAS(3),.RCD_CYCLES(3),.RP_CYCLES(3),.CAPTURE_LATENCY(3),.REGISTER_READ_RETURN(1)) bridge(
 .logic_clk(clk),.controller_clk(controller_clk),.sdram_clk(phy_clk),.reset(reset),
 .request_valid(sdram_request_valid),.request_continuation(sdram_request_continuation),.next_valid(sdram_next_valid),.next_address(sdram_next_address),
 .writing(sdram_write),.address(sdram_address),.words(sdram_words),.write_mask(sdram_write_mask),
 .write_data(sdram_write_data),.write_data_valid(sdram_write_data_valid),
 .request_ready(sdram_request_ready),.stream_active(sdram_stream_active),
 .write_data_ready(sdram_write_data_ready),.read_data(sdram_read_data),.read_valid(sdram_read_valid),
 .write_done(sdram_write_done),.initialized(sdram_init_done),
 .O_sdram_clk(O_sdram_clk),.O_sdram_cke(O_sdram_cke),.O_sdram_cs_n(O_sdram_cs_n),
 .O_sdram_cas_n(O_sdram_cas_n),.O_sdram_ras_n(O_sdram_ras_n),.O_sdram_wen_n(O_sdram_wen_n),
 .O_sdram_dqm(O_sdram_dqm),.O_sdram_addr(O_sdram_addr),.O_sdram_ba(O_sdram_ba),.IO_sdram_dq(IO_sdram_dq));
// Reference board delay corner; final fitted SDF is qualified separately.
wire pin_cke,pin_cs,pin_ras,pin_cas,pin_we,pin_write_drive;
wire [3:0]pin_dqm;wire [10:0]pin_a;wire [1:0]pin_ba;wire [31:0]pin_dq,pin_output_data;
assign #(3.916) {pin_cke,pin_cs,pin_ras,pin_cas,pin_we,pin_dqm,pin_a,pin_ba}=
 {O_sdram_cke,O_sdram_cs_n,O_sdram_ras_n,O_sdram_cas_n,O_sdram_wen_n,O_sdram_dqm,O_sdram_addr,O_sdram_ba};
assign #(2.300) pin_write_drive=bridge.u_controller.dq_drive;
assign #(3.916) pin_output_data=bridge.u_controller.dq_out;
assign pin_dq=pin_write_drive?pin_output_data:32'bz;
assign IO_sdram_dq=bridge.u_controller.dq_drive?32'bz:pin_dq;
wire [31:0]pin_cycles,refreshes;
LabPinModel #(.PERIOD_NS(9.259259),.CAS_LATENCY(3),.TAC(5.4),.TOH(2.5),.THZ(5.4),.RCD_MIN(3),.RP_MIN(3),.RETURN_MIN(1.392),.RETURN_MAX(2.314)) memory(
 .sclk(O_sdram_clk),.reset(reset),.cke(pin_cke),.cs(pin_cs),.ras(pin_ras),
 .cas(pin_cas),.we(pin_we),.dqm(pin_dqm),.a(pin_a),.ba(pin_ba),
 .dq(pin_dq),.cycle(pin_cycles),.refreshes(refreshes));
reg [7:0]flash_image[0:__PACKAGE_BYTES__-1];
reg [31:0]command=0;integer command_bits=0,data_bit=0;reg [23:0]byte_address=0;
reg [7:0]current_byte=0;
initial begin
__PACKAGE_INIT__
end
always @(posedge flash_cs_n)begin command_bits=0;data_bit=0;end
always @(posedge flash_clk)if(!flash_cs_n && command_bits<32)begin
 command={command[30:0],flash_mosi};command_bits=command_bits+1;
 if(command_bits==32)begin
  if(command[31:24]!=8'h03)$fatal(1,"Flash opcode");byte_address=command[23:0];
 end
end
always @(negedge flash_clk)if(!flash_cs_n && command_bits>=32)begin
 if(data_bit==0)begin
  current_byte=byte_address>=24'h100000 && byte_address<24'h100000+__PACKAGE_BYTES__ ?
    flash_image[byte_address-24'h100000] : 8'hff;
  byte_address=byte_address+1;
 end
 flash_miso<=#6.5 current_byte[7-data_bit];data_bit=(data_bit+1)%8;
end
integer cycles=0,frames=0,byte_count=0;
integer maintenance_trace=0;
integer maintenance_unaccepted=0;
integer progress_fd;
initial progress_fd=$fopen("__PROGRESS_PATH__","w");
reg [7:0]received,packet[0:7],checksum;
integer bit_index,i;
// Decode the actual system UART pin, including start/stop timing.
always @(negedge uart_tx)begin
 repeat(703)@(posedge clk);
 for(bit_index=0;bit_index<8;bit_index=bit_index+1)begin
  received[bit_index]=uart_tx;repeat(469)@(posedge clk);
 end
 if(uart_tx!==1'b1)$fatal(1,"UART stop bit");
 packet[byte_count]=received;
 if(byte_count==7)begin
  checksum=0;for(i=0;i<8;i=i+1)checksum=checksum^packet[i];
  if({packet[0],packet[1],packet[2],packet[3]}!==32'h44444854 ||
     packet[4]!=1 || packet[5]!=8'h0b || packet[6]!=0 || checksum!=0)
    $fatal(1,"board application UART status %h",packet[6]);
  if(dut.u_display.next_pending || dut.u_display.active_linear ||
     dut.u_display.memory_error_sticky || dut.u_display.underflow_sticky)
    $fatal(1,"Display completion/health");
  if(frames==0 && dut.u_display.active_base!=22'h218000)$fatal(1,"first target");
  if(frames==1 && dut.u_display.active_base!=22'h200000)$fatal(1,"second target");
  frames=frames+1;
  $display("native board frame%0d cycles%0d refreshes%0d",frames,cycles,refreshes);
  if(frames==2)begin
   if(refreshes==0)$fatal(1,"no refresh contention exercised");
   $display("CPU/gear/MC/Display real SPI and SDRAM pins; CL3 model 108 MHz");
   $display("DIGITAL_DESIGN_PASS");$finish;
  end
  byte_count=0;
 end else byte_count=byte_count+1;
end
always @(posedge clk)begin
 cycles=cycles+1;
 if((dut.sysctl_dcache_invalidate || dut.sysctl_dcache_clean) &&
    (dut.u_core.async_store_valid || dut.u_core.async_store_issued ||
     dut.u_core.fpu2_pending!=0 || dut.u_data_cache.state!=0 ||
     dut.u_data_cache.response_valid || dut.dcache_maintenance_busy || dut.dcache_valid_sweep))
  $fatal(1,"application global maintenance did not follow the audited idle boundary");
 if(dut.sysctl_cpu_hold && !dut.u_sysctl.watch_active && !dut.dcache_maintenance_busy)
  maintenance_unaccepted=maintenance_unaccepted+1;
 else maintenance_unaccepted=0;
 if(maintenance_unaccepted>2000)$fatal(1,"maintenance not accepted pc%h pending%0d response%0d store%0d",dut.pc,dut.u_sysctl.pending_dcache_maintenance,dut.u_data_cache.response_valid,dut.u_core.async_store_valid);
 if(dut.sysctl_dcache_invalidate || dut.sysctl_dcache_clean)maintenance_trace=24;
 if(maintenance_trace>0)begin
  $fdisplay(progress_fd,"edge%0d hold%0d pending%0d command%0d busy%0d done%0d state%0d response%0d ready%0d store%0d issued%0d request%0d fpu_pending%0d",cycles,
   dut.sysctl_cpu_hold,dut.u_sysctl.pending_dcache_maintenance,dut.sysctl_dcache_invalidate,
   dut.dcache_maintenance_busy,dut.dcache_maintenance_done,dut.u_data_cache.state,
   dut.u_data_cache.response_valid,dut.core_data_response_ready,dut.u_core.async_store_valid,
   dut.u_core.async_store_issued,dut.core_data_request_valid,dut.u_core.fpu2_pending);
  $fflush(progress_fd);maintenance_trace=maintenance_trace-1;
 end
 if(cycles%100000==0)begin
  $fdisplay(progress_fd,"cycles%0d boot%0d pc%h frame%0d target%h core%0d dcache%0d bridge%0d controller%0d r1=%h r2=%h r3=%h r6=%h r8=%h r9=%h r10=%h r11=%h SP=%h",cycles,
   dut.boot_done,dut.pc,dut.u_display.frame_index,dut.u_display.active_base,
   dut.u_core.state,dut.u_data_cache.state,dut.u_shared_sdram_port.used,bridge.core_phase,
   dut.u_core.u_gpr_ram.words[1],dut.u_core.u_gpr_ram.words[2],dut.u_core.u_gpr_ram.words[3],
   dut.u_core.u_gpr_ram.words[6],dut.u_core.u_gpr_ram.words[8],dut.u_core.u_gpr_ram.words[9],
   dut.u_core.u_gpr_ram.words[10],dut.u_core.u_gpr_ram.words[11],dut.u_core.u_gpr_ram.words[13]);
  $fflush(progress_fd);
 end
 if(dut.sysctl_dcache_invalidate || dut.sysctl_dcache_clean || dut.dcache_maintenance_done)
  $fdisplay(progress_fd,"maintenance cycles%0d hold%0d invalidate%0d clean%0d busy%0d done%0d pending%0d state%0d response%0d sweep%0d",cycles,
   dut.sysctl_cpu_hold,dut.sysctl_dcache_invalidate,dut.sysctl_dcache_clean,
   dut.dcache_maintenance_busy,dut.dcache_maintenance_done,dut.u_sysctl.pending_dcache_maintenance,
   dut.u_data_cache.state,dut.u_data_cache.response_valid,dut.dcache_valid_sweep);
 if(cycles>(PROBE_HALT!=0 ? 8000000 : 24000000))$fatal(1,"bounded board application pc%h core%0d dcache%0d hold%0d pending%0d busy%0d",dut.pc,dut.u_core.state,dut.u_data_cache.state,dut.sysctl_cpu_hold,dut.u_sysctl.pending_dcache_maintenance,dut.dcache_maintenance_busy);
 if(PROBE_HALT!=0 && dut.halted)begin
  if(dut.halt_signal!==PROBE_HALT)$fatal(1,"maintenance probe failed %h",dut.halt_signal);
  $display("native maintenance probe completed cycles%0d",cycles);
  $display("DIGITAL_DESIGN_PASS");$finish;
 end
 if(dut.boot_error || dut.faulted || dut.halted)
  $fatal(1,"boot/core failure boot%0d fault%0d pc%h halt%h",dut.boot_error_code,dut.fault_code,dut.fault_pc,dut.halt_signal);
 if(!dut.boot_done && (dut.core_instruction_request_valid || dut.device_write_enable || dut.core_data_request_valid))
  $fatal(1,"CPU side effect before boot");
end
initial begin #510000000;$fatal(1,"board application wall watchdog");end
endmodule
