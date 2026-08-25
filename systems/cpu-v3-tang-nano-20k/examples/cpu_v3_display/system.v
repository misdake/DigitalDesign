module CpuV3Display(
 input wire clk,input wire [1:0] buttons,
 input wire [31:0] sdram_read_data,input wire sdram_read_valid,
 input wire sdram_init_done,input wire sdram_command_ack,
 input wire pixel_clock,input wire serial_clock,input wire video_locked,
 output wire [5:0] leds,output wire uart_tx,
 output wire sdram_command_valid,output wire [2:0] sdram_command,
 output wire sdram_precharge,output wire [20:0] sdram_address,
 output wire [3:0] sdram_write_mask,output wire [31:0] sdram_write_data,
 output wire [7:0] sdram_burst_length,
 output wire tmds_clk_p,output wire tmds_clk_n,
 output wire [2:0] tmds_data_p,output wire [2:0] tmds_data_n
);
wire reset,unused_ready,unused_external;
__RESET__ u_reset(.clk(clk),.external_reset(|buttons),.clock_ready(sdram_init_done),
 .reset(reset),.clock_ready_synchronized(unused_ready),.external_reset_seen(unused_external));

wire ireq,iresp_ready; wire [31:0] iaddr; wire [15:0] idata;
wire boot_ready=!boot_pending&&!iresp_valid; reg boot_pending=0; reg iresp_valid=0;
wire [15:0] boot_data,unused_boot;
__BOOT_MEMORY__ u_boot(.clk(clk),.read_address(iaddr[9:0]),.rw_write_enable(1'b0),
 .rw_address(10'b0),.rw_write_data(16'b0),.read_data(boot_data),.rw_read_data(unused_boot));
always @(posedge clk) begin
 if(reset) begin boot_pending<=0; iresp_valid<=0; end
 else if(iresp_valid&&iresp_ready) iresp_valid<=0;
 else if(boot_pending) begin boot_pending<=0; iresp_valid<=1; end
 else if(ireq&&boot_ready) boot_pending<=1;
end
assign idata=boot_data;

wire dreq,dwrite,dresp_ready,dreq_ready,dresp_valid; wire [31:0] daddr;
wire [15:0] dwdata,drdata; wire derror;
wire cm_req,cm_write,cm_ready,cm_resp,cm_resp_ready,cm_error; wire [21:0] cm_addr; wire [15:0] cm_wdata,cm_rdata;
__CACHE__ u_dcache(.clk(clk),.reset(reset),.invalidate_all(1'b0),.snoop_write_valid(1'b0),.snoop_write_address(22'b0),
 .cpu_request_valid(dreq),.cpu_write(dwrite),.cpu_address(daddr),.cpu_write_data(dwdata),.cpu_response_ready(dresp_ready),
 .memory_request_ready(cm_ready),.memory_response_valid(cm_resp),.memory_read_data(cm_rdata),.memory_error(cm_error),
 .cpu_request_ready(dreq_ready),.cpu_response_valid(dresp_valid),.cpu_read_data(drdata),.cpu_error(derror),
 .memory_request_valid(cm_req),.memory_write(cm_write),.memory_address(cm_addr),.memory_write_data(cm_wdata),.memory_response_ready(cm_resp_ready));

wire halted,faulted; wire [15:0] halt_signal,fault_pc; wire [7:0] fault_code;
__CPU__ u_cpu(.clk(clk),.reset(reset),.instruction_request_ready(boot_ready),.instruction_response_valid(iresp_valid),
 .instruction_data(idata),.instruction_error(1'b0),.data_request_ready(dreq_ready),.data_response_valid(dresp_valid),
 .data_read_data(drdata),.data_error(derror),.instruction_request_valid(ireq),.instruction_address(iaddr),
 .instruction_response_ready(iresp_ready),.data_request_valid(dreq),.data_write(dwrite),.data_address(daddr),
 .data_write_data(dwdata),.data_response_ready(dresp_ready),.halted(halted),.halt_signal(halt_signal),
 .fault(faulted),.fault_code(fault_code),.fault_pc(fault_pc),.pc(),.code_segment(),.data_segment(),.retired_words());

wire display_req,display_urgent,display_ready,display_valid,display_last,display_mem_error;
wire [21:0] display_addr; wire [31:0] display_data; wire underflow;
__SDRAM__ u_sdram(.clk(clk),.reset(reset),.cpu_request_valid(cm_req),.cpu_write(cm_write),.cpu_address(cm_addr),
 .cpu_write_data(cm_wdata),.cpu_response_ready(cm_resp_ready),.display_request_valid(display_req),
 .display_urgent(display_urgent),.display_address(display_addr),.controller_read_data(sdram_read_data),
 .controller_read_valid(sdram_read_valid),.controller_init_done(sdram_init_done),.controller_command_ack(sdram_command_ack),
 .cpu_request_ready(cm_ready),.cpu_response_valid(cm_resp),.cpu_read_data(cm_rdata),.cpu_error(cm_error),
 .display_request_ready(display_ready),.display_data_valid(display_valid),.display_read_data(display_data),
 .display_last(display_last),.display_error(display_mem_error),.controller_command_valid(sdram_command_valid),
 .controller_command(sdram_command),.controller_precharge(sdram_precharge),.controller_address(sdram_address),
 .controller_write_mask(sdram_write_mask),.controller_write_data(sdram_write_data),.controller_burst_length(sdram_burst_length));
__DISPLAY__ u_display(.clk(clk),.reset(reset),.pixel_clock(pixel_clock),.serial_clock(serial_clock),.video_locked(video_locked),
 .memory_request_ready(display_ready),.memory_data_valid(display_valid),.memory_read_data(display_data),
 .memory_last(display_last),.memory_error(display_mem_error),.memory_request_valid(display_req),
 .memory_urgent(display_urgent),.memory_address(display_addr),.underflow(underflow),
 .tmds_clk_p(tmds_clk_p),.tmds_clk_n(tmds_clk_n),.tmds_data_p(tmds_data_p),.tmds_data_n(tmds_data_n));
assign leds={faulted,underflow,video_locked,sdram_init_done,cm_req,display_req};
assign uart_tx=1'b1;
endmodule
