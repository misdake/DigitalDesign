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
reg [15:0] memory_read_data = 0;
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

integer memory_requests = 0;
always @(posedge clk) begin
    memory_response_valid <= memory_request_valid;
    if (memory_request_valid) begin
        memory_requests <= memory_requests + 1;
        memory_read_data <= 16'h8000 | memory_address[11:0];
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

initial begin
    repeat (2) @(posedge clk);
    reset <= 0;
    read_word(32'h0000_0123, 16'h8123);
    if (memory_requests != 16)
        $fatal(1, "miss did not refill exactly one line");
    read_word(32'h0000_012e, 16'h812e);
    if (memory_requests != 16)
        $fatal(1, "line hit reached memory");
    invalidate_all <= 1;
    @(posedge clk);
    invalidate_all <= 0;
    read_word(32'h0000_0123, 16'h8123);
    if (memory_requests != 32)
        $fatal(1, "full invalidate did not invalidate the line");
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
