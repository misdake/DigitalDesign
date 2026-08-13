module SdramBoardSelfTest (
    input wire clk,
    input wire [1:0] buttons,
    input wire [31:0] sdram_read_data,
    input wire sdram_read_valid,
    input wire sdram_init_done,
    input wire sdram_command_ack,
    output wire [5:0] leds,
    output wire uart_tx,
    output reg sdram_command_valid,
    output reg [2:0] sdram_command,
    output reg sdram_precharge,
    output reg [20:0] sdram_address,
    output reg [3:0] sdram_write_mask,
    output reg [31:0] sdram_write_data,
    output wire [7:0] sdram_burst_length
);

localparam [2:0] CMD_REFRESH = 3'b001;
localparam [2:0] CMD_ACTIVE  = 3'b011;
localparam [2:0] CMD_WRITE   = 3'b100;
localparam [2:0] CMD_READ    = 3'b101;

localparam [3:0] ST_INIT         = 4'd0;
localparam [3:0] ST_ACT_W_REQ    = 4'd1;
localparam [3:0] ST_ACT_W_WAIT   = 4'd2;
localparam [3:0] ST_WRITE_REQ    = 4'd3;
localparam [3:0] ST_WRITE_DATA   = 4'd4;
localparam [3:0] ST_WRITE_WAIT   = 4'd5;
localparam [3:0] ST_HOLD         = 4'd6;
localparam [3:0] ST_REFRESH_REQ  = 4'd7;
localparam [3:0] ST_REFRESH_WAIT = 4'd8;
localparam [3:0] ST_ACT_R_REQ    = 4'd9;
localparam [3:0] ST_ACT_R_WAIT   = 4'd10;
localparam [3:0] ST_READ_REQ     = 4'd11;
localparam [3:0] ST_READ_DATA    = 4'd12;
localparam [3:0] ST_PASS         = 4'd13;
localparam [3:0] ST_ERROR        = 4'd14;

localparam [1:0] RETURN_WRITE = 2'd0;
localparam [1:0] RETURN_HOLD  = 2'd1;
localparam [1:0] RETURN_READ  = 2'd2;

reg [3:0] state = ST_INIT;
reg [1:0] refresh_return = RETURN_WRITE;
reg [5:0] line_index = 0;
reg [2:0] word_index = 0;
reg read_ack_seen = 0;
reg [11:0] refresh_counter = 0;
reg [18:0] hold_counter = 0;
reg [19:0] timeout_counter = 0;
reg [3:0] error_code = 0;
reg [1:0] button_sync = 0;

assign sdram_burst_length = 8'd7;

function [20:0] line_address;
    input [5:0] index;
    reg [1:0] bank;
    reg [10:0] row;
    reg [4:0] line_in_row;
    begin
        bank = index[5:4];
        row = {7'd0, index[3:0]};
        line_in_row = (index * 5) & 5'h1f;
        line_address = {bank, row, line_in_row, 3'b000};
    end
endfunction

function [31:0] word_pattern;
    input [20:0] base;
    input [2:0] index;
    begin
        word_pattern = 32'h6d3a_91c7 ^
                       {11'b0, base} ^
                       {base, 11'b0} ^
                       {8{index, 1'b0}};
    end
endfunction

task fail_test;
    input [3:0] code;
    begin
        error_code <= code;
        state <= ST_ERROR;
    end
endtask

always @(posedge clk) begin
    button_sync <= {button_sync[0], |buttons};
    sdram_command_valid <= 1'b0;
    if (sdram_init_done && state != ST_REFRESH_WAIT)
        refresh_counter <= refresh_counter + 1'b1;

    if (button_sync[1]) begin
        state <= ST_INIT;
        line_index <= 0;
        word_index <= 0;
        refresh_counter <= 0;
        hold_counter <= 0;
        timeout_counter <= 0;
        error_code <= 0;
    end else begin
        case (state)
            ST_INIT: begin
                sdram_command <= 3'b111;
                sdram_precharge <= 1'b1;
                sdram_address <= 0;
                sdram_write_mask <= 0;
                sdram_write_data <= 0;
                line_index <= 0;
                timeout_counter <= 0;
                if (sdram_init_done)
                    state <= ST_ACT_W_REQ;
            end

            ST_ACT_W_REQ: begin
                sdram_address <= line_address(line_index);
                sdram_command <= CMD_ACTIVE;
                sdram_precharge <= 1'b0;
                sdram_command_valid <= 1'b1;
                timeout_counter <= 0;
                state <= ST_ACT_W_WAIT;
            end

            ST_ACT_W_WAIT: begin
                if (sdram_command_ack)
                    state <= ST_WRITE_REQ;
                else if (timeout_counter == 20'hf_ffff)
                    fail_test(4'h1);
                else
                    timeout_counter <= timeout_counter + 1'b1;
            end

            ST_WRITE_REQ: begin
                sdram_address <= line_address(line_index);
                sdram_command <= CMD_WRITE;
                sdram_precharge <= 1'b1;
                sdram_write_mask <= 4'b0000;
                sdram_write_data <= word_pattern(line_address(line_index), 0);
                word_index <= 0;
                sdram_command_valid <= 1'b1;
                state <= ST_WRITE_DATA;
            end

            ST_WRITE_DATA: begin
                if (word_index == 3'd7) begin
                    timeout_counter <= 0;
                    state <= ST_WRITE_WAIT;
                end else begin
                    word_index <= word_index + 1'b1;
                    sdram_write_data <= word_pattern(
                        line_address(line_index), word_index + 1'b1);
                end
            end

            ST_WRITE_WAIT: begin
                if (sdram_command_ack) begin
                    if (line_index == 6'd63) begin
                        hold_counter <= 0;
                        state <= ST_HOLD;
                    end else begin
                        line_index <= line_index + 1'b1;
                        if (refresh_counter >= 12'd600) begin
                            refresh_return <= RETURN_WRITE;
                            state <= ST_REFRESH_REQ;
                        end else begin
                            state <= ST_ACT_W_REQ;
                        end
                    end
                end else if (timeout_counter == 20'hf_ffff) begin
                    fail_test(4'h2);
                end else begin
                    timeout_counter <= timeout_counter + 1'b1;
                end
            end

            ST_HOLD: begin
                if (refresh_counter >= 12'd600) begin
                    refresh_return <= RETURN_HOLD;
                    state <= ST_REFRESH_REQ;
                end else if (hold_counter == 19'd270_000) begin
                    line_index <= 0;
                    state <= ST_ACT_R_REQ;
                end else begin
                    hold_counter <= hold_counter + 1'b1;
                end
            end

            ST_REFRESH_REQ: begin
                sdram_command <= CMD_REFRESH;
                sdram_precharge <= 1'b0;
                sdram_command_valid <= 1'b1;
                timeout_counter <= 0;
                state <= ST_REFRESH_WAIT;
            end

            ST_REFRESH_WAIT: begin
                if (sdram_command_ack) begin
                    refresh_counter <= 0;
                    case (refresh_return)
                        RETURN_WRITE: state <= ST_ACT_W_REQ;
                        RETURN_HOLD: state <= ST_HOLD;
                        default: state <= ST_ACT_R_REQ;
                    endcase
                end else if (timeout_counter == 20'hf_ffff) begin
                    fail_test(4'h3);
                end else begin
                    timeout_counter <= timeout_counter + 1'b1;
                end
            end

            ST_ACT_R_REQ: begin
                sdram_address <= line_address(line_index);
                sdram_command <= CMD_ACTIVE;
                sdram_precharge <= 1'b0;
                sdram_command_valid <= 1'b1;
                timeout_counter <= 0;
                state <= ST_ACT_R_WAIT;
            end

            ST_ACT_R_WAIT: begin
                if (sdram_command_ack)
                    state <= ST_READ_REQ;
                else if (timeout_counter == 20'hf_ffff)
                    fail_test(4'h4);
                else
                    timeout_counter <= timeout_counter + 1'b1;
            end

            ST_READ_REQ: begin
                sdram_address <= line_address(line_index);
                sdram_command <= CMD_READ;
                sdram_precharge <= 1'b1;
                sdram_write_mask <= 0;
                sdram_command_valid <= 1'b1;
                word_index <= 0;
                read_ack_seen <= 0;
                timeout_counter <= 0;
                state <= ST_READ_DATA;
            end

            ST_READ_DATA: begin
                if (sdram_command_ack)
                    read_ack_seen <= 1'b1;
                if (sdram_read_valid) begin
                    if (sdram_read_data !=
                        word_pattern(line_address(line_index), word_index)) begin
                        fail_test(4'h6);
                    end else if (word_index == 3'd7) begin
                        if (!(read_ack_seen || sdram_command_ack)) begin
                            fail_test(4'h7);
                        end else if (line_index == 6'd63) begin
                            state <= ST_PASS;
                        end else begin
                            line_index <= line_index + 1'b1;
                            if (refresh_counter >= 12'd600) begin
                                refresh_return <= RETURN_READ;
                                state <= ST_REFRESH_REQ;
                            end else begin
                                state <= ST_ACT_R_REQ;
                            end
                        end
                    end else begin
                        word_index <= word_index + 1'b1;
                    end
                end else if (timeout_counter == 20'hf_ffff) begin
                    fail_test(4'h5);
                end else begin
                    timeout_counter <= timeout_counter + 1'b1;
                end
            end

            ST_PASS: state <= ST_PASS;
            ST_ERROR: state <= ST_ERROR;
            default: fail_test(4'hf);
        endcase
    end
end

assign leds = state == ST_PASS ? 6'b000001 :
              state == ST_ERROR ? {1'b1, 1'b0, error_code} :
              6'b001100;

wire test_done = state == ST_PASS || state == ST_ERROR;
reg [24:0] report_delay = 0;
reg [9:0] uart_frame = 10'h3ff;
reg [3:0] uart_bit = 0;
reg [8:0] uart_divider = 0;
reg [3:0] report_byte_index = 0;
reg uart_busy = 0;

function [7:0] report_byte;
    input [3:0] index;
    reg [7:0] status;
    begin
        status = state == ST_PASS ? 8'h00 : {4'h0, error_code};
        case (index)
            0: report_byte = 8'h44; // D
            1: report_byte = 8'h44; // D
            2: report_byte = 8'h48; // H
            3: report_byte = 8'h54; // T
            4: report_byte = 8'h01;
            5: report_byte = 8'h03; // SDRAM self-test
            6: report_byte = status;
            default: report_byte = 8'h1e ^ status;
        endcase
    end
endfunction

always @(posedge clk) begin
    if (!test_done) begin
        report_delay <= 0;
        uart_frame <= 10'h3ff;
        uart_bit <= 0;
        uart_divider <= 0;
        report_byte_index <= 0;
        uart_busy <= 0;
    end else if (!uart_busy) begin
        if (report_delay == 25'd27_000_000) begin
            uart_frame <= {1'b1, report_byte(0), 1'b0};
            uart_bit <= 0;
            uart_divider <= 0;
            report_byte_index <= 0;
            uart_busy <= 1;
            report_delay <= 0;
        end else begin
            report_delay <= report_delay + 1'b1;
        end
    end else if (uart_divider == 9'd468) begin
        uart_divider <= 0;
        if (uart_bit == 9) begin
            if (report_byte_index == 7) begin
                uart_busy <= 0;
            end else begin
                report_byte_index <= report_byte_index + 1'b1;
                uart_frame <= {1'b1, report_byte(report_byte_index + 1'b1), 1'b0};
                uart_bit <= 0;
            end
        end else begin
            uart_bit <= uart_bit + 1'b1;
        end
    end else begin
        uart_divider <= uart_divider + 1'b1;
    end
end

assign uart_tx = uart_busy ? uart_frame[uart_bit] : 1'b1;

endmodule
