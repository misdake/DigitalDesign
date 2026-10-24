wire request_valid, writing, write_valid, request_ready, write_ready;
wire [20:0] address;
wire [5:0] words;
wire [3:0] mask;
wire [63:0] write_data, read_data;
wire read_valid, write_done, initialized, stream_active, lookahead_window, next_valid;
wire request_continuation;
// Class three is direction-disjoint: GPU framebuffer reads or CPU line writes.
// Direction matching in the endpoint prevents those owners from being chained.
wire[1:0] cpu_chain_class=(gpu_ro_request_valid&&gpu_ro_request_ready)?2'd1:(gpu_fb_w_request_valid&&gpu_fb_w_request_ready)?2'd2:(gpu_fb_r_request_valid&&gpu_fb_r_request_ready)?2'd3:(data_request_valid&&data_request_ready&&data_write&&data_line)?2'd3:2'd0;
wire [20:0] next_address;
assign lookahead_enable = __EARLY_GRANT__ && lookahead_window;
__SHARED_PORT__ #(.EARLY_GRANT(__EARLY_GRANT__), .CHAIN_GROUP_FOUR(__CHAIN_GROUP_FOUR__)) adapter (
 .clk(logic_clk), .reset(reset),
 .cpu_request_valid(memory_request_valid), .cpu_write(memory_write),
 .cpu_line(memory_line), .cpu_address(memory_address),
 .cpu_line_count_minus_1(memory_line_count_minus_1), .cpu_chain_class(cpu_chain_class), .cpu_write_data(memory_write_data),
 .cpu_response_ready(memory_response_ready), .cpu_request_ready(memory_request_ready),
 .cpu_write_data_ready(memory_write_data_ready), .cpu_response_valid(memory_response_valid),
 .cpu_read_data(memory_read_data), .cpu_response_last(memory_response_last), .cpu_error(memory_error),
 .controller_read_data(read_data), .controller_read_valid(read_valid),
 .controller_init_done(initialized), .controller_request_ready(request_ready),
 .controller_stream_active(stream_active), .cpu_lookahead_window(lookahead_window),
 .controller_next_valid(next_valid), .controller_next_address(next_address),
 .controller_write_done(write_done), .controller_request_continuation(request_continuation), .controller_write_data_ready(write_ready),
 .controller_request_valid(request_valid), .controller_write(writing),
 .controller_address(address), .controller_write_mask(mask),
 .controller_write_data(write_data), .controller_write_data_valid(write_valid), .controller_words(words)
);
TangNano20KSdramNativeBridge108M54M #(.PREPARE_NEXT(__EARLY_GRANT__), .CHAIN_GROUP_FOUR(__CHAIN_GROUP_FOUR__)) bridge (
 .logic_clk(logic_clk), .controller_clk(controller_clk), .sdram_clk(sdram_clk), .reset(reset),
 .next_valid(next_valid), .next_address(next_address), .stream_active(stream_active),
 .request_valid(request_valid), .request_continuation(request_continuation), .writing(writing), .address(address), .words(words),
 .write_mask(mask), .write_data(write_data), .write_data_valid(write_valid),
 .request_ready(request_ready), .write_data_ready(write_ready), .read_data(read_data),
 .read_valid(read_valid), .write_done(write_done), .initialized(initialized),
 .O_sdram_clk(O_sdram_clk), .O_sdram_cke(O_sdram_cke), .O_sdram_cs_n(O_sdram_cs_n),
 .O_sdram_cas_n(O_sdram_cas_n), .O_sdram_ras_n(O_sdram_ras_n), .O_sdram_wen_n(O_sdram_wen_n),
 .O_sdram_dqm(O_sdram_dqm), .O_sdram_addr(O_sdram_addr), .O_sdram_ba(O_sdram_ba), .IO_sdram_dq(IO_sdram_dq)
);
