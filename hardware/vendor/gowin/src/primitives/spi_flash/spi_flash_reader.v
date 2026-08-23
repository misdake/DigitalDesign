module {{ module_name }} (
    input wire clk,
    input wire start,
    input wire [23:0] address,
    input wire [23:0] length,
    input wire data_ready,
    input wire flash_miso,
    output reg ready = 1'b1,
    output reg data_valid = 1'b0,
    output reg [7:0] data = 8'b0,
    output reg done = 1'b0,
    output reg error = 1'b0,
    output reg flash_clk = 1'b0,
    output reg flash_cs_n = 1'b1,
    output reg flash_mosi = 1'b0
);
localparam integer HALF_PERIOD_CYCLES = {{ half_period_cycles }};
localparam [24:0] CAPACITY_BYTES = 25'd{{ capacity_bytes }};

reg [31:0] tx_shift = 0;
reg [5:0] tx_remaining = 0;
reg [23:0] bytes_remaining = 0;
reg [31:0] divider = 0;
reg [2:0] rx_bit = 0;
reg [7:0] rx_shift = 0;
reg final_byte_buffered = 0;

always @(posedge clk) begin
    done <= 1'b0;
    error <= 1'b0;

    if (data_valid) begin
        if (data_ready) begin
            data_valid <= 1'b0;
            if (final_byte_buffered) begin
                final_byte_buffered <= 1'b0;
                ready <= 1'b1;
                done <= 1'b1;
            end
        end
    end else if (!ready) begin
        if (divider == HALF_PERIOD_CYCLES - 1) begin
            divider <= 0;
            if (!flash_clk) begin
                flash_clk <= 1'b1;
                if (tx_remaining == 0)
                    rx_shift <= {rx_shift[6:0], flash_miso};
            end else begin
                flash_clk <= 1'b0;
                if (tx_remaining != 0) begin
                    tx_shift <= {tx_shift[30:0], 1'b0};
                    tx_remaining <= tx_remaining - 1'b1;
                    if (tx_remaining == 1)
                        flash_mosi <= 1'b0;
                    else
                        flash_mosi <= tx_shift[30];
                end else if (rx_bit == 7) begin
                    rx_bit <= 0;
                    data <= rx_shift;
                    data_valid <= 1'b1;
                    if (bytes_remaining == 1) begin
                        bytes_remaining <= 0;
                        flash_cs_n <= 1'b1;
                        final_byte_buffered <= 1'b1;
                    end else begin
                        bytes_remaining <= bytes_remaining - 1'b1;
                    end
                end else begin
                    rx_bit <= rx_bit + 1'b1;
                end
            end
        end else begin
            divider <= divider + 1'b1;
        end
    end else if (start) begin
        if (length == 0) begin
            done <= 1'b1;
        end else if ({1'b0, address} + {1'b0, length} > CAPACITY_BYTES) begin
            done <= 1'b1;
            error <= 1'b1;
        end else begin
            ready <= 1'b0;
            flash_cs_n <= 1'b0;
            flash_clk <= 1'b0;
            // Standard read command followed by a 24-bit byte address.
            tx_shift <= {8'h03, address};
            tx_remaining <= 32;
            bytes_remaining <= length;
            divider <= 0;
            rx_bit <= 0;
            rx_shift <= 0;
            final_byte_buffered <= 1'b0;
            flash_mosi <= 1'b0;
        end
    end
end
endmodule
