module tb;
reg clk = 0;
reg reset = 1;
reg invalidate_all = 0;
reg cpu_request_valid = 0;
reg cpu_write = 0;
reg [31:0] cpu_address = 0;
reg [15:0] cpu_write_data = 0;
reg cpu_response_ready = 0;
reg memory_request_ready = 1;
reg memory_response_valid = 0;
reg [31:0] memory_read_data = 0;
reg memory_error = 0;
wire cpu_request_ready;
wire cpu_response_valid;
wire [15:0] cpu_read_data;
wire cpu_error;
wire memory_request_valid;
wire memory_write;
wire [21:0] memory_address;
wire [15:0] memory_write_data;
wire memory_response_ready;

CpuV3DirectMappedCache dut(.*);
always #5 clk = ~clk;

// Line-serving memory model: one read request returns eight ordered 32-bit
// beats (low half = even word), one write request returns one completion.
integer line_requests = 0;
integer write_requests = 0;
integer beats_served = 0;
integer drain_cycles = 0;
integer cycles = 0;
reg [3:0] beats_remaining = 0;
reg [21:0] line_base_r = 0;
reg write_response_pending = 0;
reg inject_error = 0;
reg [21:0] last_write_address = 0;
reg [15:0] last_write_data = 0;

wire [3:0] beat_index = 4'd8 - beats_remaining;

function [15:0] word_pattern;
    input [11:0] word_address;
    word_pattern = 16'h8000 | word_address;
endfunction

always @(posedge clk) begin
    cycles <= cycles + 1;
    if (dut.state == 4'd7)
        drain_cycles <= drain_cycles + 1;
    if (cycles > 20000)
        $fatal(1, "testbench cycle limit exceeded");

    memory_response_valid <= 0;
    memory_error <= 0;
    if (beats_remaining != 0) begin
        memory_response_valid <= 1;
        memory_read_data <= {word_pattern(line_base_r[11:0] + 2 * beat_index + 1),
                             word_pattern(line_base_r[11:0] + 2 * beat_index)};
        beats_served <= beats_served + 1;
        if (inject_error && beat_index == 2) begin
            // An error beat terminates the line response early.
            memory_error <= 1;
            beats_remaining <= 0;
        end else begin
            beats_remaining <= beats_remaining - 1;
        end
    end else if (write_response_pending) begin
        memory_response_valid <= 1;
        write_response_pending <= 0;
    end

    if (memory_request_valid && memory_request_ready) begin
        if (memory_write) begin
            write_requests <= write_requests + 1;
            last_write_address <= memory_address;
            last_write_data <= memory_write_data;
            write_response_pending <= 1;
        end else begin
            line_requests <= line_requests + 1;
            line_base_r <= memory_address;
            beats_remaining <= 8;
        end
    end
end

task read_word;
    input [31:0] address;
    input [15:0] expected;
    begin
        while (!cpu_request_ready) @(posedge clk);
        cpu_address <= address;
        cpu_write <= 0;
        cpu_request_valid <= 1;
        @(posedge clk);
        cpu_request_valid <= 0;
        while (!cpu_response_valid) @(posedge clk);
        if (cpu_error || cpu_read_data != expected)
            $fatal(1, "cache read failed at %h: %h", address, cpu_read_data);
        cpu_response_ready <= 1;
        @(posedge clk);
        cpu_response_ready <= 0;
    end
endtask

task read_hit_word;
    input [31:0] address;
    input [15:0] expected;
    integer response_cycles;
    begin
        while (!cpu_request_ready) @(posedge clk);
        cpu_address <= address;
        cpu_write <= 0;
        cpu_request_valid <= 1;
        @(posedge clk);
        #1;
        cpu_request_valid <= 0;
        response_cycles = 0;
        while (!cpu_response_valid) begin
            @(posedge clk);
            #1;
            response_cycles = response_cycles + 1;
        end
        if (response_cycles != 1)
            $fatal(1, "cache hit response took %0d cycles instead of one", response_cycles);
        if (cpu_error || cpu_read_data != expected)
            $fatal(1, "cache hit failed at %h: %h", address, cpu_read_data);
        cpu_response_ready <= 1;
        @(posedge clk);
        cpu_response_ready <= 0;
    end
endtask

task read_word_expect_error;
    input [31:0] address;
    begin
        while (!cpu_request_ready) @(posedge clk);
        cpu_address <= address;
        cpu_write <= 0;
        cpu_request_valid <= 1;
        @(posedge clk);
        cpu_request_valid <= 0;
        while (!cpu_response_valid) @(posedge clk);
        if (!cpu_error)
            $fatal(1, "cache read at %h did not report the injected error", address);
        cpu_response_ready <= 1;
        @(posedge clk);
        cpu_response_ready <= 0;
    end
endtask

task write_word;
    input [31:0] address;
    input [15:0] data;
    begin
        while (!cpu_request_ready) @(posedge clk);
        cpu_address <= address;
        cpu_write <= 1;
        cpu_write_data <= data;
        cpu_request_valid <= 1;
        @(posedge clk);
        cpu_request_valid <= 0;
        while (!cpu_response_valid) @(posedge clk);
        if (cpu_error)
            $fatal(1, "cache write failed at %h", address);
        cpu_response_ready <= 1;
        @(posedge clk);
        cpu_response_ready <= 0;
    end
endtask

initial begin
    repeat (2) @(posedge clk);
    reset <= 0;

    read_word(32'h0000_0123, 16'h8123);
    if (line_requests != 1 || beats_served != 8)
        $fatal(1, "miss did not refill exactly one line as eight beats");
    if (drain_cycles != 8)
        $fatal(1, "parity-split cache did not drain the line in eight cycles");
    read_hit_word(32'h0000_012e, 16'h812e);
    read_hit_word(32'h0000_012f, 16'h812f);
    if (line_requests != 1)
        $fatal(1, "line hit reached memory");

    write_word(32'h0000_0123, 16'h4567);
    if (write_requests != 1 || last_write_address != 22'h123 ||
        last_write_data != 16'h4567)
        $fatal(1, "write-through word transaction malformed");
    read_word(32'h0000_0123, 16'h4567);
    read_word(32'h0000_0122, 16'h8122);
    if (line_requests != 1)
        $fatal(1, "written line did not stay resident or parity neighbor caused a refill");

    invalidate_all <= 1;
    @(posedge clk);
    invalidate_all <= 0;
    read_word(32'h0000_0123, 16'h8123);
    if (line_requests != 2 || beats_served != 16)
        $fatal(1, "full invalidate did not invalidate the line");

    inject_error <= 1;
    read_word_expect_error(32'h0000_0523);
    inject_error <= 0;
    if (line_requests != 3 || beats_served != 19)
        $fatal(1, "error beat did not terminate the line response");
    read_word(32'h0000_0523, 16'h8523);
    if (line_requests != 4 || beats_served != 27)
        $fatal(1, "errored line must not have been installed");
    read_word(32'h0000_0123, 16'h8123);
    if (line_requests != 4)
        $fatal(1, "second same-set line did not use the invalid way");

    read_word(32'h0000_0923, 16'h8923);
    if (line_requests != 5 || beats_served != 35)
        $fatal(1, "third same-set line did not refill");
    read_word(32'h0000_0523, 16'h8523);
    if (line_requests != 5)
        $fatal(1, "deterministic replacement evicted the wrong way");
    read_word(32'h0000_0123, 16'h8123);
    if (line_requests != 6 || beats_served != 43)
        $fatal(1, "deterministic victim was not replaced");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
