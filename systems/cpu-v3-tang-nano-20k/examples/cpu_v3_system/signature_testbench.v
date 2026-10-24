`timescale 1ns/1ps
// Real SPI reader + full CPU/cache/arbiter/adapter, abstract native completion.
module tb;
localparam BULK_VIDEO_DELAY=__FLASH_PACKAGE_SIZE__>65536;
reg clk = 0;
reg [1:0] buttons = 0;
reg flash_miso = 1;
reg [63:0] sdram_read_data = 0;
reg sdram_read_valid = 0;
reg sdram_init_done = 0;
wire sdram_request_ready;
reg sdram_stream_active = 0;
reg sdram_clock_ready = 1;
reg sdram_done = 0;
reg sdram_write_data_ready = 0;
wire [5:0] leds;
wire uart_tx;
wire flash_clk;
wire flash_cs_n;
wire flash_mosi;
wire sdram_request_valid;
wire sdram_next_valid;
wire [20:0] sdram_next_address;
wire sdram_write;
wire [20:0] sdram_address;
wire [5:0] sdram_words;
wire [3:0] sdram_write_mask;
wire [63:0] sdram_write_data;
wire sdram_write_data_valid;
reg pixel_clock = 0;
reg serial_clock = 0;
reg video_locked = 1;
wire tmds_clk_p;
wire tmds_clk_n;
wire [2:0] tmds_data_p;
wire [2:0] tmds_data_n;

CpuV3System dut(.*);
always #9.259 clk = ~clk;
// The bulk case also verifies late video-clock availability. Ordinary cases
// and the native-pin composition keep video running throughout cold loading.
always #15.015 if(!BULK_VIDEO_DELAY || dut.boot_load_done) pixel_clock = ~pixel_clock;
always #3.003 if(!BULK_VIDEO_DELAY || dut.boot_load_done) serial_clock = ~serial_clock;

// Abstract native 32-bit controller port. Address is a 32-bit word address.
// The board bridge groups two native words into each 64-bit system beat.
reg [15:0] memory [0:4194303];
reg write_pending = 0;
reg [20:0] transfer_address = 0;
reg [5:0] transfer_words = 0;
reg [3:0] transfer_mask = 0;
integer transfer_pairs = 0;
integer transfer_pair = 0;
integer read_delay = 0;
integer read_pairs = 0;
reg word_read_seen = 0;
reg line_burst_seen = 0;
reg [20:0] idx;
reg [20:0] idx2;
integer cycle;

assign sdram_request_ready = !write_pending &&
    read_delay == 0 && read_pairs == 0;
always @(*) sdram_write_data_ready = write_pending;

always @(posedge clk) begin
    sdram_done <= 0;
    sdram_read_valid <= 0;
    if (sdram_request_valid && sdram_request_ready) begin
        if (sdram_words != 1 && sdram_words != 8 &&
            sdram_words != 16 && sdram_words != 32)
            $fatal(1, "illegal native descriptor length %0d", sdram_words);
        if (write_pending || read_delay != 0 || read_pairs != 0)
            $fatal(1, "overlapping native descriptors write=%0d delay=%0d pairs=%0d new_write=%0d new_words=%0d new_addr=%h old_words=%0d old_addr=%h port_state=%0d",
                write_pending, read_delay, read_pairs, sdram_write,
                sdram_words, sdram_address, transfer_words,
                transfer_address, dut.u_shared_sdram_port.state);
        transfer_address <= sdram_address;
        transfer_words <= sdram_words;
        transfer_mask <= sdram_write_mask;
        transfer_pair <= 0;
        transfer_pairs <= sdram_words == 1 ? 1 : sdram_words / 2;
        if (sdram_write) begin
            write_pending <= 1;
        end else begin
            read_delay <= 2;
            read_pairs <= sdram_words == 1 ? 1 : sdram_words / 2;
            if (sdram_write_mask != 0) $fatal(1, "read DQM mask not zero");
            if (sdram_words == 1) word_read_seen <= 1;
            else line_burst_seen <= 1;
        end
    end

    if (write_pending && sdram_write_data_valid && sdram_write_data_ready) begin
        idx = transfer_address + 2*transfer_pair;
        if (transfer_words == 1) begin
            if (!transfer_mask[0]) memory[{idx,1'b0}] <= sdram_write_data[15:0];
            if (!transfer_mask[2]) memory[{idx,1'b1}] <= sdram_write_data[31:16];
        end else begin
            idx2 = idx + 1;
            memory[{idx,1'b0}] <= sdram_write_data[15:0];
            memory[{idx,1'b1}] <= sdram_write_data[31:16];
            memory[{idx2,1'b0}] <= sdram_write_data[47:32];
            memory[{idx2,1'b1}] <= sdram_write_data[63:48];
        end
        transfer_pair <= transfer_pair + 1;
        if (transfer_pair == transfer_pairs - 1) begin
            write_pending <= 0;
            sdram_done <= 1;
        end
    end

    if (read_delay != 0) read_delay <= read_delay - 1;
    else if (read_pairs != 0) begin
        idx = transfer_address + 2*transfer_pair;
        idx2 = idx + 1;
        sdram_read_data <= transfer_words == 1 ?
            {32'b0, memory[{idx,1'b1}], memory[{idx,1'b0}]} :
            {memory[{idx2,1'b1}], memory[{idx2,1'b0}],
             memory[{idx,1'b1}], memory[{idx,1'b0}]};
        sdram_read_valid <= 1;
        transfer_pair <= transfer_pair + 1;
        read_pairs <= read_pairs - 1;
        if (read_pairs == 1) sdram_done <= 1;
    end
end


localparam integer FLASH_BASE=32'h100000;
localparam integer FLASH_PACKAGE_SIZE=__FLASH_PACKAGE_SIZE__;
localparam integer CODE_WORDS=__CODE_WORDS__;
localparam integer EXPECT_ERROR=__EXPECT_ERROR__;
localparam integer EXPECT_HALT=__EXPECT_HALT__;
reg [7:0]flash_image[0:FLASH_PACKAGE_SIZE-1];
reg [15:0]expected_code[0:CODE_WORDS-1];
reg [31:0]flash_command=0;integer flash_command_bits=0;
reg [23:0]flash_byte_address=0;integer flash_data_bit=0;reg [7:0]flash_current_byte=0;
integer n,start_cycle=0,release_cycle=0,first_fetch_cycle=0,ready_cycle=0,load_cycle=0,receipt_file;
reg released=0,first_fetch_seen=0;
task save_startup;
begin
 receipt_file=$fopen("__STARTUP_RESULT_PATH__","w");
 if(receipt_file==0)$fatal(1,"cannot save startup receipt");
 $fdisplay(receipt_file,"{\"sdram_ready_cycle\":%0d,\"load_cycle\":%0d,\"release_cycle\":%0d,\"first_fetch_cycle\":%0d,\"end_cycle\":%0d,\"code_words\":%0d,\"image_bytes\":%0d,\"spi_half_period\":%0d,\"late_video_clock\":%0d,\"expected_halt\":%0d,\"boot_error\":%0d}",ready_cycle,load_cycle,release_cycle,first_fetch_cycle,start_cycle,CODE_WORDS,FLASH_PACKAGE_SIZE,__SPI_HALF_PERIOD__,BULK_VIDEO_DELAY,EXPECT_HALT,dut.boot_error);
 $fclose(receipt_file);
end endtask
always @(posedge flash_cs_n) begin
    flash_command_bits = 0;
    flash_data_bit = 0;
end

always @(posedge flash_clk) begin
    if (!flash_cs_n && flash_command_bits < 32) begin
        flash_command = {flash_command[30:0], flash_mosi};
        flash_command_bits = flash_command_bits + 1;
        // The 24-bit address is complete after the 32nd command bit.
        if (flash_command_bits == 32)
            flash_byte_address = flash_command[23:0];
    end
end

always @(negedge flash_clk) begin
    if (!flash_cs_n && flash_command_bits >= 32) begin
        if (flash_data_bit == 0) begin
            if (flash_byte_address >= FLASH_BASE && flash_byte_address < FLASH_BASE + FLASH_PACKAGE_SIZE)
                flash_current_byte = flash_image[flash_byte_address - FLASH_BASE];
            else
                flash_current_byte = 8'hff;
            flash_byte_address = flash_byte_address + 1;
        end
        flash_miso <= #6.5 flash_current_byte[7 - flash_data_bit];
        flash_data_bit = (flash_data_bit + 1) % 8;
    end
end


initial begin
 for(n=0;n<4194304;n=n+1)memory[n]=16'hd00d;
__FLASH_PACKAGE_INIT__
__CODE_INIT__
 repeat(11)@(negedge clk);sdram_clock_ready=0;
 repeat(7)@(negedge clk);sdram_clock_ready=1;
 repeat(9)@(negedge clk);sdram_init_done=1;
end
always @(posedge clk)begin
 start_cycle=start_cycle+1;
 if(sdram_init_done && ready_cycle==0)ready_cycle=start_cycle;
 if(start_cycle>__BOOT_MAX_CYCLES__)$fatal(1,"bounded full Flash boot");
 if(dut.boot_load_done && load_cycle==0)load_cycle=start_cycle;
 if(!dut.boot_load_done && dut.display_memory_request_valid)$fatal(1,"Display fetched before load completion");
 if(!dut.u_display.front_valid && dut.display_memory_request_valid)
  $fatal(1,"Display fetched before first applied present");
 if(!dut.boot_done && (dut.core_instruction_request_valid || dut.device_write_enable || dut.core_data_request_valid))$fatal(1,"CPU side effect before boot completion");
 if(dut.reset!==1'b0)$fatal(1,"production runtime reset remained active");
 if(dut.boot_error)begin
  if(!EXPECT_ERROR)$fatal(1,"boot failure %0d",dut.boot_error_code);
  if(dut.boot_error_code!=__EXPECT_ERROR_CODE__)$fatal(1,"incorrect boot failure category %0d",dut.boot_error_code);
  if(dut.retired_words!=0 || dut.boot_done)$fatal(1,"failed image executed");
  $display("DIGITAL_DESIGN_PASS single Flash rejection cycles%0d",start_cycle);save_startup();$display("DIGITAL_DESIGN_PASS");$finish;
 end
 if(EXPECT_ERROR && dut.boot_done)$fatal(1,"malformed image released CPU");
 if(dut.boot_done && !released)begin
  released=1;release_cycle=start_cycle;
  if(dut.pc!==0 || dut.code_segment!==7 || dut.data_segment!==0)$fatal(1,"cold entry mapping");
  if(write_pending || dut.u_boot_loader.remaining!=0 || !dut.boot_load_done || !dut.display_started_ack)
   $fatal(1,"release before completed writes and Display start");
  for(n=32'h200000;n<32'h230000;n=n+1)if(memory[n]!==16'hd00d)$fatal(1,"boot wrote an unpresented framebuffer");
  for(n=0;n<8192;n=n+1)if(memory[n]!==0)$fatal(1,"static zero %0d",n);
  if(memory[8192]!==16'hd00d)$fatal(1,"clear exceeds fixed static arena");
  for(n=0;n<CODE_WORDS;n=n+1)if(memory[32'h70000+n]!==expected_code[n])$fatal(1,"code mismatch %0d",n);
  if(memory[32'h70000+CODE_WORDS]!==16'hd00d)$fatal(1,"copy exceeds code extent");
__RESOURCE_CHECK__
  for(n=32'h230000;n<32'h248000;n=n+1)if(memory[n]!==16'hd00d)$fatal(1,"boot wrote Depth");
 end
 if(dut.core_instruction_request_valid && !first_fetch_seen)begin
  first_fetch_seen=1;first_fetch_cycle=start_cycle;
  if(!released || dut.core_instruction_address!==32'h70000)$fatal(1,"first application fetch");
 end
 if(dut.faulted)$fatal(1,"application fault %0d at %h instruction%h expected%h prefix%h cseg%h",dut.fault_code,dut.fault_pc,dut.u_core.instruction,expected_code[dut.fault_pc],dut.u_core.prefix_high,dut.code_segment);
 if(released && start_cycle==release_cycle+2)buttons<=2'b11;
 if(released && start_cycle==release_cycle+5)buttons<=0;
 if(EXPECT_HALT!=0 && dut.halted)begin
  if(dut.halt_signal!==EXPECT_HALT || (EXPECT_HALT==16'h5a35 && dut.u_core.u_gpr_ram.words[13]!==16'hc000))$fatal(1,"static fixture/stack failed signal%h SP%h",dut.halt_signal,dut.u_core.u_gpr_ram.words[13]);
  $display("DIGITAL_DESIGN_PASS single Flash static fixture release%0d firstfetch%0d halt%0d",release_cycle,first_fetch_cycle,start_cycle);save_startup();$display("DIGITAL_DESIGN_PASS");$finish;
 end
 if(EXPECT_HALT==0 && first_fetch_seen && dut.device_write_enable)begin
  $display("DIGITAL_DESIGN_PASS single Flash production application release%0d firstfetch%0d firstdevice%0d",release_cycle,first_fetch_cycle,start_cycle);save_startup();$display("DIGITAL_DESIGN_PASS");$finish;
 end
end
initial begin #(__BOOT_MAX_CYCLES__*20.0);$fatal(1,"full Flash boot wall watchdog");end
endmodule
