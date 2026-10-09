module CpuV3DataCache (
    input wire clk,
    input wire reset,
    input wire clean_all,
    input wire invalidate_all,
    input wire line_copy_start,
    input wire [21:0] line_copy_source,
    input wire [7:0] line_copy_destination_page,
    input wire line_clean_start,
    input wire [21:0] line_clean_address,
    output wire line_copy_ready,
    input wire cpu_request_valid,
    input wire cpu_write,
    input wire [31:0] cpu_address,
    input wire [15:0] cpu_write_data,
    input wire cpu_response_ready,
    input wire memory_request_ready,
    input wire memory_write_data_ready,
    input wire memory_response_valid,
    input wire [63:0] memory_read_data,
    input wire memory_error,
    output wire cpu_request_ready,
    output wire cpu_response_valid,
    output wire [15:0] cpu_read_data,
    output wire cpu_error,
    output wire memory_request_valid,
    output wire memory_write,
    output wire memory_line,
    output wire [21:0] memory_address,
    output wire [63:0] memory_write_data,
    output wire memory_response_ready,
    output wire maintenance_busy,
    output reg maintenance_done = 0,
    output reg maintenance_error = 0,
    // High while metadata valid/dirty records are sweep-clearing. The system holds
    // the core for this window so a reset clear cannot surface as a
    // not-ready D-cache on the core's first access.
    output wire valid_sweep
);

localparam [4:0] ST_IDLE=0, ST_LOOKUP=1, ST_LINE_REQUEST=2,
    ST_LINE_RECEIVE=3, ST_COPY_RESPONSE=4, ST_WB_PRIME=5,
    ST_WB_CAPTURE=6, ST_WB_REQUEST=7, ST_WB_STREAM=8,
    ST_WB_RESPONSE=9, ST_SCAN=10, ST_COPY_PREPARE=11,
    ST_COPY_INVALIDATE=12, ST_SCAN_PRIME=13, ST_META_REFRESH=14;

reg [4:0] state = ST_IDLE;
reg pending_write = 0;
reg [31:0] pending_address = 0;
reg [15:0] pending_write_data = 0;
reg pending_way = 0;
reg [2:0] refill_beat = 0;
reg [2:0] wb_beat = 0;
reg wb_way = 0;
reg [5:0] wb_set = 0;
reg [21:0] wb_address = 0;
reg wb_for_maintenance = 0;
reg maintenance_active = 0;
reg maintenance_invalidate = 0;
reg [15:0] response_data = 0;
reg response_error = 0;
reg response_valid = 0;
// Each synchronous metadata DPB word holds tag, valid and dirty; way zero also
// holds the per-set victim bit. Reset, full invalidate and memory errors clear
// both ways in a 64-set sweep. Requests are blocked during that scrub. Clean
// retains validity. Invalidate reports done only after its sweep completes.
reg sweep_active = 0;
reg [5:0] sweep_set = 0;
reg sweep_finishes_maintenance = 0;
reg [15:0] refill_response_data = 0;
// A line copy is a normal source lookup/refill followed by the existing
// write-back path with only the physical segment redirected. The destination
// is not allocated on a cold destination. A resident destination is directly
// invalidated before the redirected write; its dirty contents are discarded
// because the complete line is about to be replaced.
reg line_copy_active = 0;
reg wb_for_line_copy = 0;
reg line_clean_active = 0;
reg wb_for_line_clean = 0;
reg [7:0] line_copy_destination_page_latched = 0;
reg line_copy_destination_hit_latched = 0;
reg line_copy_destination_way_latched = 0;

// Serialized metadata scan; no complete dirty bitmap exists.
reg [5:0] scan_set=0;
reg [15:0] wb_metadata=0;
wire [15:0] meta0,meta1;
reg bg_active=0,bg_valid=0,bg_done=0,found_valid=0;
reg may_have_dirty=0;

reg [5:0] bg_read_set=0,bg_pipe_set=0;
reg [6:0] found_index=0;
wire bg_state=state==ST_WB_REQUEST || state==ST_WB_STREAM;
wire bg_hit=(meta0[12]&&meta0[13]) || (meta1[12]&&meta1[13]);
wire bg_way=!(meta0[12]&&meta0[13]);
wire [5:0] pending_set = pending_address[9:4];
wire [11:0] pending_tag = pending_address[21:10];
wire [3:0] pending_word = pending_address[3:0];
wire pending_address_valid = pending_address[31:22] == 0;
// Synchronous metadata launches alongside data on CPU acceptance. Maintenance
// sources share that stage; write-back assembles its address in CAPTURE after
// PRIME has read the selected tag, without adding a transaction cycle.
wire [5:0] tag_read_set = (bg_active && bg_state) ? bg_read_set : (state == ST_SCAN_PRIME) ? scan_set :
    (state == ST_SCAN) ? (scan_set==63 ? scan_set : scan_set+6'd1) :
    (state == ST_WB_PRIME || state == ST_WB_CAPTURE || state == ST_WB_REQUEST ||
     state == ST_WB_STREAM || state == ST_WB_RESPONSE || state == ST_COPY_RESPONSE) ? wb_set :
    (state == ST_IDLE && line_copy_start && line_copy_ready) ? line_copy_source[9:4] :
    (state == ST_IDLE && line_clean_start && line_copy_ready) ? line_clean_address[9:4] :
    (state == ST_IDLE && cpu_request_valid && cpu_request_ready) ? cpu_address[9:4] : pending_set;
wire [11:0] way_0_tag;
wire [11:0] way_1_tag;
wire way_0_valid_read;
wire way_1_valid_read;
wire victim_read;
wire way_0_hit = !sweep_active && way_0_valid_read && way_0_tag == pending_tag;
wire way_1_hit = !sweep_active && way_1_valid_read && way_1_tag == pending_tag;
wire pending_hit = way_0_hit || way_1_hit;
wire hit_way = !way_0_hit && way_1_hit;
wire selected_victim = !way_0_valid_read ? 1'b0 :
                       !way_1_valid_read ? 1'b1 : victim_read;

wire refill_commit = state == ST_LINE_RECEIVE && memory_response_valid &&
    !memory_error && refill_beat == 3 && !sweep_active;
assign way_0_tag=meta0[11:0]; assign way_1_tag=meta1[11:0];
assign way_0_valid_read=meta0[12]; assign way_1_valid_read=meta1[12];
assign victim_read=meta0[14];
wire writeback_read_mode = state == ST_WB_PRIME || state == ST_WB_CAPTURE ||
    state == ST_WB_REQUEST || state == ST_WB_STREAM;
// Hold the current DPB beat during stalls; read the next beat on consumption.
// Consecutive ready edges transfer consecutive beats without FF staging.
wire [1:0] writeback_read_beat = state == ST_WB_STREAM ?
    wb_beat[1:0] + {1'b0, memory_write_data_ready} : 2'd0;
wire [5:0] data_read_set = writeback_read_mode ? wb_set :
    (state == ST_IDLE && cpu_request_valid ? cpu_address[9:4] : pending_set);
wire [3:0] data_read_word = state == ST_IDLE && cpu_request_valid ?
    cpu_address[3:0] : pending_word;
wire [9:0] lookup_way_0_address = {1'b0, data_read_set, data_read_word[3:1]};
wire [9:0] lookup_way_1_address = {1'b1, data_read_set, data_read_word[3:1]};
wire [9:0] wb_read_address_a = {wb_way, wb_set, writeback_read_beat, 1'b0};
wire [9:0] wb_read_address_b = {wb_way, wb_set, writeback_read_beat, 1'b1};
wire [15:0] bank_0_a_read_data, bank_0_b_read_data;
wire [15:0] bank_1_a_read_data, bank_1_b_read_data;
wire [15:0] way_0_read_data = pending_word[0] ? bank_1_a_read_data : bank_0_a_read_data;
wire [15:0] way_1_read_data = pending_word[0] ? bank_1_b_read_data : bank_0_b_read_data;
wire [15:0] hit_read_data = hit_way ? way_1_read_data : way_0_read_data;
wire [63:0] wb_read_data = {bank_1_b_read_data, bank_0_b_read_data,
                            bank_1_a_read_data, bank_0_a_read_data};

wire hit_store = state == ST_LOOKUP && pending_write && pending_hit;
wire refill_write = state == ST_LINE_RECEIVE && memory_response_valid && !memory_error;
wire [9:0] refill_address_a = {pending_way, pending_set, refill_beat[1:0], 1'b0};
wire [9:0] refill_address_b = {pending_way, pending_set, refill_beat[1:0], 1'b1};
wire [9:0] hit_store_address = {hit_way, pending_set, pending_word[3:1]};
wire [15:0] refill_word_0 = pending_write && pending_word[3:2] == refill_beat && pending_word[1:0] == 0 ? pending_write_data : memory_read_data[15:0];
wire [15:0] refill_word_1 = pending_write && pending_word[3:2] == refill_beat && pending_word[1:0] == 1 ? pending_write_data : memory_read_data[31:16];
wire [15:0] refill_word_2 = pending_write && pending_word[3:2] == refill_beat && pending_word[1:0] == 2 ? pending_write_data : memory_read_data[47:32];
wire [15:0] refill_word_3 = pending_write && pending_word[3:2] == refill_beat && pending_word[1:0] == 3 ? pending_write_data : memory_read_data[63:48];

__CACHE_DATA_BANKS__ u_data_banks (
    .clk(clk),
    .bank_0_a_write_enable(refill_write || (hit_store && !pending_word[0])),
    .bank_0_a_address(refill_write ? refill_address_a : writeback_read_mode ? wb_read_address_a :
        (hit_store && !pending_word[0] ? hit_store_address : lookup_way_0_address)),
    .bank_0_a_write_data(refill_write ? refill_word_0 : pending_write_data),
    .bank_0_a_read_data(bank_0_a_read_data),
    .bank_0_b_write_enable(refill_write),
    .bank_0_b_address(refill_write ? refill_address_b : writeback_read_mode ? wb_read_address_b : lookup_way_1_address),
    .bank_0_b_write_data(refill_word_2), .bank_0_b_read_data(bank_0_b_read_data),
    .bank_1_a_write_enable(refill_write || (hit_store && pending_word[0])),
    .bank_1_a_address(refill_write ? refill_address_a : writeback_read_mode ? wb_read_address_a :
        (hit_store && pending_word[0] ? hit_store_address : lookup_way_0_address)),
    .bank_1_a_write_data(refill_write ? refill_word_1 : pending_write_data),
    .bank_1_a_read_data(bank_1_a_read_data),
    .bank_1_b_write_enable(refill_write),
    .bank_1_b_address(refill_write ? refill_address_b : writeback_read_mode ? wb_read_address_b : lookup_way_1_address),
    .bank_1_b_write_data(refill_word_3), .bank_1_b_read_data(bank_1_b_read_data)
);

wire pending_hit_dirty = hit_way ? meta1[13] : meta0[13];
wire selected_victim_dirty = selected_victim ? meta1[13] : meta0[13];
wire [11:0] prepared_line_copy_destination_tag =
    {line_copy_destination_page_latched, pending_address[13:10]};
wire prepared_line_copy_destination_way_0_hit =
    prepared_line_copy_destination_tag != pending_tag && way_0_valid_read &&
    way_0_tag == prepared_line_copy_destination_tag;
wire prepared_line_copy_destination_way_1_hit =
    prepared_line_copy_destination_tag != pending_tag && way_1_valid_read &&
    way_1_tag == prepared_line_copy_destination_tag;
wire dest_invalidate=state==ST_COPY_INVALIDATE && line_copy_destination_hit_latched;
wire wb_clear=state==ST_WB_RESPONSE && memory_response_valid && !memory_error;
wire refill_prime=state==ST_LINE_REQUEST;
wire meta_update=hit_store || refill_prime || refill_commit || dest_invalidate;
wire update_way=hit_store?hit_way:dest_invalidate?line_copy_destination_way_latched:pending_way;
wire [15:0] old_word=update_way?meta1:meta0;
wire [15:0] install_word={1'b0,!pending_way,pending_write,1'b1,pending_tag};
wire [15:0] update_word=refill_commit?install_word:
    hit_store?(old_word | 16'h2000):(old_word & 16'hcfff);
// A way-one install also updates the victim bit in the current way-zero word.
// No stale pre-eviction snapshot is reused here.
wire meta_write0=sweep_active || (wb_clear&&!wb_way) ||
    (meta_update&&!update_way) || (refill_commit&&pending_way);
wire meta_write1=sweep_active || (wb_clear&&wb_way) || (meta_update&&update_way);
wire [5:0] meta_set0=sweep_active?sweep_set:wb_clear?wb_set:pending_set;
wire [5:0] meta_set1=sweep_active?sweep_set:wb_clear?wb_set:pending_set;
wire [15:0] wb_cleared=wb_for_maintenance || wb_for_line_clean ?
    (wb_metadata & 16'hdfff):(wb_metadata & 16'hcfff);
wire [15:0] meta_word0=sweep_active?16'b0:wb_clear?wb_cleared:
    (refill_commit&&pending_way)?(meta0 & 16'hbfff):update_word;
wire [15:0] meta_word1=sweep_active?16'b0:wb_clear?wb_cleared:update_word;
__CACHE_METADATA__ u_metadata(.clk(clk),.read_set(tag_read_set),
 .write_0(meta_write0),.write_set_0(meta_set0),.write_data_0(meta_word0),
 .write_1(meta_write1),.write_set_1(meta_set1),.write_data_1(meta_word1),.q0(meta0),.q1(meta1));
// ST_IDLE gives line-copy and line-clean commands priority over a CPU access.
// Their start strobes therefore need not feed this ready path as additional
// negative terms; the core never issues a normal access for the same
// instruction, and removing those redundant terms keeps maintenance decode
// off the cache-state clock-enable path.
assign cpu_request_ready = state == ST_IDLE && !response_valid &&
    !maintenance_active && !clean_all && !invalidate_all && !sweep_active;
assign line_copy_ready = state == ST_IDLE && !response_valid &&
    !maintenance_active && !clean_all && !invalidate_all && !sweep_active;
assign cpu_response_valid = response_valid;
assign cpu_read_data = response_data;
assign cpu_error = response_valid && response_error;
assign memory_request_valid = state == ST_LINE_REQUEST || state == ST_WB_REQUEST;
assign memory_write = state == ST_WB_REQUEST || state == ST_WB_STREAM ||
    state == ST_WB_RESPONSE || state == ST_COPY_RESPONSE;
assign memory_line = state != ST_IDLE && state != ST_LOOKUP;
assign memory_address = memory_write ? wb_address : {pending_address[21:4],4'b0};
assign memory_write_data = wb_read_data;
assign memory_response_ready = state == ST_LINE_RECEIVE ||
    state == ST_WB_RESPONSE || state == ST_COPY_RESPONSE;
assign maintenance_busy = maintenance_active;
assign valid_sweep = sweep_active;

// A conservative hint, never an exact count. Any accepted store sets it;
// single-line clean/copy does not attempt to subtract from the hint.
// Set has priority over done/scrub if interfaces are ever extended to overlap.
wire full_clean_finished=(state==ST_SCAN && scan_set==63 && !bg_hit) ||
    (state==ST_WB_RESPONSE && wb_for_maintenance && memory_response_valid &&
     !memory_error && !found_valid && bg_done);
wire fault_scrub=(state==ST_LINE_RECEIVE || state==ST_WB_RESPONSE ||
    state==ST_COPY_RESPONSE) && memory_response_valid && memory_error;
always @(posedge clk) begin
    if(reset) may_have_dirty<=0;
    else if(cpu_request_valid && cpu_request_ready && cpu_write) may_have_dirty<=1;
    else if(full_clean_finished || fault_scrub) may_have_dirty<=0;
end

always @(posedge clk) begin
    maintenance_done <= 0;
    if (reset) begin
        state <= ST_IDLE;
        response_valid <= 0;
        response_error <= 0;
        maintenance_active <= 0;
        line_copy_active <= 0;
        wb_for_line_copy <= 0;
        line_clean_active <= 0;
        wb_for_line_clean <= 0;
        line_copy_destination_page_latched <= 0;
        line_copy_destination_hit_latched <= 0;
        line_copy_destination_way_latched <= 0;
        maintenance_error <= 0;
        scan_set <= 0; bg_active<=0; bg_valid<=0; bg_done<=0; found_valid<=0;
        sweep_active <= 1;
        sweep_set <= 0;
        sweep_finishes_maintenance <= 0;
    end else begin
        if (response_valid && cpu_response_ready)
            response_valid <= 0;
        // Sweep control: both metadata ways scrubbed one set per cycle; an
        // invalidate that finished its write-backs reports done when the
        // sweep completes.
        if (sweep_active) begin
            sweep_set <= sweep_set + 1'b1;
            if (sweep_set == 63) begin
                sweep_active <= 0;
                if (sweep_finishes_maintenance) begin
                    sweep_finishes_maintenance <= 0;
                    maintenance_active <= 0;
                    maintenance_done <= 1;
                end
            end
        end
        // One synchronous set lookup per WB cycle, at most one pending hit.
        // Never scan on the response/metadata-clear edge: DPB writes hold DO.
        if(bg_active && bg_state) begin
            bg_pipe_set<=bg_read_set;
            bg_valid<=1;
            if(bg_read_set!=63) bg_read_set<=bg_read_set+1'b1;
            if(bg_valid) begin
                if(bg_hit) begin
                    found_valid<=1; found_index<={bg_way,bg_pipe_set};
                    bg_active<=0; bg_valid<=0;
                end else if(bg_pipe_set==63) begin
                    bg_active<=0; bg_valid<=0; bg_done<=1;
                end
            end
        end
        case (state)
            ST_IDLE: begin
                if (!response_valid && (clean_all || invalidate_all)) begin
                    maintenance_active <= 1;
                    line_copy_active <= 0;
                    wb_for_line_copy <= 0;
                    wb_for_maintenance <= 0;
                    line_clean_active <= 0;
                    wb_for_line_clean <= 0;
                    maintenance_invalidate <= invalidate_all;
                    maintenance_error <= 0;
                    wb_for_maintenance <= 1;
                    scan_set <= 0;
                    if(!may_have_dirty) begin
                        if(invalidate_all) begin
                            sweep_active<=1;sweep_set<=0;sweep_finishes_maintenance<=1;
                        end else begin maintenance_active<=0;maintenance_done<=1;end
                    end else state <= ST_SCAN_PRIME;
                end else if (line_copy_start && line_copy_ready) begin
                    maintenance_error <= 0;
                    if (|line_copy_source[3:0]) begin
                        maintenance_done <= 1;
                        maintenance_error <= 1;
                    end else begin
                        maintenance_active <= 1;
                        line_copy_active <= 1;
                        wb_for_line_copy <= 0;
                        wb_for_maintenance <= 0;
                        line_copy_destination_page_latched <=
                            line_copy_destination_page;
                        pending_write <= 0;
                        pending_address <= {10'b0, line_copy_source};
                        state <= ST_COPY_PREPARE;
                    end
                end else if (line_clean_start && line_copy_ready) begin
                    maintenance_error <= 0;
                    maintenance_active <= 1;
                    line_copy_active <= 0;
                    wb_for_line_copy <= 0;
                    wb_for_maintenance <= 0;
                    line_clean_active <= 1;
                    wb_for_line_clean <= 0;
                    pending_write <= 0;
                    pending_address <= {10'b0, line_clean_address};
                    state <= ST_LOOKUP;
                end else if (cpu_request_valid && cpu_request_ready) begin
                    line_copy_active <= 0;
                    wb_for_line_copy <= 0;
                    wb_for_maintenance <= 0;
                    line_clean_active <= 0;
                    wb_for_line_clean <= 0;
                    pending_write <= cpu_write;
                    pending_address <= cpu_address;
                    pending_write_data <= cpu_write_data;
                    response_error <= 0;
                    state <= ST_LOOKUP;
                end
            end
            ST_LOOKUP: begin
                if (!pending_address_valid) begin
                    response_data <= 0;
                    response_error <= 1;
                    response_valid <= 1;
                    state <= ST_IDLE;
                end else if (pending_hit) begin
                    if (line_copy_active) begin
                        wb_way <= hit_way;
                        wb_set <= pending_set;
                        wb_for_line_copy <= 1;
                        wb_beat <= 0;
                        state <= ST_WB_PRIME;
                    end else if (line_clean_active) begin
                        if (pending_hit_dirty) begin
                            wb_way <= hit_way;
                            wb_set <= pending_set;
                            wb_for_line_clean <= 1;
                            wb_beat <= 0;
                            state <= ST_WB_PRIME;
                        end else begin
                            maintenance_active <= 0;
                            line_clean_active <= 0;
                            maintenance_done <= 1;
                            state <= ST_IDLE;
                        end
                    end else begin
                        response_data <= pending_write ? 16'b0 : hit_read_data;
                        response_error <= 0;
                        response_valid <= 1;
                        state <= ST_IDLE;
                    end
                end else begin
                    pending_way <= selected_victim;
                    if (line_clean_active) begin
                        maintenance_active <= 0;
                        line_clean_active <= 0;
                        maintenance_done <= 1;
                        state <= ST_IDLE;
                    end else if (selected_victim_dirty) begin
                        wb_way <= selected_victim;
                        wb_set <= pending_set;
                        wb_for_maintenance <= 0;
                        wb_for_line_copy <= 0;
                        wb_beat <= 0;
                        state <= ST_WB_PRIME;
                    end else begin
                        refill_beat <= 0;
                        state <= ST_LINE_REQUEST;
                    end
                end
            end
            ST_COPY_PREPARE: begin
                line_copy_destination_hit_latched <=
                    prepared_line_copy_destination_way_0_hit ||
                    prepared_line_copy_destination_way_1_hit;
                line_copy_destination_way_latched <=
                    !prepared_line_copy_destination_way_0_hit &&
                    prepared_line_copy_destination_way_1_hit;
                // Register the destination way onto the existing valid-array
                // write port. ST_LOOKUP replaces it with the source victim on
                // a miss, so no extra write-way mux is needed.
                pending_way <= !prepared_line_copy_destination_way_0_hit &&
                    prepared_line_copy_destination_way_1_hit;
                state <= ST_COPY_INVALIDATE;
            end
            ST_COPY_INVALIDATE: begin
                state <= ST_META_REFRESH;
            end
            ST_META_REFRESH: state <= ST_LOOKUP;
            ST_SCAN_PRIME: state <= ST_SCAN;
            ST_WB_PRIME: begin
                wb_beat <= 0;
                state <= ST_WB_CAPTURE;
            end
            ST_WB_CAPTURE: begin
                wb_metadata <= wb_way?meta1:meta0;
                if(wb_for_maintenance) begin
                    found_valid<=0; bg_valid<=0; bg_done<=0;
                    if((wb_way ? meta0[12]&&meta0[13] : meta1[12]&&meta1[13])) begin
                        // The other way in this set can already be queued.
                        found_valid<=1; found_index<={!wb_way,wb_set}; bg_active<=0;
                    end else if(wb_set==63) begin
                        bg_active<=0; bg_done<=1;
                    end else begin
                        bg_active<=1; bg_read_set<=wb_set+1'b1;
                        bg_pipe_set<=wb_set+1'b1;
                    end
                end
                wb_address <= line_copy_active && wb_for_line_copy ?
                    {line_copy_destination_page_latched,
                     pending_address[13:4], 4'b0} :
                    {(wb_way ? way_1_tag : way_0_tag), wb_set, 4'b0};
                state <= ST_WB_REQUEST;
            end
            ST_WB_REQUEST: if (memory_request_ready) begin
                // Address acceptance does not consume beat zero.
                wb_beat <= 0;
                state <= ST_WB_STREAM;
            end
            ST_WB_STREAM: if (memory_response_valid && memory_error) begin
                // An error may terminate the burst before all source beats.
                // Keep response-ready low until the existing error handler.
                state <= line_copy_active && wb_for_line_copy ?
                    ST_COPY_RESPONSE : ST_WB_RESPONSE;
            end else if (memory_write_data_ready) begin
                if (wb_beat == 3)
                    state <= line_copy_active && wb_for_line_copy ?
                        ST_COPY_RESPONSE : ST_WB_RESPONSE;
                else wb_beat <= wb_beat + 1'b1;
            end
            ST_COPY_RESPONSE: if (memory_response_valid) begin
                if (memory_error) begin
                    sweep_active <= 1;
                    sweep_set <= 0;
                    maintenance_error <= 1;
                    maintenance_active <= 0;
                    line_copy_active <= 0;
                    wb_for_line_copy <= 0;
                    line_clean_active <= 0;
                    wb_for_line_clean <= 0;
                    maintenance_done <= 1;
                    state <= ST_IDLE;
                end else begin
                    maintenance_active <= 0;
                    line_copy_active <= 0;
                    wb_for_line_copy <= 0;
                    maintenance_done <= 1;
                    state <= ST_IDLE;
                end
            end
            ST_WB_RESPONSE: if (memory_response_valid) begin
                if (memory_error) begin
                    // Scrub every line with a sweep; the error itself is
                    // reported immediately.
                    sweep_active <= 1;
                    sweep_set <= 0;
                    if (maintenance_active) begin
                        maintenance_active <= 0;
                        line_copy_active <= 0;
                        wb_for_line_copy <= 0;
                        line_clean_active <= 0;
                        wb_for_line_clean <= 0;
                        maintenance_error <= 1;
                        maintenance_done <= 1;
                    end else begin
                        response_error <= 1;
                        response_data <= 0;
                        response_valid <= 1;
                    end
                    state <= ST_IDLE;
                end else begin
                    if (wb_for_maintenance) begin
                        if(found_valid) begin
                            wb_way<=found_index[6];wb_set<=found_index[5:0];
                            scan_set<=found_index[5:0];wb_beat<=0;found_valid<=0;
                            state<=ST_WB_PRIME;
                        end else if(bg_done) begin
                            if(maintenance_invalidate) begin
                                sweep_active<=1;sweep_set<=0;sweep_finishes_maintenance<=1;
                            end else begin maintenance_active<=0;maintenance_done<=1;end
                            state<=ST_IDLE;
                        end else begin
                            // Re-read the final not-yet-tested set. This also
                            // drains a synchronous scan read interrupted by WB completion.
                            scan_set<=bg_pipe_set;bg_active<=0;bg_valid<=0;
                            state<=ST_SCAN_PRIME;
                        end
                    end else if (wb_for_line_clean) begin
                        maintenance_active <= 0;
                        line_clean_active <= 0;
                        wb_for_line_clean <= 0;
                        maintenance_done <= 1;
                        state <= ST_IDLE;
                    end else begin
                        refill_beat <= 0;
                        state <= ST_LINE_REQUEST;
                    end
                end
            end
            ST_SCAN: begin
                if (meta0[12] && meta0[13] || meta1[12] && meta1[13]) begin
                    wb_way <= !(meta0[12] && meta0[13]);
                    wb_set <= scan_set;
                    wb_beat <= 0;
                    state <= ST_WB_PRIME;
                end else if (scan_set == 63) begin
                    if (maintenance_invalidate) begin
                        sweep_active <= 1; sweep_set <= 0;
                        sweep_finishes_maintenance <= 1;
                    end else begin
                        maintenance_active <= 0; maintenance_done <= 1;
                    end
                    state <= ST_IDLE;
                end else begin
                    scan_set <= scan_set+1'b1;
                    // Next set was read on this edge; stay in SCAN.
                end
            end
            ST_LINE_REQUEST: if (memory_request_ready) begin
                refill_beat <= 0;
                state <= ST_LINE_RECEIVE;
            end
            ST_LINE_RECEIVE: if (memory_response_valid) begin
                if (memory_error) begin
                    sweep_active <= 1;
                    sweep_set <= 0;
                    if (line_copy_active) begin
                        maintenance_active <= 0;
                        line_copy_active <= 0;
                        wb_for_line_copy <= 0;
                        maintenance_error <= 1;
                        maintenance_done <= 1;
                    end else if (line_clean_active) begin
                        maintenance_active <= 0;
                        line_clean_active <= 0;
                        wb_for_line_clean <= 0;
                        maintenance_error <= 1;
                        maintenance_done <= 1;
                    end else begin
                        response_data <= 0;
                        response_error <= 1;
                        response_valid <= 1;
                    end
                    state <= ST_IDLE;
                end else begin
                    if (refill_beat == pending_word[3:2])
                        case (pending_word[1:0])
                            0: refill_response_data <= memory_read_data[15:0];
                            1: refill_response_data <= memory_read_data[31:16];
                            2: refill_response_data <= memory_read_data[47:32];
                            default: refill_response_data <= memory_read_data[63:48];
                        endcase
                    if (refill_beat == 3) begin
                        if (line_copy_active) begin
                            wb_way <= pending_way;
                            wb_set <= pending_set;
                            wb_for_line_copy <= 1;
                            wb_beat <= 0;
                            state <= ST_WB_PRIME;
                        end else begin
                            response_data <= pending_write ? 16'b0 :
                                (pending_word[3:2] == 3 ?
                                    (pending_word[1:0] == 0 ? memory_read_data[15:0] :
                                     pending_word[1:0] == 1 ? memory_read_data[31:16] :
                                     pending_word[1:0] == 2 ? memory_read_data[47:32] : memory_read_data[63:48]) :
                                    refill_response_data);
                            response_error <= 0;
                            response_valid <= 1;
                            state <= ST_IDLE;
                        end
                    end else refill_beat <= refill_beat + 1'b1;
                end
            end
            default: state <= ST_IDLE;
        endcase
    end
end

endmodule
