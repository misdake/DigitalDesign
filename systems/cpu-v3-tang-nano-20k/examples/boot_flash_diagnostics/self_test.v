// Non-destructive SPI NOR diagnostics. The probe performs:
//   9Fh JEDEC ID, 05h/35h/15h status reads, 06h WREN, 05h, 04h WRDI, 05h.
// It then repeats a 13-byte UART record:
//   "FDS1", JEDEC[2:0], SR1, SR2, SR3, SR1-after-WREN,
//   SR1-after-WRDI, XOR checksum.
module FlashDiagnosticsProbe (
    input wire clk,
    input wire [1:0] buttons,
    input wire flash_miso,
    output wire [5:0] leds,
    output wire uart_tx,
    output reg flash_clk = 1'b0,
    output reg flash_cs_n = 1'b1,
    output reg flash_mosi = 1'b0
);

reg [31:0] startup = 0;
reg started = 0;
reg complete = 0;
reg [3:0] operation = 0;
reg spi_active = 0;
reg [2:0] divider = 0;
reg [7:0] tx_shift = 0;
reg [3:0] tx_bits = 0;
reg [1:0] read_total = 0;
reg [1:0] read_index = 0;
reg [2:0] rx_bit = 0;
reg [7:0] rx_shift = 0;
reg finish_on_falling_edge = 0;

reg [7:0] jedec0 = 0;
reg [7:0] jedec1 = 0;
reg [7:0] jedec2 = 0;
reg [7:0] sr1_before = 0;
reg [7:0] sr2 = 0;
reg [7:0] sr3 = 0;
reg [7:0] sr1_wren = 0;
reg [7:0] sr1_wrdi = 0;

function [7:0] command_for;
    input [3:0] index;
    begin
        case (index)
            0: command_for = 8'h9f;
            1: command_for = 8'h05;
            2: command_for = 8'h35;
            3: command_for = 8'h15;
            4: command_for = 8'h06;
            5: command_for = 8'h05;
            6: command_for = 8'h04;
            default: command_for = 8'h05;
        endcase
    end
endfunction

function [1:0] read_count_for;
    input [3:0] index;
    begin
        if (index == 0)
            read_count_for = 3;
        else if (index == 4 || index == 6)
            read_count_for = 0;
        else
            read_count_for = 1;
    end
endfunction

task save_byte;
    input [3:0] op;
    input [1:0] index;
    input [7:0] value;
    begin
        case (op)
            0: case (index)
                0: jedec0 <= value;
                1: jedec1 <= value;
                default: jedec2 <= value;
            endcase
            1: sr1_before <= value;
            2: sr2 <= value;
            3: sr3 <= value;
            5: sr1_wren <= value;
            7: sr1_wrdi <= value;
        endcase
    end
endtask

reg uart_busy = 0;
reg [3:0] uart_byte = 0;
reg [9:0] uart_frame = 10'h3ff;
reg [3:0] uart_bit = 0;
reg [7:0] uart_divider = 0;
reg [23:0] repeat_delay = 0;

function [7:0] record_byte;
    input [3:0] index;
    begin
        case (index)
            0: record_byte = 8'h46; // F
            1: record_byte = 8'h44; // D
            2: record_byte = 8'h53; // S
            3: record_byte = 8'h31; // 1
            4: record_byte = jedec0;
            5: record_byte = jedec1;
            6: record_byte = jedec2;
            7: record_byte = sr1_before;
            8: record_byte = sr2;
            9: record_byte = sr3;
            10: record_byte = sr1_wren;
            11: record_byte = sr1_wrdi;
            default: record_byte = 8'h60 ^ jedec0 ^ jedec1 ^ jedec2
                ^ sr1_before ^ sr2 ^ sr3 ^ sr1_wren ^ sr1_wrdi;
        endcase
    end
endfunction

always @(posedge clk) begin
    if (|buttons) begin
        startup <= 0;
        started <= 0;
        complete <= 0;
        operation <= 0;
        spi_active <= 0;
        flash_clk <= 0;
        flash_cs_n <= 1;
        flash_mosi <= 0;
        uart_busy <= 0;
        repeat_delay <= 0;
    end else begin
        if (!started) begin
            if (startup == 32'd999999) begin
                started <= 1;
                operation <= 0;
            end else begin
                startup <= startup + 1'b1;
            end
        end else if (!complete && !spi_active) begin
            if (operation == 8) begin
                complete <= 1;
            end else begin
                spi_active <= 1;
                flash_cs_n <= 0;
                flash_clk <= 0;
                tx_shift <= command_for(operation);
                tx_bits <= 8;
                read_total <= read_count_for(operation);
                read_index <= 0;
                rx_bit <= 0;
                rx_shift <= 0;
                finish_on_falling_edge <= 0;
                flash_mosi <= command_for(operation) >> 7;
                divider <= 0;
            end
        end else if (spi_active) begin
            if (divider == 3'd1) begin
                divider <= 0;
                if (!flash_clk) begin
                    flash_clk <= 1;
                    if (tx_bits == 0) begin
                        rx_shift <= {rx_shift[6:0], flash_miso};
                        if (rx_bit == 7) begin
                            save_byte(operation, read_index, {rx_shift[6:0], flash_miso});
                            rx_bit <= 0;
                            if (read_index + 1'b1 == read_total)
                                finish_on_falling_edge <= 1;
                            else
                                read_index <= read_index + 1'b1;
                        end else begin
                            rx_bit <= rx_bit + 1'b1;
                        end
                    end
                end else begin
                    flash_clk <= 0;
                    if (finish_on_falling_edge) begin
                        flash_cs_n <= 1;
                        flash_mosi <= 0;
                        spi_active <= 0;
                        operation <= operation + 1'b1;
                        finish_on_falling_edge <= 0;
                    end else if (tx_bits != 0) begin
                        tx_bits <= tx_bits - 1'b1;
                        tx_shift <= {tx_shift[6:0], 1'b0};
                        if (tx_bits == 1) begin
                            flash_mosi <= 0;
                            if (read_total == 0) begin
                                flash_cs_n <= 1;
                                spi_active <= 0;
                                operation <= operation + 1'b1;
                            end
                        end else begin
                            flash_mosi <= tx_shift[6];
                        end
                    end
                end
            end else begin
                divider <= divider + 1'b1;
            end
        end

        if (complete && !uart_busy) begin
            if (repeat_delay == 24'd999999) begin
                repeat_delay <= 0;
                uart_byte <= 0;
                uart_frame <= {1'b1, record_byte(0), 1'b0};
                uart_bit <= 0;
                uart_divider <= 0;
                uart_busy <= 1;
            end else begin
                repeat_delay <= repeat_delay + 1'b1;
            end
        end else if (uart_busy && uart_divider == 8'd233) begin
            uart_divider <= 0;
            if (uart_bit == 9) begin
                if (uart_byte == 12) begin
                    uart_busy <= 0;
                end else begin
                    uart_byte <= uart_byte + 1'b1;
                    uart_frame <= {1'b1, record_byte(uart_byte + 1'b1), 1'b0};
                    uart_bit <= 0;
                end
            end else begin
                uart_bit <= uart_bit + 1'b1;
            end
        end else if (uart_busy) begin
            uart_divider <= uart_divider + 1'b1;
        end
    end
end

assign uart_tx = uart_busy ? uart_frame[uart_bit] : 1'b1;
assign leds = {complete, uart_busy, sr1_wren[1], sr1_wrdi[1], sr1_before[1], started};

endmodule
