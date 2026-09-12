module DisplayLineBuffer(
    input wire write_clock, input wire write_enable,
    input wire [9:0] write_address, input wire [31:0] write_data,
    input wire read_clock, input wire [9:0] read_address,
    output reg [31:0] read_data = 0
);
// Three 400-pixel lines pack into 600 32-bit words = 19200 bits, so Gowin
// maps this dual-clock RAM into two 18-Kbit BSRAMs.
reg [31:0] memory [0:1023];
always @(posedge write_clock)
    if (write_enable) memory[write_address] <= write_data;
always @(posedge read_clock)
    read_data <= memory[read_address];
endmodule
