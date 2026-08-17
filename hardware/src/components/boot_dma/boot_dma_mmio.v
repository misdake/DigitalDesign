module BootDmaMmio (
    input wire clk,
    input wire reset,
    input wire [3:0] device_index,
    input wire [3:0] device_channel,
    input wire device_read_enable,
    input wire device_write_enable,
    input wire [15:0] device_write_data,
    input wire dma_busy,
    input wire dma_done,
    input wire dma_error,
    input wire [7:0] dma_error_code,
    input wire [31:0] dma_actual_crc32,
    input wire [31:0] dma_completed_words,
    output reg [15:0] device_read_data,
    output reg dma_start = 0,
    output reg [23:0] flash_offset = 0,
    output reg [21:0] destination = 0,
    output reg [31:0] file_size_bytes = 0,
    output reg [31:0] memory_size_bytes = 0,
    output reg [31:0] expected_crc32 = 0
);

always @* begin
    device_read_data = 0;
    if (device_read_enable && device_index == 4'd2) begin
        case (device_channel)
            1: device_read_data = dma_error ? 16'h8000 :
                                  dma_done ? 16'd2 :
                                  dma_busy ? 16'd1 : 16'd0;
            2: device_read_data = flash_offset[15:0];
            3: device_read_data = {8'b0, flash_offset[23:16]};
            4: device_read_data = destination[15:0];
            5: device_read_data = {10'b0, destination[21:16]};
            6: device_read_data = file_size_bytes[15:0];
            7: device_read_data = file_size_bytes[31:16];
            8: device_read_data = memory_size_bytes[15:0];
            9: device_read_data = memory_size_bytes[31:16];
            10: device_read_data = expected_crc32[15:0];
            11: device_read_data = expected_crc32[31:16];
            12: device_read_data = dma_actual_crc32[15:0];
            13: device_read_data = dma_actual_crc32[31:16];
            14: device_read_data = {8'b0, dma_error_code};
            15: device_read_data = dma_completed_words[15:0];
            default: device_read_data = 0;
        endcase
    end
end

always @(posedge clk) begin
    dma_start <= 0;
    if (reset) begin
        flash_offset <= 0;
        destination <= 0;
        file_size_bytes <= 0;
        memory_size_bytes <= 0;
        expected_crc32 <= 0;
    end else if (device_write_enable && device_index == 4'd2) begin
        case (device_channel)
            0: if (device_write_data == 1) dma_start <= 1;
            2: flash_offset[15:0] <= device_write_data;
            3: flash_offset[23:16] <= device_write_data[7:0];
            4: destination[15:0] <= device_write_data;
            5: destination[21:16] <= device_write_data[5:0];
            6: file_size_bytes[15:0] <= device_write_data;
            7: file_size_bytes[31:16] <= device_write_data;
            8: memory_size_bytes[15:0] <= device_write_data;
            9: memory_size_bytes[31:16] <= device_write_data;
            10: expected_crc32[15:0] <= device_write_data;
            11: expected_crc32[31:16] <= device_write_data;
            default: begin end
        endcase
    end
end
endmodule
