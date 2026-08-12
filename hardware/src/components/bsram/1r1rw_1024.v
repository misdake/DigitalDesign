module {{ module_name }}(
    input wire clk,
    input wire [9:0] read_address,
    input wire rw_write_enable,
    input wire [9:0] rw_address,
    input wire [{{ high_bit }}:0] rw_write_data,
    output reg [{{ high_bit }}:0] read_data,
    output reg [{{ high_bit }}:0] rw_read_data
);

reg [{{ high_bit }}:0] memory [0:1023];

always @(posedge clk) begin
    read_data <= memory[read_address];
end

always @(posedge clk) begin
    if (rw_write_enable)
        memory[rw_address] <= rw_write_data;
    else
        rw_read_data <= memory[rw_address];
end

endmodule
