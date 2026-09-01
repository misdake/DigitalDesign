module CpuV3InstructionFetchQueue (
    input wire clk,
    input wire reset,
    input wire flush,
    input wire core_request_valid,
    input wire [31:0] core_address,
    input wire core_response_ready,
    input wire memory_request_ready,
    input wire memory_response_valid,
    input wire [15:0] memory_read_data,
    input wire memory_error,
    output wire core_request_ready,
    output wire core_response_valid,
    output wire [15:0] core_read_data,
    output wire core_error,
    output wire memory_request_valid,
    output wire [31:0] memory_address,
    output wire memory_response_ready
);

localparam [2:0] QUEUE_DEPTH = 4;

reg stream_valid = 0;
reg epoch = 0;
reg [31:0] expected_core_address = 0;
reg [31:0] next_memory_address = 0;

// Four entries are intentionally flip-flops. Mapping these tiny control/data
// FIFOs into RAM16 cells would consume scarce asynchronous-read SSRAM and add
// another registered-address scheduling problem to the frontend.
(* syn_ramstyle = "registers" *) reg [15:0] queue_data [0:3];
(* syn_ramstyle = "registers" *) reg queue_error [0:3];
(* syn_ramstyle = "registers" *) reg [31:0] queue_address [0:3];
reg [1:0] queue_head = 0;
reg [1:0] queue_tail = 0;
reg [2:0] queue_count = 0;

(* syn_ramstyle = "registers" *) reg metadata_epoch [0:3];
(* syn_ramstyle = "registers" *) reg [31:0] metadata_address [0:3];
reg [1:0] metadata_head = 0;
reg [1:0] metadata_tail = 0;
reg [2:0] metadata_count = 0;

wire core_address_matches = stream_valid && core_address == expected_core_address;
wire queue_head_matches = queue_count != 0 &&
                          queue_address[queue_head] == core_address;
wire restart = core_request_valid &&
               (!core_address_matches || (queue_count != 0 && !queue_head_matches));
wire core_pop = core_request_valid && core_response_ready &&
                core_address_matches && queue_head_matches;

assign core_response_valid = core_request_valid &&
                             core_address_matches && queue_head_matches;
assign core_request_ready = core_response_valid && core_response_ready;
assign core_read_data = queue_data[queue_head];
assign core_error = queue_error[queue_head];

wire [3:0] reserved_words = queue_count + metadata_count;
assign memory_request_valid = stream_valid && !flush && !restart &&
                              reserved_words < QUEUE_DEPTH;
assign memory_address = next_memory_address;
wire memory_request_fire = memory_request_valid && memory_request_ready;

wire response_is_current = metadata_count != 0 &&
                           metadata_epoch[metadata_head] == epoch;
assign memory_response_ready = metadata_count != 0 &&
    (!response_is_current || queue_count < QUEUE_DEPTH || core_pop);
wire memory_response_fire = memory_response_valid && memory_response_ready;
wire enqueue_response = memory_response_fire && response_is_current &&
                        !flush && !restart;

wire [31:0] next_issue_address =
    {next_memory_address[31:16], next_memory_address[15:0] + 1'b1};
wire [31:0] next_expected_address =
    {expected_core_address[31:16], expected_core_address[15:0] + 1'b1};

always @(posedge clk) begin
    if (reset) begin
        stream_valid <= 0;
        epoch <= 0;
        expected_core_address <= 0;
        next_memory_address <= 0;
        queue_head <= 0;
        queue_tail <= 0;
        queue_count <= 0;
        metadata_head <= 0;
        metadata_tail <= 0;
        metadata_count <= 0;
    end else begin
        if (flush || restart) begin
            epoch <= !epoch;
            queue_head <= 0;
            queue_tail <= 0;
            queue_count <= 0;
            if (core_request_valid) begin
                stream_valid <= 1;
                expected_core_address <= core_address;
                next_memory_address <= core_address;
            end else begin
                stream_valid <= 0;
            end
        end else begin
            if (core_pop) begin
                queue_head <= queue_head + 1'b1;
                expected_core_address <= next_expected_address;
            end
            if (enqueue_response) begin
                queue_data[queue_tail] <= memory_read_data;
                queue_error[queue_tail] <= memory_error;
                queue_address[queue_tail] <= metadata_address[metadata_head];
                queue_tail <= queue_tail + 1'b1;
            end
            case ({enqueue_response, core_pop})
                2'b10: queue_count <= queue_count + 1'b1;
                2'b01: queue_count <= queue_count - 1'b1;
                default: queue_count <= queue_count;
            endcase
            if (memory_request_fire)
                next_memory_address <= next_issue_address;
        end

        if (memory_request_fire) begin
            metadata_epoch[metadata_tail] <= epoch;
            metadata_address[metadata_tail] <= next_memory_address;
            metadata_tail <= metadata_tail + 1'b1;
        end
        if (memory_response_fire)
            metadata_head <= metadata_head + 1'b1;
        case ({memory_request_fire, memory_response_fire})
            2'b10: metadata_count <= metadata_count + 1'b1;
            2'b01: metadata_count <= metadata_count - 1'b1;
            default: metadata_count <= metadata_count;
        endcase
    end
end

endmodule
