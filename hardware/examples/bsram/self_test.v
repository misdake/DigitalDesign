module BsramBoardSelfTest(
    input wire clk,
    input wire [1:0] buttons,
    output wire [5:0] leds,
    output wire uart_tx
);

reg [1:0] reset_sync = 2'b00;
always @(posedge clk)
    reset_sync <= {reset_sync[0], |buttons};
wire reset = reset_sync[1];

reg [2:0] phase = 0;
reg [9:0] address = 0;
reg [9:0] checked_address = 0;
reg check_valid = 0;
reg error_sticky = 0;

wire filling = phase == 0;
wire timing_write = phase == 4;
wire writing = filling || timing_write;
wire [15:0] sp16_pattern = 16'h5aa5 ^ {6'b0, address};
wire [17:0] sp18_pattern = 18'h2a55a ^ {8'b0, address};
wire [15:0] rw16_pattern = 16'hc33c ^ {6'b0, address};
wire [17:0] rw18_pattern = 18'h13cc3 ^ {8'b0, address};

wire [15:0] sp16_read;
wire [17:0] sp18_read;
wire [15:0] r16_read;
wire [15:0] rw16_read;
wire [17:0] r18_read;
wire [17:0] rw18_read;
wire [15:0] tdp16_a_read;
wire [15:0] tdp16_b_read;
wire [17:0] tdp18_a_read;
wire [17:0] tdp18_b_read;

Bsram1Rw1024_WIDTH16 u_Bsram1Rw1024_WIDTH16(
    .clk(clk), .write_enable(writing), .address(address),
    .write_data(sp16_pattern), .read_data(sp16_read));
Bsram1Rw1024_WIDTH18 u_Bsram1Rw1024_WIDTH18(
    .clk(clk), .write_enable(writing), .address(address),
    .write_data(sp18_pattern), .read_data(sp18_read));

wire [9:0] rw_port_address = filling ? address : ~address;
Bsram1R1Rw1024_WIDTH16 u_Bsram1R1Rw1024_WIDTH16(
    .clk(clk), .read_address(address), .rw_write_enable(writing),
    .rw_address(rw_port_address), .rw_write_data(rw16_pattern),
    .read_data(r16_read), .rw_read_data(rw16_read));
Bsram1R1Rw1024_WIDTH18 u_Bsram1R1Rw1024_WIDTH18(
    .clk(clk), .read_address(address), .rw_write_enable(writing),
    .rw_address(rw_port_address), .rw_write_data(rw18_pattern),
    .read_data(r18_read), .rw_read_data(rw18_read));

wire [9:0] tdp_a_address = address;
wire [9:0] tdp_b_address = address + 10'd341;
wire [15:0] tdp16_a_pattern = 16'h1357 ^ {6'b0, tdp_a_address};
wire [15:0] tdp16_b_pattern = 16'h8462 ^ {6'b0, tdp_b_address};
wire [17:0] tdp18_a_pattern = 18'h13579 ^ {8'b0, tdp_a_address};
wire [17:0] tdp18_b_pattern = 18'h2864a ^ {8'b0, tdp_b_address};

BsramTrueDualPort1024_WIDTH16 u_BsramTrueDualPort1024_WIDTH16(
    .clk(clk),
    .a_write_enable(writing), .a_address(tdp_a_address),
    .a_write_data(tdp16_a_pattern), .a_read_data(tdp16_a_read),
    .b_write_enable(writing), .b_address(tdp_b_address),
    .b_write_data(tdp16_b_pattern), .b_read_data(tdp16_b_read));
BsramTrueDualPort1024_WIDTH18 u_BsramTrueDualPort1024_WIDTH18(
    .clk(clk),
    .a_write_enable(writing), .a_address(tdp_a_address),
    .a_write_data(tdp18_a_pattern), .a_read_data(tdp18_a_read),
    .b_write_enable(writing), .b_address(tdp_b_address),
    .b_write_data(tdp18_b_pattern), .b_read_data(tdp18_b_read));

wire [15:0] checked_sp16 = 16'h5aa5 ^ {6'b0, checked_address};
wire [17:0] checked_sp18 = 18'h2a55a ^ {8'b0, checked_address};
wire [15:0] checked_r16 = 16'hc33c ^ {6'b0, checked_address};
wire [17:0] checked_r18 = 18'h13cc3 ^ {8'b0, checked_address};
wire [9:0] checked_rw_address = ~checked_address;
wire [15:0] checked_rw16 = 16'hc33c ^ {6'b0, checked_rw_address};
wire [17:0] checked_rw18 = 18'h13cc3 ^ {8'b0, checked_rw_address};
wire [9:0] checked_tdp_b_address = checked_address + 10'd341;
wire [15:0] checked_tdp16_a = checked_address < 341 ?
    (16'h8462 ^ {6'b0, checked_address}) :
    (16'h1357 ^ {6'b0, checked_address});
wire [15:0] checked_tdp16_b = checked_tdp_b_address < 341 ?
    (16'h8462 ^ {6'b0, checked_tdp_b_address}) :
    (16'h1357 ^ {6'b0, checked_tdp_b_address});
wire [17:0] checked_tdp18_a = checked_address < 341 ?
    (18'h2864a ^ {8'b0, checked_address}) :
    (18'h13579 ^ {8'b0, checked_address});
wire [17:0] checked_tdp18_b = checked_tdp_b_address < 341 ?
    (18'h2864a ^ {8'b0, checked_tdp_b_address}) :
    (18'h13579 ^ {8'b0, checked_tdp_b_address});

wire values_match =
    sp16_read == checked_sp16 && sp18_read == checked_sp18 &&
    r16_read == checked_r16 && rw16_read == checked_rw16 &&
    r18_read == checked_r18 && rw18_read == checked_rw18 &&
    tdp16_a_read == checked_tdp16_a &&
    tdp16_b_read == checked_tdp16_b &&
    tdp18_a_read == checked_tdp18_a &&
    tdp18_b_read == checked_tdp18_b;

wire timing_values_match =
    sp16_read == (16'h5aa5 ^ 16'd10) &&
    sp18_read == (18'h2a55a ^ 18'd10) &&
    r16_read == (16'hc33c ^ 16'd20) &&
    r18_read == (18'h13cc3 ^ 18'd20) &&
    rw16_read == (16'hc33c ^ {6'b0, ~10'd10}) &&
    rw18_read == (18'h13cc3 ^ {8'b0, ~10'd10}) &&
    tdp16_a_read == (16'h8462 ^ 16'd10) &&
    tdp16_b_read == (16'h1357 ^ 16'd351) &&
    tdp18_a_read == (18'h2864a ^ 18'd10) &&
    tdp18_b_read == (18'h13579 ^ 18'd351);

always @(posedge clk) begin
    if (reset) begin
        phase <= 0;
        address <= 0;
        checked_address <= 0;
        check_valid <= 0;
        error_sticky <= 0;
    end else begin
        case (phase)
            0: begin
                if (address == 10'd1023) begin
                    phase <= 1;
                    address <= 0;
                    check_valid <= 0;
                end else begin
                    address <= address + 1'b1;
                end
            end
            1: begin
                if (check_valid && !values_match)
                    error_sticky <= 1;
                checked_address <= address;
                check_valid <= 1;
                if (address == 10'd1023)
                    phase <= 2;
                else
                    address <= address + 1'b1;
            end
            2: begin
                if (check_valid && !values_match)
                    error_sticky <= 1;
                check_valid <= 0;
                address <= 10;
                phase <= 3;
            end
            3: begin
                address <= 20;
                phase <= 4;
            end
            4: phase <= 5;
            5: begin
                if (!timing_values_match)
                    error_sticky <= 1;
                phase <= 6;
            end
            default: phase <= 6;
        endcase
    end
end

wire done = phase == 6;
assign leds = error_sticky ? 6'b100000 : (done ? 6'b000001 : 6'b000000);

reg [21:0] report_delay = 0;
reg [9:0] uart_frame = 10'h3ff;
reg [3:0] uart_bit = 0;
reg [7:0] uart_divider = 0;
reg uart_busy = 0;

always @(posedge clk) begin
    if (reset) begin
        report_delay <= 0;
        uart_frame <= 10'h3ff;
        uart_bit <= 0;
        uart_divider <= 0;
        uart_busy <= 0;
    end else if (!uart_busy) begin
        if (done && report_delay == 22'd2700000) begin
            uart_frame <= {1'b1, error_sticky ? 8'h46 : 8'h50, 1'b0};
            uart_bit <= 0;
            uart_divider <= 0;
            uart_busy <= 1;
            report_delay <= 0;
        end else if (done) begin
            report_delay <= report_delay + 1'b1;
        end
    end else if (uart_divider == 8'd233) begin
        uart_divider <= 0;
        if (uart_bit == 9)
            uart_busy <= 0;
        else
            uart_bit <= uart_bit + 1'b1;
    end else begin
        uart_divider <= uart_divider + 1'b1;
    end
end

assign uart_tx = uart_busy ? uart_frame[uart_bit] : 1'b1;

endmodule
