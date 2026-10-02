`timescale 1ns/1ps
module tb;
    reg clk = 0;
    always #5 clk = !clk;
    reg reset = 0;
    reg cpu_request_valid = 0, cpu_write = 0, cpu_line = 0;
    reg [21:0] cpu_address = 0;
    reg [1:0] cpu_line_count_minus_1 = 0;
    reg [63:0] cpu_write_data = 0;
    reg cpu_response_ready = 1;
    reg [63:0] controller_read_data = 0;
    reg controller_read_valid = 0;
    reg controller_init_done = 0, controller_request_ready = 1;
    reg controller_done = 0, controller_write_data_ready = 0;
    wire cpu_request_ready, cpu_write_data_ready, cpu_response_valid;
    wire [63:0] cpu_read_data;
    wire cpu_response_last, cpu_error;
    wire controller_request_valid, controller_write;
    wire [20:0] controller_address;
    wire [3:0] controller_write_mask;
    wire [63:0] controller_write_data;
    wire controller_write_data_valid;
    wire [5:0] controller_words;
    SharedSdramPort dut(.*);

    integer cycles = 0;
    integer fed = 0;
    always @(posedge clk) begin
        cycles <= cycles + 1;
        if (cycles > 2000) $fatal(1, "adapter timeout state=%0d", dut.state);
        if (cpu_write_data_ready) fed <= fed + 1;
    end

    task request_line;
        input writing;
        input [21:0] address;
        input [1:0] count;
        input [5:0] words;
        begin
            @(negedge clk);
            cpu_write = writing;
            cpu_line = 1;
            cpu_address = address;
            cpu_line_count_minus_1 = count;
            cpu_request_valid = 1;
            #1;
            if (!cpu_request_ready || !controller_request_valid ||
                controller_address !== address[21:1] || controller_words !== words ||
                controller_write !== writing || controller_write_mask !== 0)
                $fatal(1, "bad native line descriptor");
            @(posedge clk); #1;
            cpu_request_valid = 0;
        end
    endtask

    task complete_write;
        input integer count;
        integer k;
        begin
            fed = 0;
            for (k = 0; k < count; k = k + 1) begin
                @(negedge clk);
                cpu_write_data = 64'hA000000000000000 + k;
                controller_write_data_ready = 1;
                #1;
                if (!cpu_write_data_ready || !controller_write_data_valid ||
                    controller_write_data !== cpu_write_data)
                    $fatal(1, "write pair %0d was not source-held", k);
                @(posedge clk); #1;
                controller_write_data_ready = 0;
            end
            if (fed !== count) $fatal(1, "wrong source advance count %0d", fed);
            @(negedge clk); controller_done = 1;
            @(posedge clk); #1; controller_done = 0;
            if (!cpu_response_valid || !cpu_response_last || cpu_error)
                $fatal(1, "write completion missing");
            @(posedge clk); #1;
        end
    endtask

    task complete_read;
        input integer count;
        integer k;
        begin
            for (k = 0; k < count; k = k + 1) begin
                @(negedge clk);
                controller_read_data = 64'hC000000000000000 + k;
                controller_read_valid = 1;
                @(posedge clk); #1;
                if (!cpu_response_valid || cpu_read_data !== controller_read_data ||
                    cpu_response_last !== (k == count - 1))
                    $fatal(1, "read beat %0d mismatch", k);
            end
            @(negedge clk); controller_read_valid = 0; controller_done = 1;
            @(posedge clk); #1; controller_done = 0;
            repeat (3) @(posedge clk);
            #1;
            if (dut.state !== 0) $fatal(1, "read owner did not retire");
        end
    endtask

    initial begin
        repeat (2) @(posedge clk);
        @(negedge clk); controller_init_done = 1;
        request_line(1, 22'h000200, 0, 8); complete_write(4);
        request_line(0, 22'h000240, 1, 16); complete_read(8);
        request_line(1, 22'h000300, 3, 32); complete_write(16);

        // Reject an unsupported length before it reaches the controller.
        @(negedge clk);
        cpu_write = 1; cpu_line = 1; cpu_address = 22'h000380;
        cpu_line_count_minus_1 = 2; cpu_request_valid = 1;
        #1;
        if (!cpu_request_ready || controller_request_valid) $fatal(1, "reserved length request escaped");
        @(posedge clk); #1; cpu_request_valid = 0;
        if (!cpu_response_valid || !cpu_error) $fatal(1, "reserved length error missing");
        @(posedge clk); #1;

        // A halfword write uses one aligned native word and masks its low lane.
        @(negedge clk);
        cpu_line = 0; cpu_write = 1; cpu_address = 22'h00000f;
        cpu_write_data = 64'h00001234; cpu_request_valid = 1;
        #1;
        if (!controller_request_valid || controller_words != 1 ||
            controller_write_mask != 4'b0011) $fatal(1, "scalar descriptor mismatch");
        @(posedge clk); #1; cpu_request_valid = 0;
        @(negedge clk); controller_write_data_ready = 1;
        #1;
        if (controller_write_data !== 64'h0000000012340000)
            $fatal(1, "scalar lane mismatch");
        @(posedge clk); #1; controller_write_data_ready = 0;
        @(negedge clk); controller_done = 1;
        @(posedge clk); #1; controller_done = 0;
        @(posedge clk); #1;
        if (cpu_error) $fatal(1, "unexpected scalar error");
        $display("DIGITAL_DESIGN_PASS"); $finish;
    end
endmodule
