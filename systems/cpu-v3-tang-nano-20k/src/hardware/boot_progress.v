module BootProgressMonitor(
    input wire clk,
    input wire reset,
    input wire sdram_ready,
    input wire dma_busy,
    input wire dma_error,
    input wire cpu_fault,
    input wire [15:0] code_segment,
    input wire software_led_write,
    output wire diagnostic_active,
    output reg [5:0] diagnostic_leds,
    output reg [2:0] phase,
    output wire error_sticky
);

reg software_leds_seen = 1'b0;
reg error_sticky_reg = 1'b0;

always @(posedge clk) begin
    if (reset) begin
        software_leds_seen <= 1'b0;
        error_sticky_reg <= 1'b0;
    end else begin
        if (software_led_write)
            software_leds_seen <= 1'b1;
        if (dma_error || cpu_fault)
            error_sticky_reg <= 1'b1;
    end
end

always @* begin
    if (reset)
        phase = 3'd0;
    else if (error_sticky_reg || dma_error || cpu_fault)
        phase = 3'd7;
    else if (!sdram_ready)
        phase = 3'd1;
    else if (code_segment == 0 && dma_busy)
        phase = 3'd3;
    else if (code_segment == 0)
        phase = 3'd2;
    else if (code_segment == 1)
        phase = 3'd4;
    else
        phase = 3'd5;

    case (phase)
        0: diagnostic_leds = 6'b000001;
        1: diagnostic_leds = 6'b000010;
        2: diagnostic_leds = 6'b000100;
        3: diagnostic_leds = 6'b001000;
        4: diagnostic_leds = 6'b010000;
        5: diagnostic_leds = 6'b100000;
        default: diagnostic_leds = 6'b100001;
    endcase
end

assign diagnostic_active = !software_leds_seen;
assign error_sticky = error_sticky_reg;

endmodule
