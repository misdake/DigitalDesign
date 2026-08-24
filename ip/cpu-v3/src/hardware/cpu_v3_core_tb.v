module tb;
reg clk = 0;
reg reset = 1;
reg instruction_request_ready = 1;
reg instruction_response_valid = 0;
reg [15:0] instruction_data = 0;
reg instruction_error = 0;
reg data_request_ready = 1;
reg data_response_valid = 0;
reg [15:0] data_read_data = 0;
reg data_error = 0;
wire instruction_request_valid;
wire [31:0] instruction_address;
wire instruction_response_ready;
wire data_request_valid;
wire data_write;
wire [31:0] data_address;
wire [15:0] data_write_data;
wire data_response_ready;
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
integer index;

always @(posedge clk) begin
    instruction_response_valid <= instruction_request_valid;
    if (instruction_request_valid)
        instruction_data <= memory[instruction_address[15:0]];
    data_response_valid <= data_request_valid;
    if (data_request_valid) begin
        if (data_write)
            memory[data_address[15:0]] <= data_write_data;
        else
            data_read_data <= memory[data_address[15:0]];
    end
end

initial begin
    for (index = 0; index < 65536; index = index + 1)
        memory[index] = 0;
    memory[0] = 16'hfff0;
    memory[1] = 16'hafd0;
    memory[2] = 16'hfff0;
    memory[3] = 16'haff0;
    memory[4] = 16'haf00;
    memory[5] = 16'haf15;
    memory[6] = 16'he1c1;
    memory[7] = 16'ha9c0;
    memory[8] = 16'hf000;
    memory[9] = 16'hb0c1;
    memory[10] = 16'he800;
    memory[11] = 16'h0001;
    memory[12] = 16'haf21;
    memory[13] = 16'h1112;
    memory[14] = 16'hf0ff;
    memory[15] = 16'hcff6;
    memory[16] = 16'he800;

    repeat (3) @(posedge clk);
    reset = 0;
    wait (halted || fault);
    #1;
    if (fault || halt_signal !== 16'd15 || retired_words == 0) begin
        $display("FAIL: fault=%d code=%d signal=%h retired=%d", fault, fault_code, halt_signal, retired_words);
        $finish(1);
    end
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    #20000;
    $display("FAIL: timeout");
    $finish(1);
end
endmodule
