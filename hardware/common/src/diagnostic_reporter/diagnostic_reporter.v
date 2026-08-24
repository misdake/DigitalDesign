module {{ module_name }}(
    input wire clk,
    input wire report_enable,
    input wire [7:0] status,
    output wire uart_tx,
    output wire uart_busy,
    output wire frame_toggle
);

reg first_report = 1'b1;
reg [{{ delay_counter_high_bit }}:0] delay_counter = 0;
reg [7:0] latched_status = 0;
reg [2:0] byte_index = 0;
reg [9:0] uart_frame = 10'h3ff;
reg [3:0] uart_bit = 0;
reg [{{ uart_counter_high_bit }}:0] uart_divider = 0;
reg uart_busy_reg = 0;
reg frame_toggle_reg = 0;

function [7:0] report_byte;
    input [2:0] index;
    input [7:0] frame_status;
    begin
        case (index)
            0: report_byte = 8'h44;
            1: report_byte = 8'h44;
            2: report_byte = 8'h48;
            3: report_byte = 8'h54;
            4: report_byte = 8'h01;
            5: report_byte = 8'h{{ "{:02x}"|format(test_id) }};
            6: report_byte = frame_status;
            default: report_byte = 8'h{{ "{:02x}"|format(checksum_base) }} ^ frame_status;
        endcase
    end
endfunction

always @(posedge clk) begin
    if (!report_enable) begin
        first_report <= 1'b1;
        delay_counter <= 0;
        latched_status <= 0;
        byte_index <= 0;
        uart_frame <= 10'h3ff;
        uart_bit <= 0;
        uart_divider <= 0;
        uart_busy_reg <= 0;
    end else if (!uart_busy_reg) begin
        if (delay_counter == (first_report
                ? {{ delay_counter_width }}'d{{ first_report_delay_minus_one }}
                : {{ delay_counter_width }}'d{{ report_interval_minus_one }})) begin
            delay_counter <= 0;
            first_report <= 1'b0;
            latched_status <= status;
            byte_index <= 0;
            uart_frame <= {1'b1, report_byte(0, status), 1'b0};
            uart_bit <= 0;
            uart_divider <= 0;
            uart_busy_reg <= 1'b1;
        end else begin
            delay_counter <= delay_counter + 1'b1;
        end
    end else if (uart_divider == {{ uart_counter_width }}'d{{ clocks_per_bit_minus_one }}) begin
        uart_divider <= 0;
        if (uart_bit == 9) begin
            if (byte_index == 7) begin
                uart_busy_reg <= 1'b0;
                frame_toggle_reg <= !frame_toggle_reg;
            end else begin
                byte_index <= byte_index + 1'b1;
                uart_frame <= {1'b1, report_byte(byte_index + 1'b1, latched_status), 1'b0};
                uart_bit <= 0;
            end
        end else begin
            uart_bit <= uart_bit + 1'b1;
        end
    end else begin
        uart_divider <= uart_divider + 1'b1;
    end
end

assign uart_tx = uart_busy_reg ? uart_frame[uart_bit] : 1'b1;
assign uart_busy = uart_busy_reg;
assign frame_toggle = frame_toggle_reg;

endmodule
