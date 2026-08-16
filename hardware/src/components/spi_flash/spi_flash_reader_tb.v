module tb;
reg clk = 0;
reg start = 0;
reg [23:0] address = 0;
reg [23:0] length = 0;
reg data_ready = 0;
reg flash_miso = 0;
wire ready;
wire data_valid;
wire [7:0] data;
wire done;
wire error;
wire flash_clk;
wire flash_cs_n;
wire flash_mosi;

{{ module_name }} dut(
    .clk(clk),
    .start(start),
    .address(address),
    .length(length),
    .data_ready(data_ready),
    .flash_miso(flash_miso),
    .ready(ready),
    .data_valid(data_valid),
    .data(data),
    .done(done),
    .error(error),
    .flash_clk(flash_clk),
    .flash_cs_n(flash_cs_n),
    .flash_mosi(flash_mosi)
);

always #5 clk = ~clk;

reg [31:0] captured_command = 0;
integer command_bits = 0;
reg [23:0] model_data = {8'h{{ "{:02x}"|format(expected_0) }}, 8'h{{ "{:02x}"|format(expected_1) }}, 8'h{{ "{:02x}"|format(expected_2) }}};
integer output_bit = 23;

always @(posedge flash_clk) begin
    if (!flash_cs_n && command_bits < 32) begin
        captured_command <= {captured_command[30:0], flash_mosi};
        command_bits <= command_bits + 1;
    end
end

always @(negedge flash_clk) begin
    if (!flash_cs_n && command_bits >= 32 && output_bit >= 0) begin
        flash_miso <= model_data[output_bit];
        output_bit <= output_bit - 1;
    end
end

task accept_byte;
    input [7:0] expected;
    begin
        wait (data_valid);
        #1;
        if (data !== expected) begin
            $display("FAIL: expected byte %02x, got %02x", expected, data);
            $finish(1);
        end
        repeat (3) begin
            @(posedge clk);
            #1;
            if (!data_valid || data !== expected || flash_clk !== 0) begin
                $display("FAIL: response was not held under backpressure");
                $finish(1);
            end
        end
        data_ready = 1;
        @(posedge clk);
        #1;
        data_ready = 0;
    end
endtask

initial begin
    repeat (2) @(posedge clk);
    if (!ready || !flash_cs_n) begin
        $display("FAIL: reader did not start idle");
        $finish(1);
    end

    address = 24'h000001;
    length = 3;
    start = 1;
    @(posedge clk);
    #1;
    start = 0;

    accept_byte(8'h{{ "{:02x}"|format(expected_0) }});
    if (captured_command !== 32'h03000001) begin
        $display("FAIL: command/address was %08x", captured_command);
        $finish(1);
    end
    accept_byte(8'h{{ "{:02x}"|format(expected_1) }});
    accept_byte(8'h{{ "{:02x}"|format(expected_2) }});

    if (!done || error || !ready || !flash_cs_n) begin
        $display("FAIL: final handshake did not complete the burst");
        $finish(1);
    end
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    #200000;
    $display("FAIL: timeout");
    $finish(1);
end
endmodule
