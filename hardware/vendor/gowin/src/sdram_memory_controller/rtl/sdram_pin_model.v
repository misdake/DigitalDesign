`timescale 1ns / 1ps
// CL2 timing oracle using the published 18 ns RP/RCD reference.
// Behavioral pin model, not an exact model of the embedded memory die.
module LabPinModel #(
    parameter TAC = 6.0,
    parameter TOH = 2.0,
    parameter THZ = 0,
    parameter RETURN_MIN = 0,
    parameter RETURN_MAX = 0,
    parameter real PERIOD_NS = 1000.0 / 108.0,
    parameter RCD_MIN = $rtoi($ceil(18.0 / PERIOD_NS)),
    parameter RP_MIN = $rtoi($ceil(18.0 / PERIOD_NS)),
    parameter RFC_MIN = $rtoi($ceil(63.0 / PERIOD_NS))
) (
    input                 sclk,
    input                 reset,
    input                 cke,
    input                 cs,
    input                 ras,
    input                 cas,
    input                 we,
    input          [ 3:0] dqm,
    input          [10:0] a,
    input          [ 1:0] ba,
    inout          [31:0] dq,
    output integer        cycle = 0,
    output integer        refreshes = 0
);
    reg [31:0] mem[0:2097151];
    reg marked[0:2097151];
    function [31:0] physical_word;
        input [20:0] location;
        begin
            physical_word = (marked[location] === 1'b1) ? mem[location] : 32'hbad0bad0;
        end
    endfunction
    task seed_word;
        input [20:0] location;
        input [31:0] value;
        begin
            mem[location] = value;
            marked[location] = 1;
        end
    endtask
    reg [3:0] active = 0;
    reg [10:0] rows[0:3];
    integer activated[0:3], precharged[0:3], last_write[0:3];
    integer last_refresh = -100, last_mode = -100;
    reg  mode_loaded = 0;
    real last_edge = -1000;
    reg reading = 0, writing_bus = 0;
    reg [1:0] burst_bank = 0;
    reg [7:0] col = 0;
    reg [20:0] queue_addr[0:1];
    reg [1:0] queue_valid = 0;
    reg output_active = 0;
    reg dq_enable = 0;
    reg [31:0] dq_value = 0;
    assign dq = dq_enable ? dq_value : 32'bz;
    integer i, j, location;
    reg [31:0] next_dq;
    reg push;
    reg [20:0] push_addr;
    initial begin
        for (i = 0; i < 4; i = i + 1) begin
            activated[i]  = -100;
            precharged[i] = -100;
            last_write[i] = -100;
        end
    end
    always @(posedge sclk) begin
        cycle = cycle + 1;
        if (mode_loaded && $realtime - last_edge < 9.999)
            $fatal(1, "CL2 device tCK2 minimum 10ns violated");
        last_edge = $realtime;
        // Controller reset starts a fresh initialization/refresh coverage epoch.
        // The memory array and bank state remain independent until real PRE commands.
        if (reset) mode_loaded = 0;
        if (!reset && cke && !cs) begin
            if (mode_loaded && cycle - last_refresh > $rtoi(15600.0 / PERIOD_NS))
                $fatal(1, "refresh deadline exceeded");
            push = 0;
            push_addr = 0;
            if (cycle - last_refresh < RFC_MIN && {ras, cas, we} != 7)
                $fatal(
                    1,
                    "tRFC violation cycle=%d cmd=%d since=%d",
                    cycle,
                    {
                        ras, cas, we
                    },
                    cycle - last_refresh
                );
            if (cycle - last_mode < 2 && {ras, cas, we} != 7) $fatal(1, "tMRD violation");
            case ({
                ras, cas, we
            })
                3: begin
                    if (cycle - activated[ba] < $rtoi($ceil(63.0 / PERIOD_NS)))
                        $fatal(1, "tRC violation");
                    if (active[ba] || cycle - precharged[ba] < RP_MIN)
                        $fatal(1, "ACT bank/tRP violation");
                    for (j = 0; j < 4; j = j + 1)
                    if (j != ba && cycle - activated[j] < $rtoi($ceil(14.0 / PERIOD_NS)))
                        $fatal(1, "tRRD violation");
                    active[ba] = 1;
                    rows[ba] = a;
                    activated[ba] = cycle;
                end
                2: begin
                    // PRE closes a matching full-page burst; already queued READ data drains.
                    if (writing_bus && (a[10] || ba == burst_bank) && dqm != 15)
                        $fatal(1, "PRE interruption of WRITE requires masked input data");
                    if (a[10] || ba == burst_bank) begin
                        reading = 0;
                        writing_bus = 0;
                    end
                    for (j = 0; j < 4; j = j + 1)
                    if (a[10] || j == ba) begin
                        if (active[j] && (cycle - activated[j] < $rtoi(
                                $ceil(42.0 / PERIOD_NS)
                            ) || cycle - last_write[j] < 2))
                            $fatal(1, "PRE timing violation");
                        active[j] = 0;
                        precharged[j] = cycle;
                    end
                end
                1: begin
                    if (reading || writing_bus || queue_valid != 0)
                        $fatal(1, "REF before burst/data pipeline drain");
                    if (active != 0) $fatal(1, "REF with active bank");
                    for (j = 0; j < 4; j = j + 1)
                    if (cycle - precharged[j] < RP_MIN) $fatal(1, "REF tRP violation");
                    last_refresh = cycle;
                    refreshes = refreshes + 1;
                end
                0: begin
                    if (a != 11'h027) $fatal(1, "Unexpected mode %h", a);
                    last_mode   = cycle;
                    mode_loaded = 1;
                end
                4, 5: begin
                    if (!active[ba] || cycle - activated[ba] < RCD_MIN)
                        $fatal(1, "column command tRCD/bank violation");
                    burst_bank = ba;
                    col = a[7:0];
                    reading = ({ras, cas, we} == 5);
                    writing_bus = ({ras, cas, we} == 4);

                end
                6: begin
                    reading = 0;
                    writing_bus = 0;
                end
            endcase
            if (writing_bus) begin
                location = {burst_bank, rows[burst_bank], col};
                if ($isunknown(dq) && dqm != 15) $fatal(1, "Invalid write DQ/DQM");
                if (marked[location] !== 1'b1) mem[location] = 32'hbad0bad0;
                for (j = 0; j < 4; j = j + 1) if (!dqm[j]) mem[location][j*8+:8] = dq[j*8+:8];
                marked[location] = 1;
                // tWR is measured from the last accepted data-in, not masked NOP cycles.
                // EM638325 Rev3.2 p11: mask all intervening data through WRITE/PRE interruption.
                if (dqm != 15) last_write[burst_bank] = cycle;
                col = col + 1'b1;
            end
            if (reading) begin
                push = 1;
                push_addr = {burst_bank, rows[burst_bank], col};
                col = col + 1'b1;
            end
            // CL2 launches each word after one intervening rising edge plus tAC.
            // Device tAC is independent of the controller's capture phase.
            if (queue_valid[0]) begin
                location = queue_addr[0];
                next_dq  = (marked[location] === 1'b1) ? mem[location] : 32'hbad0bad0;
                // Old data is guaranteed only through tOH, not until max tAC.
                // Poison the unspecified transition window to expose false timing passes.
                if (TAC + RETURN_MAX > TOH + RETURN_MIN) dq_value <= #(TOH + RETURN_MIN) 32'bx;
                dq_value <= #(TAC + RETURN_MAX) next_dq;
                // Enable once at burst start. Re-enabling on every delayed word can
                // resurrect a stopped burst when pad delay exceeds one clock.
                if (!output_active) dq_enable <= #(TAC + RETURN_MAX) 1;
                output_active = 1;
            end else begin
                dq_enable <= #(THZ + RETURN_MIN) 0;
                output_active = 0;
            end
            queue_addr[1] = queue_addr[0];
            queue_addr[0] = push_addr;
            queue_valid   = {queue_valid[0], push};
        end
    end
endmodule
