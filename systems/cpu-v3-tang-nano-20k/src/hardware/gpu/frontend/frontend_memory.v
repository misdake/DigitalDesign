// Standalone frontend storage and asynchronous long-request transport.
// Addresses on this private interface are bytes; external beats are 64 bits.
`ifdef FRONTEND_VENDOR_SIM
`undef __ICARUS__
`endif
module FrontendScratchpad(input wire clk,
    input wire [9:0] read_address, output wire [63:0] read_data,
    input wire write_enable, input wire [9:0] write_address,
    input wire [63:0] write_data);
`ifdef __ICARUS__
    reg [63:0] memory [0:1023];
    reg [63:0] data;
    assign read_data = data;
    always @(posedge clk) begin
        data <= memory[read_address];
        if (write_enable) memory[write_address] <= write_data;
    end
`else
    genvar lane;
    generate for (lane=0; lane<4; lane=lane+1) begin: lanes
        wire [15:0] unused;
        DPB #(.BIT_WIDTH_0(16), .BIT_WIDTH_1(16),
            .READ_MODE0(1'b0), .READ_MODE1(1'b0),
            .WRITE_MODE0(2'b00), .WRITE_MODE1(2'b00)) ram (
            .CLKA(clk), .CLKB(clk), .CEA(1'b1), .CEB(1'b1),
            .OCEA(1'b0), .OCEB(1'b0), .RESETA(1'b0), .RESETB(1'b0),
            .WREA(1'b0), .WREB(write_enable), .BLKSELA(3'b0), .BLKSELB(3'b0),
            .ADA({read_address,2'b00,2'b11}),
            .ADB({write_address,2'b00,2'b11}), .DIA(16'b0),
            .DIB(write_data[lane*16+:16]), .DOA(read_data[lane*16+:16]), .DOB(unused));
    end endgenerate
`endif
endmodule

// Two 512x36 semi-dual-port banks share row addresses: one 72-bit
// write and one synchronous 72-bit read. Each slot owns 256 rows.
module FrontendResultStore(input wire clk,
    input wire [8:0] read_address, output wire [71:0] read_data,
    input wire write_enable, input wire [8:0] write_address,
    input wire [71:0] write_data);
`ifdef __ICARUS__
    reg [35:0] even_bank [0:511];
    reg [35:0] odd_bank [0:511];
    reg [71:0] data;
    assign read_data = data;
    always @(posedge clk) begin
        data <= {odd_bank[read_address],even_bank[read_address]};
        if(write_enable) begin
            even_bank[write_address]<=write_data[35:0];
            odd_bank[write_address]<=write_data[71:36];
        end
    end
`else
    genvar bank;
    generate for (bank=0; bank<2; bank=bank+1) begin: banks
        SDPX9B #(.BIT_WIDTH_0(36), .BIT_WIDTH_1(36), .READ_MODE(1'b0)) ram (
            .CLKA(clk), .CLKB(clk), .CEA(write_enable),
            .CEB(1'b1), .OCE(1'b0), .RESETA(1'b0), .RESETB(1'b0),
            .BLKSELA(3'b0), .BLKSELB(3'b0),
            .ADA({write_address,1'b0,4'b1111}), .ADB({read_address,5'b0}),
            .DI(write_data[bank*36+:36]), .DO(read_data[bank*36+:36]));
    end endgenerate
`endif
endmodule

// A held ready/valid request plus one accepted long request. The CP may run
// independently while this engine splits, fills and drains the physical bus.
// Completion is held until acknowledged and occurs once for the whole request.
module FrontendFetch(input wire clk, input wire reset,
    input wire job_valid, output wire job_ready,
    input wire [22:0] job_source, input wire [12:0] job_destination,
    input wire [13:0] job_bytes,
    output wire done_valid, input wire done_ready, output reg done_error,
    output wire memory_request_valid, input wire memory_request_ready,
    output wire [22:0] memory_address, output wire [7:0] memory_bytes,
    input wire memory_response_valid, input wire [63:0] memory_data,
    input wire memory_last, input wire memory_error,
    output wire scratch_write, output wire [9:0] scratch_address,
    output wire [63:0] scratch_data);
    localparam IDLE=0, REQUEST=1, RECEIVE=2, DONE=3;
    reg [1:0] state;
    reg [22:0] source;
    reg [12:0] destination;
    reg [13:0] remaining;
    reg [4:0] beats;
    wire [7:0] length = (source[6:0]==0 && remaining>=128) ? 8'd128 :
        ((source[5:0]==0 && remaining>=64) ? 8'd64 : 8'd32);
    // Natural alignment implies that 32/64/128 B transactions never cross
    // the 1 KiB SDRAM row. Source/end checks use extended unsigned arithmetic.
    wire [23:0] source_end = {1'b0,job_source}+job_bytes;
    wire [14:0] destination_end = {2'b0,job_destination}+job_bytes;
    assign job_ready = state==IDLE;
    assign done_valid = state==DONE;
    assign memory_request_valid = state==REQUEST;
    assign memory_address = source;
    assign memory_bytes = length;
    assign scratch_write = state==RECEIVE && memory_response_valid &&
        !memory_error && !done_error;
    assign scratch_address = destination[12:3];
    assign scratch_data = memory_data;
    always @(posedge clk) begin
        if (reset) begin state<=IDLE; done_error<=0; source<=0;
            destination<=0; remaining<=0; beats<=0; end
        else case(state)
            IDLE: if (job_valid) begin
                source<=job_source; destination<=job_destination;
                remaining<=job_bytes;
                done_error<=0;
                if (job_source[4:0]!=0 || job_destination[4:0]!=0 ||
                    job_bytes==0 || job_bytes[4:0]!=0 ||
                    source_end>24'h800000 || destination_end>8192) begin
                    done_error<=1; state<=DONE;
                end else state<=REQUEST;
            end
            REQUEST: if (memory_request_ready) begin
                beats<=length[7:3]; state<=RECEIVE;
            end
            RECEIVE: if (memory_response_valid) begin
                source<=source+8; destination<=destination+8;
                remaining<=remaining-8; beats<=beats-1;
                if (memory_error || memory_last!=(beats==1)) done_error<=1;
                // Drain the accepted transaction even if a beat reports an
                // error. A malformed last signal is also a coarse error.
                if (beats==1 || memory_last) begin
                    if (remaining==8 || done_error || memory_error || !memory_last)
                        state<=DONE;
                    else state<=REQUEST;
                end
            end
            DONE: if (done_ready) state<=IDLE;
        endcase
    end
endmodule
