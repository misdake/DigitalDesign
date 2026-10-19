`timescale 1ns/1ps
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

// 54 MHz logic: divisor 1/2 gives nominal 27/13.5 MHz SCK.
always #9.259 clk = ~clk;

reg [31:0] captured_command = 0;
integer command_bits = 0;
integer commands = 0;
integer output_bit = 7;
reg stream_started = 0;
reg [23:0] model_address = 0;
reg [7:0] model_data = 0;
localparam integer CAPACITY = {{ capacity_bytes }};
localparam integer HALF_PERIOD = {{ half_period_cycles }};
realtime last_spi_edge = 0;
realtime minimum_half_ns = 1.0e9;

always @(flash_clk) begin
    if (!flash_cs_n) begin
        if ($realtime-last_spi_edge < minimum_half_ns)
            minimum_half_ns=$realtime-last_spi_edge;
        if ($realtime - last_spi_edge < 18.517 * HALF_PERIOD)
            $fatal(1, "SPI clock violated the configured half period");
        last_spi_edge = $realtime;
    end
end

function [7:0] image_byte;
    input [23:0] byte_address;
    begin
        case (byte_address)
            1: image_byte = 8'h{{ "{:02x}"|format(expected_0) }};
            2: image_byte = 8'h{{ "{:02x}"|format(expected_1) }};
            3: image_byte = 8'h{{ "{:02x}"|format(expected_2) }};
            default: image_byte = byte_address[7:0] ^ byte_address[15:8] ^
                                  byte_address[23:16] ^ 8'ha7;
        endcase
    end
endfunction

always @(negedge flash_cs_n) begin
    captured_command = 0;
    command_bits = 0;
    output_bit = 7;
    stream_started = 0;
    commands = commands + 1;
    last_spi_edge = $realtime;
end

always @(posedge flash_clk) begin
    if (!flash_cs_n && command_bits < 32) begin
        captured_command = {captured_command[30:0], flash_mosi};
        command_bits = command_bits + 1;
    end
end

always @(negedge flash_clk) begin
    if (!flash_cs_n && command_bits == 32) begin
        if (!stream_started) begin
            if (captured_command[31:24] !== 8'h03)
                $fatal(1, "unexpected SPI opcode %08x", captured_command);
            model_address = captured_command[23:0];
            stream_started = 1;
        end
        model_data = image_byte(model_address);
        // Reference-device tCLQV only; board/FPGA I/O delay is not modeled.
        flash_miso <= #6.5 model_data[output_bit];
        if (output_bit == 0) begin
            output_bit = 7;
            model_address = model_address + 1'b1;
        end else output_bit = output_bit - 1;
    end
end

task accept_byte;
    input [7:0] expected;
    input final_byte;
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
            if (!data_valid || data !== expected || flash_clk !== 0 ||
                ready || done || flash_cs_n !== final_byte) begin
                $display("FAIL: response was not held under backpressure");
                $finish(1);
            end
        end
        @(negedge clk);
        data_ready = 1;
        start = 0;
        @(posedge clk);
        #1;
        if (data_valid || error || done !== final_byte || ready !== final_byte)
            $fatal(1, "incorrect byte acceptance/completion boundary");
        data_ready = 0;
    end
endtask

task read_bytes;
    input [23:0] base;
    input integer count;
    integer index, previous_commands;
    begin
        @(negedge clk);
        if (!ready) $fatal(1, "reader did not return to idle");
        previous_commands = commands;
        address = base;
        length = count;
        start = 1;
        @(posedge clk); #1;
        start = 0;
        if (ready || flash_cs_n || done || error)
            $fatal(1, "valid command was not accepted");
        // A busy reader must ignore a competing start and live descriptor changes.
        address = 24'hffffff;
        length = 24'hffffff;
        start = 1;
        for (index = 0; index < count; index = index + 1)
            accept_byte(image_byte(base + index), index == count - 1);
        if (captured_command !== {8'h03, base} || commands != previous_commands + 1)
            $fatal(1, "latched descriptor or transaction ownership changed");
        @(posedge clk); #1;
        if (done || error || !ready || !flash_cs_n)
            $fatal(1, "completion did not pulse for exactly one edge");
    end
endtask

task immediate_command;
    input [23:0] base, count;
    input expected_error;
    integer previous_commands;
    begin
        @(negedge clk);
        previous_commands = commands;
        address = base;
        length = count;
        start = 1;
        @(posedge clk); #1;
        start = 0;
        if (!ready || !done || error !== expected_error || data_valid ||
            !flash_cs_n || flash_clk || commands != previous_commands)
            $fatal(1, "empty/invalid command touched SPI or completed incorrectly");
        @(posedge clk); #1;
        if (done || error) $fatal(1, "immediate completion did not clear");
    end
endtask

initial begin
    repeat (2) @(posedge clk);
    if (!ready || !flash_cs_n) begin
        $display("FAIL: reader did not start idle");
        $finish(1);
    end

    read_bytes(1, 3);
    immediate_command(24'hffffff, 0, 0);
    immediate_command(CAPACITY - 1, 2, 1);
    immediate_command(24'hfffffe, 4, 1);
    read_bytes(CAPACITY - 3, 3);
    read_bytes(CAPACITY - 1, 1);
    read_bytes(1, 3);
    if(minimum_half_ns > 18.519*HALF_PERIOD)
        $fatal(1,"SPI never reached its configured active frequency");
    $display("SPI_REF half_period_cycles=%0d minimum_half_ns=%0.3f output_delay_ns=6.5",HALF_PERIOD,minimum_half_ns);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    #200000;
    $display("FAIL: timeout");
    $finish(1);
end
endmodule
