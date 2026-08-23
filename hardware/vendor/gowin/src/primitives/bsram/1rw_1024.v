module {{ module_name }}(
    input wire clk,
    input wire write_enable,
    input wire [9:0] address,
    input wire [{{ high_bit }}:0] write_data,
    output reg [{{ high_bit }}:0] read_data
);

reg [{{ high_bit }}:0] memory [0:1023];
integer init_address;

initial begin
    for (init_address = 0; init_address < 1024; init_address = init_address + 1)
        memory[init_address] = {{ image.default_literal }};
{% for word in image.overrides %}    memory[{{ word.address }}] = {{ word.literal }};
{% endfor %}end

always @(posedge clk) begin
    if (write_enable)
        memory[address] <= write_data;
    else
        read_data <= memory[address];
end

endmodule
