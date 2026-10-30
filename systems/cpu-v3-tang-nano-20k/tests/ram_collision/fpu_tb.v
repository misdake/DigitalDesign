`timescale 1ns/1ps
module tb;
reg clk = 0;
reg reset = 1;
reg hold = 0;
reg instruction_request_ready = 1;
reg instruction_response_valid = 0;
reg [15:0] instruction_data = 0;
reg instruction_error = 0;
reg data_request_ready = 1;
reg data_response_valid = 0;
reg [15:0] data_read_data = 0;
reg data_error = 0;
reg data_line_copy_ready = 1;
reg data_cache_command_error = 0;
wire [15:0] device_read_data;
wire instruction_request_valid;
wire [31:0] instruction_address;
wire instruction_response_ready;
wire data_request_valid;
wire data_write;
wire [31:0] data_address;
wire [15:0] data_write_data;
wire data_response_ready;
wire data_line_copy_valid;
wire [21:0] data_line_copy_source;
wire data_line_clean_valid;
wire [21:0] data_line_clean_address;
wire [2:0] device_index;
wire [3:0] device_channel;
wire device_read_enable;
wire device_write_enable;
wire [15:0] device_write_data;
wire halted;
wire [15:0] halt_signal;
wire fault;
wire [7:0] fault_code;
wire [15:0] fault_pc;
wire [15:0] pc;
wire [15:0] code_segment;
wire [15:0] data_segment;
wire [31:0] retired_words;

CpuV3Core dut(.*);
always #5 clk = ~clk;

reg [15:0] memory [0:65535];
reg [15:0] devices [0:127];
assign device_read_data = devices[{device_index, device_channel}];
integer index;
integer cycles;
reg started;
reg end_flag;

always @(posedge clk) begin
instruction_response_valid <= instruction_request_valid;
if (instruction_request_valid)
instruction_data <= memory[instruction_address[15:0]];
data_response_valid <= data_request_valid;
if (data_request_valid && data_write)
memory[data_address[15:0]] <= data_write_data;
else if (data_request_valid)
data_read_data <= memory[data_address[15:0]];
end

reg [8:0]gold_address[0:__LAST_WRITE__];reg[31:0]gold_value[0:__LAST_WRITE__];
reg[31:0]oracle[0:511];reg[31:0]qa=0,qb=0;
integer writes=0,operands=0,tainted_operands=0,external_samples=0;reg injected=0;
initial begin
 for(index=0;index<65536;index=index+1)memory[index]=0;
 for(index=0;index<512;index=index+1)oracle[index]=0;
 for(index=0;index<128;index=index+1)devices[index]=0;
__ORACLE_INIT__
__PROGRAM_INIT__
 repeat(3)@(posedge clk);#1 reset=0;
 cycles=0;
 while(!halted && !fault && cycles<12000)begin @(posedge clk);#1;cycles=cycles+1;end
 if(!halted || fault)$fatal(1,"CPU failed to halt fault=%b code=%h pc=%h",fault,fault_code,pc);
 repeat(30)@(posedge clk);#1;
 if(writes!=__WRITE_COUNT__)$fatal(1,"wrong FPU write count %0d",writes);
__STORE_CHECKS__
 if(tainted_operands!=0)$fatal(1,"AUDIT_LIVE_TAINT operands=%0d",tainted_operands);
 if(dut.u_fpu.rf.audit_collisions==0)$fatal(1,"zero collision coverage");
 $display("DIGITAL_DESIGN_PASS FPU cycles=%0d writes=%0d operands=%0d tainted_effective=%0d external_samples=%0d collisions=%0d disabled_b_matches=%0d",cycles,writes,operands,tainted_operands,external_samples,dut.u_fpu.rf.audit_collisions,dut.u_fpu.rf.audit_disabled_b_matches);$finish;
end
initial begin repeat(12500)@(posedge clk);$fatal(1,"FPU cycle cap");end
always @(negedge clk)if($test$plusargs("FORCE_LIVE") && dut.u_fpu.issue && !injected)begin
 force dut.u_fpu.rf.read_a_data_r=32'hxxxxxxxx;injected=1;
end
always @(posedge clk)begin
 if(!reset)begin
  if(dut.u_fpu.issue)begin
   operands=operands+1;
   if(dut.u_fpu.rf.audit_taint_a)tainted_operands=tainted_operands+1;
   if(dut.u_fpu.rf_read_a_data!==qa)$fatal(1,"AUDIT_LIVE_OPERAND_A pc=%h got=%h expected=%h",pc,dut.u_fpu.rf_read_a_data,qa);
   if(!(dut.u_fpu.vector_op && (dut.u_fpu.vector_subop==6 || dut.u_fpu.vector_subop==7 || dut.u_fpu.vector_subop==12)))begin
    operands=operands+1;if(dut.u_fpu.rf.audit_taint_b)tainted_operands=tainted_operands+1;
    if(dut.u_fpu.rf_read_b_data!==qb)$fatal(1,"AUDIT_LIVE_OPERAND_B pc=%h got=%h expected=%h",pc,dut.u_fpu.rf_read_b_data,qb);
   end
  end
  if(!hold && ((dut.state==dut.ST_FPU2_CAPTURE) || (dut.state==dut.ST_FPU2_AUX_READ && dut.fpu2_seen_complete)))begin
   external_samples=external_samples+1;
   if(dut.u_fpu.rf.audit_taint_b)tainted_operands=tainted_operands+1;
   if(dut.fpu2_ext_read_data!==qb)$fatal(1,"AUDIT_LIVE_EXTERNAL_READ pc=%h",pc);
  end
 end
 // Read oracle sees the pre-write architectural value. Only real write commits update it.
 qa<=oracle[dut.u_fpu.rf.read_a_address];
 if(dut.u_fpu.rf.read_b_enable)qb<=oracle[dut.u_fpu.rf.read_b_address];
 if(dut.u_fpu.rf.write_enable && !reset)begin
  if(writes>=__WRITE_COUNT__ || dut.u_fpu.rf.write_address!==gold_address[writes] || dut.u_fpu.rf.write_data!==gold_value[writes])
   $fatal(1,"AUDIT_LIVE_COMMIT index=%0d addr=%h value=%h expected_addr=%h expected_value=%h",writes,dut.u_fpu.rf.write_address,dut.u_fpu.rf.write_data,gold_address[writes],gold_value[writes]);
  oracle[gold_address[writes]]=gold_value[writes];writes=writes+1;
 end
end
endmodule
