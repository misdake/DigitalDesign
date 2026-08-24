module BootDmaEngine (
    input wire clk,
    input wire reset,
    input wire start,
    input wire [23:0] flash_offset,
    input wire [21:0] destination,
    input wire [31:0] file_size_bytes,
    input wire [31:0] memory_size_bytes,
    input wire [31:0] expected_crc32,
    input wire flash_ready,
    input wire flash_data_valid,
    input wire [7:0] flash_data,
    input wire flash_done,
    input wire flash_error,
    input wire memory_request_ready,
    input wire memory_response_valid,
    input wire memory_error,
    output wire busy,
    output reg done = 1'b0,
    output reg error = 1'b0,
    output reg [7:0] error_code = 8'b0,
    output reg [31:0] actual_crc32 = 32'b0,
    output wire [31:0] completed_words,
    output wire flash_start,
    output wire [23:0] flash_address,
    output wire [23:0] flash_length,
    output wire flash_data_ready,
    output wire memory_request_valid,
    output wire memory_write,
    output wire [21:0] memory_address,
    output wire [15:0] memory_write_data,
    output wire memory_response_ready
);
localparam [7:0] ERROR_FILE_LARGER_THAN_MEMORY = 1;
localparam [7:0] ERROR_FLASH_RANGE = 2;
localparam [7:0] ERROR_MEMORY_RANGE = 3;
localparam [7:0] ERROR_FLASH_IO = 4;
localparam [7:0] ERROR_MEMORY_IO = 5;
localparam [7:0] ERROR_CRC_MISMATCH = 6;

localparam [2:0] PHASE_IDLE = 0;
localparam [2:0] PHASE_WAIT_FLASH = 1;
localparam [2:0] PHASE_STREAM = 2;
localparam [2:0] PHASE_REQUEST_MEMORY = 3;
localparam [2:0] PHASE_WAIT_MEMORY = 4;

reg [2:0] phase = PHASE_IDLE;
reg [23:0] active_flash_offset = 0;
reg [21:0] active_destination = 0;
reg [31:0] active_file_size = 0;
reg [31:0] active_memory_words = 0;
reg [31:0] active_expected_crc = 0;
reg [31:0] crc = 32'hffffffff;
reg [31:0] byte_index = 0;
reg [31:0] word_index = 0;
reg [7:0] low_byte = 0;
reg [15:0] write_data = 0;

wire [32:0] requested_memory_words =
    {1'b0, memory_size_bytes} + 1'b1 >> 1;
wire [32:0] flash_end = {9'b0, flash_offset} + {1'b0, file_size_bytes};
wire [32:0] memory_end = {11'b0, destination} + requested_memory_words;

assign busy = phase != PHASE_IDLE;
assign completed_words = word_index;
assign flash_start = phase == PHASE_WAIT_FLASH;
assign flash_address = active_flash_offset;
assign flash_length = active_file_size[23:0];
assign flash_data_ready = phase == PHASE_STREAM && byte_index < active_file_size;
assign memory_request_valid = phase == PHASE_REQUEST_MEMORY;
assign memory_write = 1'b1;
assign memory_address = active_destination + word_index[21:0];
assign memory_write_data = write_data;
assign memory_response_ready = phase == PHASE_WAIT_MEMORY;

function [31:0] crc32_byte;
    input [31:0] current;
    input [7:0] byte;
    integer bit_index;
    reg [31:0] next;
    begin
        next = current ^ byte;
        for (bit_index = 0; bit_index < 8; bit_index = bit_index + 1)
            next = (next >> 1) ^ (32'hedb88320 & (0 - next[0]));
        crc32_byte = next;
    end
endfunction

task fail;
    input [7:0] code;
    begin
        phase <= PHASE_IDLE;
        done <= 1'b0;
        error <= 1'b1;
        error_code <= code;
        actual_crc32 <= ~crc;
    end
endtask

always @(posedge clk) begin
    if (reset) begin
        phase <= PHASE_IDLE;
        done <= 1'b0;
        error <= 1'b0;
        error_code <= 0;
        actual_crc32 <= 0;
        word_index <= 0;
    end else if (phase == PHASE_IDLE) begin
        if (start) begin
            done <= 1'b0;
            error <= 1'b0;
            error_code <= 0;
            actual_crc32 <= 0;
            active_flash_offset <= flash_offset;
            active_destination <= destination;
            active_file_size <= file_size_bytes;
            active_memory_words <= requested_memory_words[31:0];
            active_expected_crc <= expected_crc32;
            crc <= 32'hffffffff;
            byte_index <= 0;
            word_index <= 0;
            low_byte <= 0;
            write_data <= 0;
            if (file_size_bytes > memory_size_bytes) begin
                error <= 1'b1;
                error_code <= ERROR_FILE_LARGER_THAN_MEMORY;
            end else if (flash_end > 33'h01000000) begin
                error <= 1'b1;
                error_code <= ERROR_FLASH_RANGE;
            end else if (memory_end > 33'h00400000) begin
                error <= 1'b1;
                error_code <= ERROR_MEMORY_RANGE;
            end else if (requested_memory_words == 0) begin
                actual_crc32 <= 0;
                if (expected_crc32 == 0) begin
                    done <= 1'b1;
                end else begin
                    error <= 1'b1;
                    error_code <= ERROR_CRC_MISMATCH;
                end
            end else begin
                phase <= file_size_bytes == 0 ? PHASE_STREAM : PHASE_WAIT_FLASH;
            end
        end
    end else if (flash_error) begin
        fail(ERROR_FLASH_IO);
    end else if (memory_error) begin
        fail(ERROR_MEMORY_IO);
    end else begin
        case (phase)
            PHASE_WAIT_FLASH: begin
                if (flash_ready)
                    phase <= PHASE_STREAM;
            end
            PHASE_STREAM: begin
                if (byte_index < active_file_size) begin
                    if (flash_data_valid) begin
                        crc <= crc32_byte(crc, flash_data);
                        byte_index <= byte_index + 1'b1;
                        if (!byte_index[0]) begin
                            low_byte <= flash_data;
                            if (byte_index + 1'b1 == active_file_size) begin
                                write_data <= {8'b0, flash_data};
                                phase <= PHASE_REQUEST_MEMORY;
                            end
                        end else begin
                            write_data <= {flash_data, low_byte};
                            phase <= PHASE_REQUEST_MEMORY;
                        end
                    end
                end else begin
                    write_data <= 0;
                    phase <= PHASE_REQUEST_MEMORY;
                end
            end
            PHASE_REQUEST_MEMORY: begin
                if (memory_request_ready)
                    phase <= PHASE_WAIT_MEMORY;
            end
            PHASE_WAIT_MEMORY: begin
                if (memory_response_valid) begin
                    word_index <= word_index + 1'b1;
                    if (word_index + 1'b1 == active_memory_words) begin
                        actual_crc32 <= ~crc;
                        phase <= PHASE_IDLE;
                        if (~crc == active_expected_crc) begin
                            done <= 1'b1;
                        end else begin
                            error <= 1'b1;
                            error_code <= ERROR_CRC_MISMATCH;
                        end
                    end else begin
                        phase <= PHASE_STREAM;
                    end
                end
            end
            default: fail(ERROR_MEMORY_IO);
        endcase
    end
end

wire unused_flash_done = flash_done;
endmodule
