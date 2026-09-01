module tb;
reg clk = 0;
reg reset = 1;
reg flush = 0;
reg core_request_valid = 0;
reg [31:0] core_address = 0;
reg core_response_ready = 1;
reg memory_request_ready = 1;
reg memory_response_valid = 0;
reg [15:0] memory_read_data = 0;
reg memory_error = 0;
wire core_request_ready;
wire core_response_valid;
wire [15:0] core_read_data;
wire core_error;
wire memory_request_valid;
wire [31:0] memory_address;
wire memory_response_ready;
wire prefetch_request_valid;
wire [31:0] prefetch_address;
wire prefetch_cancel;

CpuV3InstructionFetchQueue dut(.*);
always #5 clk = ~clk;

integer cycles = 0;
integer accepts = 0;
integer previous_accept_cycle = -1;
integer consecutive_accepts = 0;
integer prefetch_candidates = 0;
integer prefetch_cancels = 0;
reg [31:0] last_prefetch_address = 0;

function [15:0] word_pattern;
    input [31:0] address;
    word_pattern = 16'h6000 ^ address[15:0];
endfunction

always @(posedge clk) begin
    cycles <= cycles + 1;
    if (cycles > 500)
        $fatal(1, "fetch queue test exceeded cycle limit");
    memory_response_valid <= memory_request_valid && memory_request_ready;
    if (memory_request_valid && memory_request_ready) begin
        memory_read_data <= word_pattern(memory_address);
        accepts <= accepts + 1;
        if (previous_accept_cycle >= 0 && cycles == previous_accept_cycle + 1)
            consecutive_accepts <= consecutive_accepts + 1;
        previous_accept_cycle <= cycles;
    end
    if (prefetch_request_valid) begin
        prefetch_candidates <= prefetch_candidates + 1;
        last_prefetch_address <= prefetch_address;
    end
    if (prefetch_cancel)
        prefetch_cancels <= prefetch_cancels + 1;
end

always @(negedge clk) begin
    if (!reset && (dut.queue_count > 4 || dut.metadata_count > 4 ||
                   dut.queue_count + dut.metadata_count > 4))
        $fatal(1, "fetch queue reservation overflow: queued=%0d outstanding=%0d",
               dut.queue_count, dut.metadata_count);
end

task consume;
    input [31:0] address;
    begin
        core_address <= address;
        core_request_valid <= 1;
        while (!core_request_ready) @(negedge clk);
        if (!core_response_valid || core_error || core_read_data != word_pattern(address))
            $fatal(1, "fetch queue returned stale/wrong word at %h: %h", address,
                   core_read_data);
        @(posedge clk);
        #1;
        core_request_valid <= 0;
    end
endtask

task consume_redirect_fast;
    input [31:0] address;
    integer redirect_cycle;
    begin
        core_address <= address;
        core_request_valid <= 1;
        #1;
        if (!memory_request_valid || memory_address != address)
            $fatal(1, "redirect did not issue its target lookup immediately");
        redirect_cycle = cycles;
        while (!core_request_ready) @(negedge clk);
        if (cycles - redirect_cycle > 2)
            $fatal(1, "redirect response bypass took %0d cycles", cycles - redirect_cycle);
        if (!core_response_valid || core_error || core_read_data != word_pattern(address))
            $fatal(1, "fast redirect returned stale/wrong word at %h: %h", address,
                   core_read_data);
        @(posedge clk);
        #1;
        core_request_valid <= 0;
    end
endtask

initial begin
    repeat (2) @(posedge clk);
    reset <= 0;

    consume(32'h0001_1000);
    consume(32'h0001_1001);
    consume(32'h0001_1002);
    if (consecutive_accepts < 2)
        $fatal(1, "fetch queue did not issue consecutive memory lookups");

    // Redirect while old sequential words can still be queued or outstanding.
    consume_redirect_fast(32'h0002_2000);
    consume(32'h0002_2001);

    // Invalidation toggles the epoch; old responses must be drained, not used.
    core_address <= 32'h0003_3000;
    core_request_valid <= 1;
    flush <= 1;
    @(posedge clk);
    flush <= 0;
    while (!core_request_ready) @(negedge clk);
    if (core_read_data != word_pattern(32'h0003_3000))
        $fatal(1, "post-flush fetch used a stale response");
    @(posedge clk);
    #1;
    core_request_valid <= 0;

    // Hold the core response path closed until all four reservations contain
    // fetched words. No fifth downstream lookup may escape while full.
    flush <= 1;
    @(posedge clk);
    #1;
    flush <= 0;
    while (dut.metadata_count != 0) @(negedge clk);
    core_address <= 32'h0004_4000;
    core_request_valid <= 1;
    core_response_ready <= 0;
    while (dut.queue_count != 4) @(negedge clk);
    repeat (3) begin
        if (memory_request_valid)
            $fatal(1, "fetch queue issued a lookup with all reservations full");
        @(negedge clk);
    end
    core_response_ready <= 1;
    while (!core_request_ready) @(negedge clk);
    if (core_read_data != word_pattern(32'h0004_4000))
        $fatal(1, "backpressured queue returned the wrong head word");
    @(posedge clk);
    #1;
    core_request_valid <= 0;

    consume(32'h0005_500a);
    if (prefetch_candidates != 1 || last_prefetch_address != 32'h0005_5010)
        $fatal(1, "real word-10 progress did not nominate the next line");
    if (prefetch_cancels == 0)
        $fatal(1, "redirects did not cancel obsolete prefetch work");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
