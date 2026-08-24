module {{ module_name }}(
    input wire clk,
    output wire tick
);

reg [{{ high_bit }}:0] counter = {{ width }}'d0;
reg tick_reg = 1'b0;

always @(posedge clk) begin
    if (counter == {{ width }}'d{{ terminal }}) begin
        counter <= {{ width }}'d0;
        tick_reg <= 1'b1;
    end else begin
        counter <= counter + 1'b1;
        tick_reg <= 1'b0;
    end
end

assign tick = tick_reg;

endmodule
