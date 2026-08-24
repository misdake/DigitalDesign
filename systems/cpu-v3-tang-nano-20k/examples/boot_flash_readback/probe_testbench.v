// The production probe reads the generated package length. This test limits
// the stream to eight bytes and verifies every checksummed UART record.
module tb;
localparam integer CLOCKS_PER_BIT = 234;
localparam integer READ_LENGTH = 8;

reg clk = 0;
wire [5:0] leds;
wire uart_tx;
wire flash_clk;
wire flash_cs_n;
wire flash_mosi;
reg flash_miso = 1;

FlashReadbackProbe #(.READ_LENGTH(READ_LENGTH)) dut (
    .clk(clk),
    .buttons(2'b00),
    .flash_miso(flash_miso),
    .leds(leds),
    .uart_tx(uart_tx),
    .flash_clk(flash_clk),
    .flash_cs_n(flash_cs_n),
    .flash_mosi(flash_mosi)
);

always #5 clk = ~clk;

localparam [23:0] FLASH_BASE = 24'h100000;
reg [7:0] flash_image [0:READ_LENGTH-1];
reg [31:0] flash_command = 0;
integer flash_command_bits = 0;
reg [23:0] flash_byte_address = 0;
integer flash_data_bit = 0;
reg [7:0] flash_current_byte = 0;

integer flash_init_index;
initial begin
    for (flash_init_index = 0; flash_init_index < READ_LENGTH; flash_init_index = flash_init_index + 1)
        flash_image[flash_init_index] = 8'h40 + flash_init_index;
end

always @(posedge flash_cs_n) begin
    flash_command_bits = 0;
    flash_data_bit = 0;
end

always @(posedge flash_clk) begin
    if (!flash_cs_n && flash_command_bits < 32) begin
        flash_command = {flash_command[30:0], flash_mosi};
        flash_command_bits = flash_command_bits + 1;
        if (flash_command_bits == 32)
            flash_byte_address = flash_command[23:0];
    end
end

always @(negedge flash_clk) begin
    if (!flash_cs_n && flash_command_bits >= 32) begin
        if (flash_data_bit == 0) begin
            if (flash_byte_address >= FLASH_BASE && flash_byte_address < FLASH_BASE + READ_LENGTH)
                flash_current_byte = flash_image[flash_byte_address - FLASH_BASE];
            else
                flash_current_byte = 8'hff;
            flash_byte_address = flash_byte_address + 1'b1;
        end
        flash_miso <= flash_current_byte[7 - flash_data_bit];
        flash_data_bit = (flash_data_bit + 1) % 8;
    end
end

task read_uart_byte;
    output [7:0] value;
    integer bit_index;
    begin
        @(negedge uart_tx);
        repeat (CLOCKS_PER_BIT + CLOCKS_PER_BIT / 2) @(posedge clk);
        for (bit_index = 0; bit_index < 8; bit_index = bit_index + 1) begin
            value[bit_index] = uart_tx;
            repeat (CLOCKS_PER_BIT) @(posedge clk);
        end
        if (!uart_tx) begin
            $display("FAIL: UART stop bit low");
            $finish(1);
        end
        repeat (CLOCKS_PER_BIT / 2) @(posedge clk);
    end
endtask

reg [7:0] frame [0:7];
integer record_index;
integer byte_index;
reg [7:0] checksum;
initial begin
    for (record_index = 0; record_index < READ_LENGTH; record_index = record_index + 1) begin
        for (byte_index = 0; byte_index < 8; byte_index = byte_index + 1)
            read_uart_byte(frame[byte_index]);
        if (frame[0] !== 8'h46 || frame[1] !== 8'h42 ||
            frame[2] !== 8'h52 || frame[3] !== 8'h31) begin
            $display("FAIL: bad frame magic at record %0d", record_index);
            $finish(1);
        end
        if ({frame[5], frame[4]} !== record_index || frame[6] !== 8'h40 + record_index) begin
            $display("FAIL: record %0d offset/data %02x%02x/%02x",
                record_index, frame[5], frame[4], frame[6]);
            $finish(1);
        end
        checksum = 0;
        for (byte_index = 0; byte_index < 7; byte_index = byte_index + 1)
            checksum = checksum ^ frame[byte_index];
        if (frame[7] !== checksum) begin
            $display("FAIL: bad checksum at record %0d", record_index);
            $finish(1);
        end
    end
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    repeat (3000000) @(posedge clk);
    $display("FAIL: timeout");
    $finish(1);
end
endmodule
