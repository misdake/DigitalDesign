module {{ module_name }}(
    input wire clk,
    input wire write_enable,
    input wire [9:0] address,
    input wire [{{ high_bit }}:0] write_data,
    output reg [{{ high_bit }}:0] read_data
);

reg [{{ high_bit }}:0] memory [0:1023];

always @(posedge clk) begin
    if (write_enable)
        memory[address] <= write_data;
    else
        read_data <= memory[address];
end

endmodule
