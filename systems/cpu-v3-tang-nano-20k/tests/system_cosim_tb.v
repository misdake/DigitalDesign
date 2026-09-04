// System-level emulator-vs-RTL co-simulation testbench for the CpuV3 Tang Nano
// 20K system: core, instruction fetch queue, two-way I-cache, D-cache, memory
// arbiter, and a behavioral SDRAM word port that is cycle-identical to the
// Rust `SdramModel` in `system_emu/mod.rs`.
//
// Placeholders (module names, program image, check region, cycle budgets) are
// substituted by the Rust runner in `system_cosim.rs`.
module tb;
reg clk = 0;
reg reset = 1;
reg clean_all = 0;

// core <-> fetch queue
wire core_instruction_request_valid;
wire [31:0] core_instruction_address;
wire core_instruction_response_ready;
wire fetch_core_request_ready;
wire fetch_core_response_valid;
wire [15:0] fetch_core_read_data;
wire fetch_core_error;

// fetch queue <-> I-cache
wire fetch_memory_request_valid;
wire [31:0] fetch_memory_address;
wire fetch_memory_response_ready;
wire ic_cpu_request_ready;
wire ic_cpu_response_valid;
wire [15:0] ic_cpu_read_data;
wire ic_cpu_error;
wire fetch_prefetch_request_valid;
wire [31:0] fetch_prefetch_address;
wire fetch_prefetch_cancel;

// I-cache <-> arbiter
wire ic_memory_request_valid;
wire ic_memory_write;
wire ic_memory_line;
wire [21:0] ic_memory_address;
wire [63:0] ic_memory_write_data;
wire ic_memory_response_ready;
wire arb_instruction_request_ready;
wire arb_instruction_response_valid;
wire [63:0] arb_instruction_read_data;
wire arb_instruction_error;
wire [31:0] ic_prefetch_issued;
wire [31:0] ic_prefetch_useful;
wire [31:0] ic_prefetch_useless;
wire [31:0] ic_prefetch_dropped;

// core <-> D-cache
wire core_data_request_valid;
wire core_data_write;
wire [31:0] core_data_address;
wire [15:0] core_data_write_data;
wire core_data_response_ready;
wire dc_cpu_request_ready;
wire dc_cpu_response_valid;
wire [15:0] dc_cpu_read_data;
wire dc_cpu_error;

// D-cache <-> arbiter
wire dc_memory_request_valid;
wire dc_memory_write;
wire dc_memory_line;
wire [21:0] dc_memory_address;
wire [63:0] dc_memory_write_data;
wire dc_memory_response_ready;
wire arb_data_request_ready;
wire arb_data_response_valid;
wire [63:0] arb_data_read_data;
wire arb_data_error;
wire dc_maintenance_busy;
wire dc_maintenance_done;
wire dc_maintenance_error;

// arbiter <-> SDRAM
wire arb_memory_request_valid;
wire arb_memory_write;
wire arb_memory_line;
wire [21:0] arb_memory_address;
wire [63:0] arb_memory_write_data;
wire arb_memory_response_ready;
wire sdram_request_ready;
reg sdram_response_valid = 0;
reg [63:0] sdram_read_data = 0;
reg sdram_response_last = 0;

// core status / device ports
wire [2:0] core_device_index;
wire [3:0] core_device_channel;
wire core_device_read_enable;
wire core_device_write_enable;
wire [15:0] core_device_write_data;
wire halted;
wire [15:0] halt_signal;
wire fault;
wire [7:0] fault_code;
wire [15:0] fault_pc;
wire [15:0] pc;
wire [15:0] code_segment;
wire [15:0] data_segment;
wire [31:0] retired_words;

// arbiter DMA port (tied idle)
wire arb_dma_request_ready;
wire arb_dma_response_valid;
wire [15:0] arb_dma_read_data;
wire arb_dma_error;

__CORE__ u_core (
    .clk(clk),
    .reset(reset),
    .hold(1'b0),
    .instruction_request_ready(fetch_core_request_ready),
    .instruction_response_valid(fetch_core_response_valid),
    .instruction_data(fetch_core_read_data),
    .instruction_error(fetch_core_error),
    .data_request_ready(dc_cpu_request_ready),
    .data_response_valid(dc_cpu_response_valid),
    .data_read_data(dc_cpu_read_data),
    .data_error(dc_cpu_error),
    .device_read_data(16'h0000),
    .instruction_request_valid(core_instruction_request_valid),
    .instruction_address(core_instruction_address),
    .instruction_response_ready(core_instruction_response_ready),
    .data_request_valid(core_data_request_valid),
    .data_write(core_data_write),
    .data_address(core_data_address),
    .data_write_data(core_data_write_data),
    .data_response_ready(core_data_response_ready),
    .device_index(core_device_index),
    .device_channel(core_device_channel),
    .device_read_enable(core_device_read_enable),
    .device_write_enable(core_device_write_enable),
    .device_write_data(core_device_write_data),
    .halted(halted),
    .halt_signal(halt_signal),
    .fault(fault),
    .fault_code(fault_code),
    .fault_pc(fault_pc),
    .pc(pc),
    .code_segment(code_segment),
    .data_segment(data_segment),
    .retired_words(retired_words)
);

__FETCH__ u_fetch (
    .clk(clk),
    .reset(reset),
    .flush(1'b0),
    .core_request_valid(core_instruction_request_valid),
    .core_address(core_instruction_address),
    .core_response_ready(core_instruction_response_ready),
    .memory_request_ready(ic_cpu_request_ready),
    .memory_response_valid(ic_cpu_response_valid),
    .memory_read_data(ic_cpu_read_data),
    .memory_error(ic_cpu_error),
    .core_request_ready(fetch_core_request_ready),
    .core_response_valid(fetch_core_response_valid),
    .core_read_data(fetch_core_read_data),
    .core_error(fetch_core_error),
    .memory_request_valid(fetch_memory_request_valid),
    .memory_address(fetch_memory_address),
    .memory_response_ready(fetch_memory_response_ready),
    .prefetch_request_valid(fetch_prefetch_request_valid),
    .prefetch_address(fetch_prefetch_address),
    .prefetch_cancel(fetch_prefetch_cancel)
);

__ICACHE__ u_icache (
    .clk(clk),
    .reset(reset),
    .invalidate_all(1'b0),
    .prefetch_request_valid(fetch_prefetch_request_valid),
    .prefetch_address(fetch_prefetch_address),
    .prefetch_cancel(fetch_prefetch_cancel),
    .cpu_request_valid(fetch_memory_request_valid),
    .cpu_write(1'b0),
    .cpu_address(fetch_memory_address),
    .cpu_write_data(16'h0000),
    .cpu_response_ready(fetch_memory_response_ready),
    .memory_request_ready(arb_instruction_request_ready),
    .memory_response_valid(arb_instruction_response_valid),
    .memory_read_data(arb_instruction_read_data),
    .memory_error(arb_instruction_error),
    .cpu_request_ready(ic_cpu_request_ready),
    .cpu_response_valid(ic_cpu_response_valid),
    .cpu_read_data(ic_cpu_read_data),
    .cpu_error(ic_cpu_error),
    .memory_request_valid(ic_memory_request_valid),
    .memory_write(ic_memory_write),
    .memory_line(ic_memory_line),
    .memory_address(ic_memory_address),
    .memory_write_data(ic_memory_write_data),
    .memory_response_ready(ic_memory_response_ready),
    .prefetch_issued(ic_prefetch_issued),
    .prefetch_useful(ic_prefetch_useful),
    .prefetch_useless(ic_prefetch_useless),
    .prefetch_dropped(ic_prefetch_dropped)
);

__DCACHE__ u_dcache (
    .clk(clk),
    .reset(reset),
    .clean_all(clean_all),
    .invalidate_all(1'b0),
    .cpu_request_valid(core_data_request_valid),
    .cpu_write(core_data_write),
    .cpu_address(core_data_address),
    .cpu_write_data(core_data_write_data),
    .cpu_response_ready(core_data_response_ready),
    .memory_request_ready(arb_data_request_ready),
    .memory_response_valid(arb_data_response_valid),
    .memory_read_data(arb_data_read_data),
    .memory_error(arb_data_error),
    .cpu_request_ready(dc_cpu_request_ready),
    .cpu_response_valid(dc_cpu_response_valid),
    .cpu_read_data(dc_cpu_read_data),
    .cpu_error(dc_cpu_error),
    .memory_request_valid(dc_memory_request_valid),
    .memory_write(dc_memory_write),
    .memory_line(dc_memory_line),
    .memory_address(dc_memory_address),
    .memory_write_data(dc_memory_write_data),
    .memory_response_ready(dc_memory_response_ready),
    .maintenance_busy(dc_maintenance_busy),
    .maintenance_done(dc_maintenance_done),
    .maintenance_error(dc_maintenance_error)
);

__ARBITER__ u_arbiter (
    .clk(clk),
    .reset(reset),
    .instruction_request_valid(ic_memory_request_valid),
    .instruction_address(ic_memory_address),
    .instruction_response_ready(ic_memory_response_ready),
    .data_request_valid(dc_memory_request_valid),
    .data_write(dc_memory_write),
    .data_line(dc_memory_line),
    .data_address(dc_memory_address),
    .data_write_data(dc_memory_write_data),
    .data_response_ready(dc_memory_response_ready),
    .dma_request_valid(1'b0),
    .dma_write(1'b0),
    .dma_address(22'h0),
    .dma_write_data(16'h0000),
    .dma_response_ready(1'b0),
    .memory_request_ready(sdram_request_ready),
    .memory_response_valid(sdram_response_valid),
    .memory_read_data(sdram_read_data),
    .memory_response_last(sdram_response_last),
    .memory_error(1'b0),
    .instruction_request_ready(arb_instruction_request_ready),
    .instruction_response_valid(arb_instruction_response_valid),
    .instruction_read_data(arb_instruction_read_data),
    .instruction_error(arb_instruction_error),
    .data_request_ready(arb_data_request_ready),
    .data_response_valid(arb_data_response_valid),
    .data_read_data(arb_data_read_data),
    .data_error(arb_data_error),
    .dma_request_ready(arb_dma_request_ready),
    .dma_response_valid(arb_dma_response_valid),
    .dma_read_data(arb_dma_read_data),
    .dma_error(arb_dma_error),
    .memory_request_valid(arb_memory_request_valid),
    .memory_write(arb_memory_write),
    .memory_line(arb_memory_line),
    .memory_address(arb_memory_address),
    .memory_write_data(arb_memory_write_data),
    .memory_response_ready(arb_memory_response_ready)
);

// Behavioral SDRAM word port, a literal port of the Rust `SdramModel`:
// refresh due every 600 clocks; a line read costs ACTIVE + READ + four 64-bit
// beats + three recovery clocks.
localparam ST_IDLE = 0;
localparam ST_WRITE_CAPTURE = 1;
localparam ST_WRITE_STAGE = 2;
localparam ST_ACTIVE_REQ = 3;
localparam ST_ACTIVE_WAIT = 4;
localparam ST_OP_REQ = 5;
localparam ST_OP_WAIT = 6;
localparam ST_CPU_RESPONSE = 7;
localparam ST_RECOVERY = 8;
localparam ST_REFRESH_REQ = 9;
localparam ST_REFRESH_WAIT = 10;

reg [3:0] sdram_state = ST_IDLE;
reg [15:0] refresh_count = 0;
reg pending_write = 0;
reg pending_line = 0;
reg [21:0] pending_address = 0;
reg [63:0] pending_write_data = 0;
reg [63:0] line_write_buffer [0:3];
reg [2:0] beat = 0;
reg [7:0] read_delay = 0;
reg [7:0] recovery_count = 0;

reg [15:0] memory [0:65535];

wire refresh_due = refresh_count >= 600;
assign sdram_request_ready = sdram_state == ST_IDLE && refresh_count < 600;

integer beat_index;
always @(posedge clk) begin
    case (sdram_state)
        ST_IDLE: begin
            sdram_response_valid <= 0;
            if (refresh_due) begin
                sdram_state <= ST_REFRESH_REQ;
            end else if (arb_memory_request_valid) begin
                pending_write <= arb_memory_write;
                pending_line <= arb_memory_line;
                pending_address <= arb_memory_address;
                pending_write_data <= arb_memory_write_data;
                if (arb_memory_write && arb_memory_line) begin
                    line_write_buffer[0] <= arb_memory_write_data;
                    beat <= 1;
                    sdram_state <= ST_WRITE_CAPTURE;
                end else begin
                    sdram_state <= ST_ACTIVE_REQ;
                end
            end
        end
        ST_WRITE_CAPTURE: begin
            line_write_buffer[beat] <= arb_memory_write_data;
            if (beat == 3) begin
                beat <= 0;
                sdram_state <= ST_WRITE_STAGE;
            end else begin
                beat <= beat + 1;
            end
        end
        ST_WRITE_STAGE: begin
            if (beat == 3) begin
                beat <= 0;
                sdram_state <= ST_ACTIVE_REQ;
            end else begin
                beat <= beat + 1;
            end
        end
        ST_ACTIVE_REQ: sdram_state <= ST_ACTIVE_WAIT;
        ST_ACTIVE_WAIT: sdram_state <= ST_OP_REQ;
        ST_OP_REQ: begin
            if (pending_write) begin
                sdram_state <= ST_OP_WAIT;
            end else begin
                read_delay <= 2;
                beat <= 0;
                sdram_state <= ST_OP_WAIT;
            end
        end
        ST_OP_WAIT: begin
            if (pending_write) begin
                if (pending_line) begin
                    for (beat_index = 0; beat_index < 4; beat_index = beat_index + 1) begin
                        memory[pending_address + 4 * beat_index] <= line_write_buffer[beat_index][15:0];
                        memory[pending_address + 4 * beat_index + 1] <= line_write_buffer[beat_index][31:16];
                        memory[pending_address + 4 * beat_index + 2] <= line_write_buffer[beat_index][47:32];
                        memory[pending_address + 4 * beat_index + 3] <= line_write_buffer[beat_index][63:48];
                    end
                end else begin
                    memory[pending_address] <= pending_write_data[15:0];
                end
                sdram_response_valid <= 1;
                sdram_read_data <= 0;
                sdram_response_last <= 1;
                sdram_state <= ST_CPU_RESPONSE;
            end else if (read_delay != 0) begin
                read_delay <= read_delay - 1;
            end else begin
                sdram_read_data <= {memory[pending_address + 4 * beat + 3],
                                    memory[pending_address + 4 * beat + 2],
                                    memory[pending_address + 4 * beat + 1],
                                    memory[pending_address + 4 * beat]};
                sdram_response_valid <= 1;
                sdram_response_last <= beat == 3;
                if (beat == 3) begin
                    recovery_count <= 0;
                    sdram_state <= ST_RECOVERY;
                end else begin
                    beat <= beat + 1;
                end
            end
        end
        ST_CPU_RESPONSE: begin
            sdram_response_valid <= 0;
            recovery_count <= 0;
            sdram_state <= ST_RECOVERY;
        end
        ST_RECOVERY: begin
            sdram_response_valid <= 0;
            if (recovery_count == 3) begin
                sdram_state <= ST_IDLE;
            end else begin
                recovery_count <= recovery_count + 1;
            end
        end
        ST_REFRESH_REQ: sdram_state <= ST_REFRESH_WAIT;
        ST_REFRESH_WAIT: begin
            refresh_count <= 0;
            sdram_state <= ST_IDLE;
        end
    endcase

    if (sdram_state != ST_REFRESH_WAIT && !refresh_due) begin
        refresh_count <= refresh_count + 1;
    end
end

always #5 clk = ~clk;

integer init_index;
integer cycles;
integer dump_index;
reg started;
reg end_flag;

initial begin
    for (init_index = 0; init_index < 65536; init_index = init_index + 1)
        memory[init_index] = 16'h0000;
    __MEMORY_INIT__
    repeat (2) @(posedge clk);
    #1 reset = 0;
    cycles = 0;
    started = 0;
    end_flag = 0;
    while (cycles < __MAX_CYCLES__ && !end_flag) begin
        #1;
        if (started || core_instruction_request_valid)
            started = 1;
        if (started) begin
            $display("CORE %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d", cycles, pc, code_segment, data_segment, retired_words, halted, halt_signal, fault, fault_code, fault_pc, core_instruction_request_valid, core_instruction_address, core_instruction_response_ready, core_data_request_valid, core_data_write, core_data_address, core_data_write_data, core_data_response_ready);
            if (halted || fault) end_flag = 1;
        end
        @(posedge clk);
        cycles = cycles + 1;
    end
    if (!halted) begin
        $display("NO_HALT");
    end else begin
        // Post-halt D-cache clean: pulse clean_all for exactly one clock, then
        // wait for maintenance_done so every dirty line is written back.
        clean_all = 1;
        @(posedge clk);
        #1 clean_all = 0;
        while (!dc_maintenance_done) begin
            @(posedge clk);
            #1;
        end
        for (dump_index = __CHECK_BASE__; dump_index < __CHECK_BASE__ + __CHECK_LEN__; dump_index = dump_index + 1)
            $display("MEM %0d %04x", dump_index, memory[dump_index]);
    end
    $display("TRACE_END");
    $finish;
end

initial begin
    repeat (__TIMEOUT_CYCLES__) @(posedge clk);
    $display("TIMEOUT");
    $finish(1);
end
endmodule
