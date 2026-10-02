`timescale 1ns/1ps
module tb;
    reg controller_clk = 0;
    reg logic_clk = 0;
    reg phy_clk = 0;
    wire sdram_clk = phy_clk;
    always #5.051 controller_clk = !controller_clk;
    always @(posedge controller_clk) logic_clk = !logic_clk;
    initial begin
        #13.259;
        forever begin phy_clk = !phy_clk; #5.051; end
    end

    reg reset = 1;
    reg request_valid = 0, writing = 0;
    reg [20:0] address = 0;
    reg [5:0] words = 0;
    reg [3:0] write_mask = 0;
    wire [63:0] write_data;
    reg write_data_valid = 0;
    reg [3:0] offer_mask = 0;
    reg scalar_source = 0;
    reg [63:0] scalar_data = 0;
    wire request_ready, write_data_ready;
    wire [63:0] read_data;
    wire read_valid, done, initialized;
    wire O_sdram_clk, O_sdram_cke, O_sdram_cs_n;
    wire O_sdram_cas_n, O_sdram_ras_n, O_sdram_wen_n;
    wire [3:0] O_sdram_dqm;
    wire [10:0] O_sdram_addr;
    wire [1:0] O_sdram_ba;
    wire [31:0] IO_sdram_dq;
    integer pin_cycle, refreshes;
    reg checking_read = 0;
    TangNano20KSdramNativeBridge108M54M dut(.*);
    LabPinModel #(.PERIOD_NS(10.102), .RETURN_MIN(1), .RETURN_MAX(4)) pin (
        .sclk(O_sdram_clk), .reset(reset), .cke(O_sdram_cke),
        .cs(O_sdram_cs_n), .ras(O_sdram_ras_n), .cas(O_sdram_cas_n),
        .we(O_sdram_wen_n), .dqm(O_sdram_dqm), .a(O_sdram_addr),
        .ba(O_sdram_ba), .dq(IO_sdram_dq),
        .cycle(pin_cycle), .refreshes(refreshes)
    );
    integer core_cycles = 0;
    always @(posedge controller_clk) begin
        core_cycles <= core_cycles + 1;
        if (core_cycles > 100000) $fatal(1, "native bridge cycle bound");
    end

    function [63:0] value;
        input integer sector;
        input integer beat;
        reg [31:0] high_word, low_word;
        begin
            high_word = 32'h98760000 + sector*256 + beat;
            low_word = 32'h12340000 + sector*256 + beat;
            value = {high_word, low_word};
        end
    endfunction
    integer source_sector = 0;
    integer source_beat = 0;
    assign write_data = scalar_source ? scalar_data : value(source_sector, source_beat);
    always @(posedge logic_clk)
        if (write_data_valid && write_data_ready)
            source_beat <= source_beat + 1;

    integer read_count = 0;
    integer expected_sector = 0;
    integer expected_beats = 0;
    reg scalar_check = 0;
    reg [63:0] expected_scalar = 0;
    always @(posedge logic_clk) begin
        if (read_valid) begin
            if (!checking_read || read_count >= expected_beats ||
                read_data !== (scalar_check ? expected_scalar : value(expected_sector, read_count)))
                $fatal(1, "read pair %0d: got %h, wanted %h",
                       read_count, read_data, value(expected_sector, read_count));
            read_count <= read_count + 1;
        end
    end

    task offer;
        input transaction_write;
        input [20:0] transaction_address;
        input [5:0] transaction_words;
        begin
            @(negedge logic_clk);
            request_valid = 1;
            writing = transaction_write;
            address = transaction_address;
            words = transaction_words;
            write_mask = offer_mask;
            while (!request_ready) @(negedge logic_clk);
            @(posedge logic_clk); #1; request_valid = 0;
        end
    endtask

    task write_scalar;
        input [3:0] mask;
        input [63:0] data;
        integer bound;
        begin
            offer_mask = mask;
            scalar_source = 1;
            scalar_data = data;
            offer(1, 21'h100, 1);
            offer_mask = 0;
            source_beat = 0;
            write_data_valid = 1;
            bound = 0;
            while (source_beat == 0 && bound < 100) begin
                @(posedge logic_clk); #1;
                bound = bound + 1;
            end
            if (source_beat != 1) $fatal(1, "scalar write was not captured");
            write_data_valid = 0;
            wait_done();
            scalar_source = 0;
        end
    endtask

    task read_scalar;
        input [31:0] expected;
        begin
            read_count = 0;
            checking_read = 1;
            scalar_check = 1;
            expected_beats = 1;
            expected_scalar = {32'b0, expected};
            offer(0, 21'h100, 1);
            wait_done();
            if (read_count !== 1) $fatal(1, "scalar read count %0d", read_count);
            checking_read = 0;
            scalar_check = 0;
        end
    endtask

    task wait_done;
        integer bound;
        begin
            bound = 0;
            while (!done && bound < 2000) begin
                @(posedge logic_clk); #1;
                bound = bound + 1;
            end
            if (!done) $fatal(1, "native descriptor never completed");
            @(posedge logic_clk); #1;
        end
    endtask

    task write_sector;
        input integer sector;
        integer bound;
        begin
            offer(1, sector*32, 32);
            source_sector = sector;
            source_beat = 0;
            write_data_valid = 1;
            bound = 0;
            while (source_beat < 16 && bound < 100) begin
                @(posedge logic_clk); #1;
                bound = bound + 1;
            end
            if (source_beat != 16)
                $fatal(1, "write sector %0d stopped after %0d pairs", sector, source_beat);
            write_data_valid = 0;
            wait_done();
        end
    endtask

    task read_sector;
        input integer sector;
        begin
            read_count = 0;
            expected_sector = sector;
            expected_beats = 16;
            checking_read = 1;
            offer(0, sector*32, 32);
            wait_done();
            if (read_count !== 16)
                $fatal(1, "read sector %0d returned %0d pairs", sector, read_count);
            checking_read = 0;
        end
    endtask

    integer sector;
    initial begin
        repeat (4) @(posedge logic_clk);
        @(negedge logic_clk); reset = 0;
        wait (initialized);
        for (sector = 0; sector < 4; sector = sector + 1)
            write_sector(sector);
        for (sector = 0; sector < 4; sector = sector + 1)
            read_sector(sector);
        // An external reset during a return stream must discard that owner.
        offer(0, 0, 32);
        wait (dut.core_phase[4:0] == 18);
        @(negedge logic_clk); reset = 1;
        repeat (4) @(posedge logic_clk);
        @(negedge logic_clk); reset = 0;
        wait (initialized);
        repeat (4) begin
            @(posedge logic_clk); #1;
            if (read_valid || done) $fatal(1, "orphan return after reset");
        end
        read_sector(0);
        write_scalar(4'b0000, 64'h0000000011223344);
        write_scalar(4'b0011, 64'h00000000aabbccdd);
        read_scalar(32'haabb3344);
        if (refreshes < 2) $fatal(1, "controller never refreshed");
        $display("PASS native 64/32 bridge, four bank-striped sectors, masked scalar, reset and pin timings");
        $finish;
    end
endmodule
