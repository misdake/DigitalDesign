module tb;
reg clk = 0;
reg reset = 0;
reg start = 0;
reg [23:0] flash_offset = 24'h100;
reg [21:0] destination = 22'h100007;
reg [31:0] file_size_bytes = 3;
reg [31:0] memory_size_bytes = 6;
reg flash_ready = 1;
wire flash_data_valid;
wire [7:0] flash_data;
reg flash_done = 0;
reg flash_error = 0;
reg memory_request_ready = 1;
reg memory_response_valid = 0;
reg memory_error = 0;
wire busy;
wire done;
wire error;
wire [7:0] error_code;
wire [31:0] completed_words;
wire flash_start;
wire [23:0] flash_address;
wire [23:0] flash_length;
wire flash_data_ready;
wire memory_request_valid;
wire memory_write;
wire [21:0] memory_address;
wire [15:0] memory_write_data;
wire memory_response_ready;

BootDmaEngine dut(.*);
always #5 clk = ~clk;

reg [7:0] source [0:2];
integer source_index = 0;
integer write_index = 0;

// Hold each byte until the engine actually consumes it; the engine drops
// flash_data_ready while it performs the memory write for each word.
assign flash_data_valid = source_index < 3;
assign flash_data = source[source_index];

always @(posedge clk) begin
    memory_response_valid <= 0;
    if (flash_data_ready && flash_data_valid)
        source_index <= source_index + 1;
    if (memory_request_valid) begin
        case (write_index)
            0: if (memory_address !== 22'h100007 || memory_write_data !== 16'h2211) $finish(1);
            1: if (memory_address !== 22'h100008 || memory_write_data !== 16'h0033) $finish(1);
            2: if (memory_address !== 22'h100009 || memory_write_data !== 16'h0000) $finish(1);
            default: $finish(1);
        endcase
        write_index <= write_index + 1;
    end
    if (memory_response_ready)
        memory_response_valid <= 1;
end

initial begin
    source[0] = 8'h11;
    source[1] = 8'h22;
    source[2] = 8'h33;
    repeat (2) @(posedge clk);
    start <= 1;
    @(posedge clk);
    start <= 0;
    wait (done || error);
    #1;
    if (error || completed_words !== 3 || write_index !== 3)
        $finish(1);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    #10000;
    $display("FAIL: timeout");
    $finish(1);
end
endmodule
