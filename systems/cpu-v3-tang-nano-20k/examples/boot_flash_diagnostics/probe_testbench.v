`timescale 1ns/1ps

module tb;
reg clk = 0;
reg [1:0] buttons = 0;
reg flash_miso = 0;
wire [5:0] leds;
wire uart_tx;
wire flash_clk;
wire flash_cs_n;
wire flash_mosi;

FlashDiagnosticsProbe dut (
    .clk(clk), .buttons(buttons), .flash_miso(flash_miso),
    .leds(leds), .uart_tx(uart_tx), .flash_clk(flash_clk),
    .flash_cs_n(flash_cs_n), .flash_mosi(flash_mosi)
);

always #1 clk = !clk;

reg [7:0] command = 0;
integer command_bits = 0;
integer response_bit = 0;
integer sr1_reads = 0;
reg [7:0] response = 0;

always @(negedge flash_cs_n) begin
    command = 0;
    command_bits = 0;
    response_bit = 0;
end

always @(posedge flash_clk) begin
    if (!flash_cs_n && command_bits < 8) begin
        command = {command[6:0], flash_mosi};
        command_bits = command_bits + 1;
    end
end

always @(negedge flash_clk) begin
    if (!flash_cs_n && command_bits == 8) begin
        case (command)
            8'h9f: case (response_bit / 8)
                0: response = 8'hef;
                1: response = 8'h40;
                default: response = 8'h17;
            endcase
            8'h05: case (sr1_reads)
                0: response = 8'h00;
                1: response = 8'h02;
                default: response = 8'h00;
            endcase
            8'h35: response = 8'h02;
            8'h15: response = 8'h00;
            default: response = 8'h00;
        endcase
        flash_miso = response[7 - (response_bit % 8)];
        response_bit = response_bit + 1;
    end
end

always @(posedge flash_cs_n) begin
    if (command == 8'h05)
        sr1_reads = sr1_reads + 1;
end

reg [7:0] bytes [0:12];
integer byte_index;
integer bit_index;
integer timeout;
initial begin
    timeout = 0;
    while (!dut.complete && timeout < 3000000) begin
        @(posedge clk);
        timeout = timeout + 1;
    end
    if (!dut.complete) $fatal(1, "diagnostics did not complete");
    for (byte_index = 0; byte_index < 13; byte_index = byte_index + 1) begin
        @(negedge uart_tx);
        repeat (351) @(posedge clk);
        for (bit_index = 0; bit_index < 8; bit_index = bit_index + 1) begin
            bytes[byte_index][bit_index] = uart_tx;
            repeat (234) @(posedge clk);
        end
        repeat (117) @(posedge clk);
    end
    if (bytes[0] !== "F" || bytes[1] !== "D" || bytes[2] !== "S" || bytes[3] !== "1")
        $fatal(1, "bad record magic");
    if (bytes[4] !== 8'hef || bytes[5] !== 8'h40 || bytes[6] !== 8'h17)
        $fatal(1, "bad JEDEC ID");
    if (bytes[7] !== 0 || bytes[8] !== 2 || bytes[9] !== 0 || bytes[10] !== 2 || bytes[11] !== 0)
        $fatal(1, "bad status sequence");
    if ((bytes[0] ^ bytes[1] ^ bytes[2] ^ bytes[3] ^ bytes[4] ^ bytes[5]
        ^ bytes[6] ^ bytes[7] ^ bytes[8] ^ bytes[9] ^ bytes[10] ^ bytes[11]) !== bytes[12])
        $fatal(1, "bad checksum");
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    #20000000;
    $fatal(1, "timeout");
end
endmodule
