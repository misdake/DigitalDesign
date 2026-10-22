module FramebufferHdmi(
    input wire clk, input wire reset,
    input wire pixel_clock, input wire serial_clock, input wire video_locked,
    input wire boot_enable, input wire boot_failed,
    input wire memory_request_ready, input wire memory_data_valid,
    input wire [63:0] memory_read_data, input wire memory_last, input wire memory_error,
    input wire [2:0] device_index, input wire [3:0] device_channel,
    input wire device_read_enable, input wire device_write_enable,
    input wire [15:0] device_write_data,
    output wire memory_request_valid, output wire memory_urgent,
    output wire [21:0] memory_address, output wire underflow,
    output wire started_ack, output wire startup_error,
    output reg [15:0] device_read_data,
    output wire tmds_clk_p, output wire tmds_clk_n,
    output wire [2:0] tmds_data_p, output wire [2:0] tmds_data_n
);
localparam [21:0] FB_BASE=22'h200000;
localparam [2:0] DISPLAY_DEVICE=3'd3;
localparam [31:0] LAST_VALID_FB_BASE=32'h003e8900;
// Fixed 800x480 2x timing shared with the host model, testbench and board PLL.
__DISPLAY_CONFIG__
localparam integer PRECODE_BITS=__TMDS_PRECODE_BITS__;
// Two published groups, each containing two source rows. A group is released
// only after the final vertical repeat of its second row. Toggle metadata is
// stable until release and crosses through the publish synchronizer.
reg [1:0] published=0, released=0;
reg [1:0] release_meta=0, release_sync=0;
reg fill_slot=0;
// fill_y/local_y identify the even first row of a two-row group. Fetch both
// rows of one tile before advancing burst_index to the next tile column.
// One group cursor is the authority for frame and tile-local rows.
reg [6:0] fill_group=0;
wire [7:0] fill_y={fill_group,1'b0};
wire [3:0] local_y={fill_group[2:0],1'b0};
// Addresses are stored in one 32-byte SDRAM transaction unit.
reg [17:0] active_base_units=FB_BASE[21:4], tile_row_base_units=FB_BASE[21:4];
wire [21:0] active_base={active_base_units,4'b0};
wire [21:0] tile_row_base={tile_row_base_units,4'b0};

reg [17:0] shadow_base_units=FB_BASE[21:4];
reg shadow_high_bad=0,shadow_low_bad=0;
reg [17:0] pending_base_units=FB_BASE[21:4];
wire [21:0] pending_base={pending_base_units,4'b0};
reg shadow_linear=0, pending_linear=0, active_linear=0;
reg [1:0] group_linear=0;
reg next_low_written=0, next_high_written=0;
reg next_pending=0, invalid_address=0;
reg frame_complete=0;
reg [15:0] frame_index=0;
reg frame_meta=0, frame_sync=0, frame_seen=0, frame_toggle=0;
reg underflow_sticky=0, underflow_meta=0, underflow_sync=0;
reg [4:0] burst_index=0;
reg fill_second=0;
reg [1:0] beat_index=0;
reg burst_active=0, memory_error_sticky=0;
wire fill_slot_free = published[fill_slot] == release_sync[fill_slot];
assign memory_urgent = 1'b1;
assign memory_request_valid = boot_enable && !boot_failed && fill_slot_free && !burst_active &&
                              !frame_complete && !memory_error_sticky;
// Adjacent rows of each tile are fetched consecutively. The arbiter retains
// its fixed 32-byte display transactions: the pair is two requests, not a
// longer non-preemptible owner interval.
// Column, even-row pair and second-row bits are disjoint.
wire [17:0] request_units=tile_row_base_units +
    {9'b0,burst_index,local_y[3:1],fill_second};
assign memory_address={request_units,4'b0};
// Each shared address holds four RGB565 pixels across the two banks. One row
// uses 100 addresses, one group 200; 416..511 hold packed color/TMDS tables.
// SDRAM beats bypass staging and never touch those read-only addresses.
wire [8:0] fill_slot_base = fill_slot ? 9'd200 : 9'd0;
wire [8:0] line_write_address = fill_slot_base +
    (fill_second ? 9'd100 : 9'd0) + {2'b0,burst_index,2'b00} + {7'b0,beat_index};
wire line_write = burst_active && memory_data_valid && !memory_error &&
                  !memory_error_sticky && !boot_failed && !reset;
reg [8:0] line_read_address_a, line_read_address_b;
wire [31:0] line_read_data_a, line_read_data_b;
__LINE_BUFFER__ u_line_buffer(
    .write_clock(clk), .write_enable(line_write), .write_address(line_write_address),
    .write_data(memory_read_data), .read_clock(pixel_clock),
    .read_address_a(line_read_address_a), .read_address_b(line_read_address_b),
    .read_data_a(line_read_data_a), .read_data_b(line_read_data_b)
);
always @(posedge clk) begin
    release_meta <= released;
    release_sync <= release_meta;
    frame_meta <= frame_toggle;
    frame_sync <= frame_meta;
    underflow_meta <= underflow_sticky;
    underflow_sync <= underflow_meta;
    if (reset) begin
        published<=0; fill_slot<=0; fill_group<=0; active_base_units<=FB_BASE[21:4];
        tile_row_base_units<=FB_BASE[21:4];
        shadow_base_units<=FB_BASE[21:4];shadow_high_bad<=0;shadow_low_bad<=0; pending_base_units<=FB_BASE[21:4];
        next_low_written<=0; next_high_written<=0;
        next_pending<=0; invalid_address<=0; frame_complete<=0;
        frame_index<=0; frame_meta<=0; frame_sync<=0; frame_seen<=0;
        underflow_meta<=0; underflow_sync<=0;
        burst_index<=0; beat_index<=0; burst_active<=0; memory_error_sticky<=0;
        fill_second<=0; shadow_linear<=0; pending_linear<=0; active_linear<=0; group_linear<=0;
    end else begin
        if (memory_error) begin memory_error_sticky<=1; burst_active<=0; end
        if (frame_sync != frame_seen) begin
            frame_seen<=frame_sync;
            frame_index<=frame_index+1'b1;
            if (frame_complete) begin
                fill_group<=0;

                frame_complete<=0;
                if (next_pending) begin
                    active_base_units<=pending_base_units; active_linear<=pending_linear;
                    tile_row_base_units<=pending_base_units;
                    next_pending<=0;
                end else tile_row_base_units<=active_base_units;
            end
        end
        // Process device writes after the frame event so a NEXT_SWAP arriving
        // on the same clock edge remains pending for the following frame. The
        // active frame still receives the previously submitted address.
        if (device_write_enable && device_index==DISPLAY_DEVICE) begin
            if (device_channel==4'd1) begin
                shadow_base_units[11:0]<=device_write_data[15:4];shadow_low_bad<=|device_write_data[3:0];
                next_low_written<=1;
                invalid_address<=0;
            end else if (device_channel==4'd2) begin
                shadow_base_units[17:12]<=device_write_data[5:0];shadow_high_bad<=|device_write_data[15:6];
                next_high_written<=1;
                invalid_address<=0;
            end else if (device_channel==4'd4) begin
                shadow_linear<=device_write_data[0];
            end else if (device_channel==4'd3 && device_write_data==16'd1 &&
                         next_low_written && next_high_written) begin
                next_low_written<=0;
                next_high_written<=0;
                if (!shadow_high_bad && !shadow_low_bad &&
                    shadow_base_units<=LAST_VALID_FB_BASE[21:4]) begin
                    pending_base_units<=shadow_base_units; pending_linear<=shadow_linear;
                    next_pending<=1;
                    invalid_address<=0;
                end else begin
                    invalid_address<=1;
                end
            end
        end
        if (memory_request_valid && memory_request_ready) begin
            burst_active<=1; beat_index<=0;
        end
        // Each unstallable 64-bit beat writes both BSRAM banks directly.
        // A malformed/failed segment is never published; reset is recovery.
        if (burst_active && memory_data_valid && !memory_error_sticky) begin
            if (memory_error || (memory_last != (beat_index==3))) begin
                memory_error_sticky<=1; burst_active<=0;
            end else if (beat_index==3) begin
                burst_active<=0; beat_index<=0;
                if (!fill_second) fill_second<=1;
                else begin
                    fill_second<=0;
                    if (burst_index==LAST_BURST) begin
                        published[fill_slot]<=~published[fill_slot];
                        group_linear[fill_slot]<=active_linear;
                        burst_index<=0; fill_slot<=~fill_slot;
                        if (fill_y==LAST_FILL_Y-1) frame_complete<=1;
                        else begin
                            fill_group<=fill_group+1'b1;
                            if (local_y==4'd14) begin
                                 tile_row_base_units<=tile_row_base_units+{9'b0,TILE_ROW_STRIDE[12:4]};
                            end
                        end
                    end else burst_index<=burst_index+1'b1;
                end
            end else beat_index<=beat_index+1'b1;
        end
    end
end
reg [1:0] publish_meta=0, publish_sync=0;
reg started=0;
reg [1:0] enable_sync=0, failure_sync=0;
reg started_meta=0, started_sync=0;
always @(posedge clk) begin
    started_meta<=started;
    started_sync<=started_meta;
end
assign started_ack=started_sync;
assign startup_error=memory_error_sticky | underflow_sync;
always @(posedge pixel_clock) begin
    enable_sync<={enable_sync[0],boot_enable};
    failure_sync<={failure_sync[0],boot_failed | memory_error_sticky};
end
// Reset/PLL lock are asynchronous to pixel_clock. Delay their release here
// before scanout consumes the line-group publication toggles or queue state.
reg [2:0] pixel_reset_sync=0;
always @(posedge pixel_clock)
    pixel_reset_sync <= {pixel_reset_sync[1:0], ~(reset | ~video_locked)};
wire pixel_reset = ~pixel_reset_sync[2];
reg display_slot=0, display_second=0;
wire [8:0] display_slot_base = (display_slot ? 9'd200 : 9'd0) +
                                             (display_second ? 9'd100 : 9'd0);
reg vertical_repeat=0;
wire scan_enabled=started && !failure_sync[1];
reg [10:0] h_count=0;
reg [9:0] v_count=0;
wire hsync = h_count < H_SYNC_END;
wire vsync = v_count < V_SYNC_END;
wire active = h_count>=H_ACTIVE_START && h_count<H_ACTIVE_END &&
               v_count>=V_ACTIVE_START && v_count<V_ACTIVE_END;
wire framebuffer_x = active;
wire line_ready = publish_sync[display_slot] != released[display_slot];
reg scan_line_ready=0, scan_linear=0;
reg visible_pipe3=0, framebuffer_pipe3=0;
reg hsync_pipe3=0, vsync_pipe3=0;
function [PRECODE_BITS-1:0] encode_byte;
    input [7:0] data;
    integer bit_index, ones, count;
    reg xnor_mode;
    reg [8:0] q;
    begin
        ones=0;
        for(bit_index=0;bit_index<8;bit_index=bit_index+1) ones=ones+data[bit_index];
        xnor_mode=ones>4 || (ones==4 && !data[0]);
        q[0]=data[0]; q[8]=!xnor_mode;
        for(bit_index=1;bit_index<8;bit_index=bit_index+1)
            q[bit_index]=q[bit_index-1]^data[bit_index]^xnor_mode;
        count=0;
        for(bit_index=0;bit_index<8;bit_index=bit_index+1) count=count+q[bit_index];
        encode_byte=q;
        if(PRECODE_BITS==13) encode_byte=encode_byte | (count<<9);
    end
endfunction
localparam [3*PRECODE_BITS-1:0] BLACK_PRECODE={3{encode_byte(8'd0)}};
reg [3*PRECODE_BITS-1:0] precode_pipe=BLACK_PRECODE;
`ifdef __ICARUS__
// Test-only inverse feeds the independent RGB oracle and old encoder miter.
// The production datapath transports only precodes.
function [7:0] decode_byte;
    input [PRECODE_BITS-1:0] q;
    integer bit_index;
    begin
        decode_byte[0]=q[0];
        for(bit_index=1;bit_index<8;bit_index=bit_index+1)
            decode_byte[bit_index]=q[bit_index]^q[bit_index-1]^~q[8];
    end
endfunction
wire [23:0] rgb_pipe={decode_byte(precode_pipe[3*PRECODE_BITS-1:2*PRECODE_BITS]),
    decode_byte(precode_pipe[2*PRECODE_BITS-1:PRECODE_BITS]),
    decode_byte(precode_pipe[PRECODE_BITS-1:0])};
`endif
// Independent bank addresses use seven of eight read slots per pixel pair:
// source qword, then two R, two G, two B lookups. The next source read
// overlaps the previous pair's blue capture. Initiation interval is four.
// The consumer's fixed cadence owns the entire source/R/G/B calendar.
// There is no independently stalled stage within a ready output row.
reg [1:0] read_phase=0;
reg [7:0] fetch_pair=0;
reg pair_bank=0;
reg [31:0] raw_pair=0;
reg [PRECODE_BITS-1:0] r0=0,r1=0,g0=0,g1=0;
wire [31:0] source_pair = pair_bank ? line_read_data_b : line_read_data_a;
wire [5:0] source_r0 = {source_pair[15:11],source_pair[15]};
wire [5:0] source_r1 = {source_pair[31:27],source_pair[31]};
wire [5:0] raw_g0 = raw_pair[10:5], raw_g1 = raw_pair[26:21];
wire [5:0] raw_b0 = {raw_pair[4:0],raw_pair[4]};
wire [5:0] raw_b1 = {raw_pair[20:16],raw_pair[20]};
// One completed pair is the only output holding state. Its first pixel
// is consumed directly at completion; subsequent repeats use this register.
reg [6*PRECODE_BITS-1:0] output_pair=0;
wire conversion_active = scan_enabled && scan_line_ready &&
    v_count>=V_ACTIVE_START && v_count<V_ACTIVE_END &&
    h_count>=H_ACTIVE_START-11'd4 && h_count<H_ACTIVE_END;
wire issue_pair = conversion_active && read_phase==0 &&
    h_count<H_ACTIVE_END-11'd4;
wire emit_pair = conversion_active && h_count>=H_ACTIVE_START &&
    read_phase==0;
wire output_second = read_phase[1];
wire [6*PRECODE_BITS-1:0] converted_pair =
    {r1,g1,line_read_data_b[PRECODE_BITS-1:0],r0,g0,line_read_data_a[PRECODE_BITS-1:0]};
wire [6*PRECODE_BITS-1:0] current_pair = emit_pair ? converted_pair : output_pair;
wire [3*PRECODE_BITS-1:0] current_precode = output_second ?
    current_pair[6*PRECODE_BITS-1:3*PRECODE_BITS] : current_pair[3*PRECODE_BITS-1:0];
always @* begin
    line_read_address_a=display_slot_base+{2'b0,fetch_pair[7:1]};
    line_read_address_b=line_read_address_a;
    case (read_phase)
        1: begin
            if(scan_linear) begin line_read_address_a={3'b111,source_r0}; line_read_address_b={3'b111,source_r1}; end
            else begin line_read_address_a={4'b1101,source_pair[15:11]}; line_read_address_b={4'b1101,source_pair[31:27]}; end
        end
        2: begin line_read_address_a={3'b111,raw_g0}; line_read_address_b={3'b111,raw_g1}; end
        3: begin
            if(scan_linear) begin line_read_address_a={3'b111,raw_b0}; line_read_address_b={3'b111,raw_b1}; end
            else begin line_read_address_a={4'b1101,raw_pair[4:0]}; line_read_address_b={4'b1101,raw_pair[20:16]}; end
        end
        default: begin end
    endcase
end
always @(posedge pixel_clock) begin
    publish_meta<=published; publish_sync<=publish_meta;
    if (pixel_reset) begin
        h_count<=0; v_count<=0; released<=0; display_slot<=0; display_second<=0; frame_toggle<=0;
        vertical_repeat<=0; started<=0; underflow_sticky<=0;
        visible_pipe3<=0; framebuffer_pipe3<=0; hsync_pipe3<=0; vsync_pipe3<=0; precode_pipe<=BLACK_PRECODE;
        scan_line_ready<=0; scan_linear<=0; read_phase<=0; fetch_pair<=0;

    end else begin
        if (h_count==H_TOTAL-1) begin
            h_count<=0;
            if (scan_enabled && v_count==V_ACTIVE_END-1) frame_toggle<=~frame_toggle;
            if (v_count==V_TOTAL-1) begin
                v_count<=0;
                if (!started && enable_sync[1] && !failure_sync[1] &&
                    (publish_sync ^ released)==2'b11) started<=1;
            end else v_count<=v_count+1'b1;
        end else h_count<=h_count+1'b1;
        visible_pipe3<=active;
        framebuffer_pipe3<=scan_enabled && framebuffer_x;
        hsync_pipe3<=hsync; vsync_pipe3<=vsync;
        precode_pipe<=BLACK_PRECODE;
        if (scan_enabled && active && scan_line_ready) precode_pipe<=current_precode;
        if (h_count==0) begin
            // Readiness is sampled for the whole output row. A late publish
            // cannot reveal a partial row or consume data from a failed fill.
            scan_line_ready<=scan_enabled && line_ready; scan_linear<=group_linear[display_slot];
            read_phase<=0; fetch_pair<=0;

        end else if (conversion_active) begin
            read_phase<=read_phase+1'b1;
            if (issue_pair) begin
                pair_bank<=fetch_pair[0]; fetch_pair<=fetch_pair+1'b1;
            end
            // At edge S the RAM produces source Q. At S+1 the old source Q
            // supplies R addresses and raw_pair; S+2/S+3 capture old R/G Q.
            // S+4 consumes old B Q directly and issues the next source read.
            if (h_count<H_ACTIVE_END-11'd4) begin
                case (read_phase)
                    1: raw_pair<=source_pair;
                    2: begin r0<=line_read_data_a[PRECODE_BITS-1:0]; r1<=line_read_data_b[PRECODE_BITS-1:0]; end
                    3: begin
                        if(scan_linear) begin g0<=line_read_data_a[PRECODE_BITS-1:0]; g1<=line_read_data_b[PRECODE_BITS-1:0]; end
                        else begin g0<=line_read_data_a[2*PRECODE_BITS-1:PRECODE_BITS]; g1<=line_read_data_b[2*PRECODE_BITS-1:PRECODE_BITS]; end
                    end
                    default: begin end
                endcase
            end
            if (emit_pair) output_pair<=converted_pair;
        end
        if (scan_enabled && h_count==H_ACTIVE_END-1 && v_count>=V_ACTIVE_START && v_count<V_ACTIVE_END) begin
            if (vertical_repeat) begin
                vertical_repeat<=0;
                if (scan_line_ready) begin
                    display_second<=~display_second;
                    if (display_second) begin
                        released[display_slot]<=~released[display_slot]; display_slot<=~display_slot;
                    end
                end else underflow_sticky<=1;
            end else vertical_repeat<=1;
        end
    end
end
always @* begin
    device_read_data=0;
    if (device_read_enable && device_index==DISPLAY_DEVICE) begin
        case (device_channel)
            0: device_read_data=frame_index;
            1: device_read_data=active_base[15:0];
            2: device_read_data={10'b0,active_base[21:16]};
            3: device_read_data={11'b0,memory_error_sticky,underflow_sync,
                invalid_address,(next_low_written ^ next_high_written),next_pending};
            4: device_read_data={15'b0,active_linear};
            default: device_read_data=0;
        endcase
    end
end
assign underflow = underflow_sticky | memory_error_sticky;
wire [PRECODE_BITS-1:0] red=precode_pipe[3*PRECODE_BITS-1:2*PRECODE_BITS];
wire [PRECODE_BITS-1:0] green=precode_pipe[2*PRECODE_BITS-1:PRECODE_BITS];
wire [PRECODE_BITS-1:0] blue=precode_pipe[PRECODE_BITS-1:0];

wire [9:0] blue_symbol,green_symbol,red_symbol;
HdmiTmdsEncoder #(.PRECODE_BITS(PRECODE_BITS)) u_blue(.clk(pixel_clock),.reset(pixel_reset),.de(visible_pipe3),
 .control({vsync_pipe3,hsync_pipe3}),.precode(blue),.symbol(blue_symbol));
HdmiTmdsEncoder #(.PRECODE_BITS(PRECODE_BITS)) u_green(.clk(pixel_clock),.reset(pixel_reset),.de(visible_pipe3),
 .control(2'b00),.precode(green),.symbol(green_symbol));
HdmiTmdsEncoder #(.PRECODE_BITS(PRECODE_BITS)) u_red(.clk(pixel_clock),.reset(pixel_reset),.de(visible_pipe3),
 .control(2'b00),.precode(red),.symbol(red_symbol));
wire [3:0] serialized;
`ifdef __ICARUS__
assign serialized={pixel_clock,red_symbol[0],green_symbol[0],blue_symbol[0]};
assign tmds_clk_p=serialized[3]; assign tmds_clk_n=~serialized[3];
assign tmds_data_p=serialized[2:0]; assign tmds_data_n=~serialized[2:0];
`else
HdmiSerializer10 sb(.pixel_clk(pixel_clock),.serial_clk(serial_clock),.data(blue_symbol),.serial(serialized[0]));
HdmiSerializer10 sg(.pixel_clk(pixel_clock),.serial_clk(serial_clock),.data(green_symbol),.serial(serialized[1]));
HdmiSerializer10 sr(.pixel_clk(pixel_clock),.serial_clk(serial_clock),.data(red_symbol),.serial(serialized[2]));
HdmiSerializer10 sc(.pixel_clk(pixel_clock),.serial_clk(serial_clock),.data(10'b0000011111),.serial(serialized[3]));
ELVDS_OBUF ob0(.I(serialized[3]),.O(tmds_clk_p),.OB(tmds_clk_n));
ELVDS_OBUF ob1(.I(serialized[0]),.O(tmds_data_p[0]),.OB(tmds_data_n[0]));
ELVDS_OBUF ob2(.I(serialized[1]),.O(tmds_data_p[1]),.OB(tmds_data_n[1]));
ELVDS_OBUF ob3(.I(serialized[2]),.O(tmds_data_p[2]),.OB(tmds_data_n[2]));
`endif
endmodule

module HdmiTmdsEncoder #(parameter PRECODE_BITS=9)(input wire clk,input wire reset,input wire de,
 input wire [1:0] control,input wire [PRECODE_BITS-1:0] precode,output reg [9:0] symbol);
wire [8:0] qm=precode[8:0];
wire [3:0] qm_ones;
generate if(PRECODE_BITS==13) begin
 assign qm_ones=precode[12:9];
end else begin
 assign qm_ones=qm[0]+qm[1]+qm[2]+qm[3]+qm[4]+qm[5]+qm[6]+qm[7];
end endgenerate
reg signed [5:0] disparity=0;
wire signed [5:0] qm_delta=$signed({1'b0,qm_ones,1'b0})-6'sd8;
always @(posedge clk) begin
 if(reset) begin disparity<=0; symbol<=10'b1101010100; end
 else if(!de) begin disparity<=0; case(control)
  0:symbol<=10'b1101010100; 1:symbol<=10'b0010101011;
  2:symbol<=10'b0101010100; default:symbol<=10'b1010101011; endcase end
 else if(disparity==0 || qm_ones==4) begin
  symbol[9]<=~qm[8]; symbol[8]<=qm[8]; symbol[7:0]<=qm[8]?qm[7:0]:~qm[7:0];
  if(qm[8]) disparity<=disparity+qm_delta; else disparity<=disparity-qm_delta;
 // The unsized alternatives below are literal zero, so narrowing them to the
 // six-bit signed disparity datapath is exact. Sized zero changes Gowin's
 // signed-expression inference and costs substantial logic on GW2AR-18.
 end else if((disparity>0&&qm_ones>4)||(disparity<0&&qm_ones<4)) begin
  symbol<={1'b1,qm[8],~qm[7:0]}; disparity<=disparity-qm_delta+(qm[8]?6'sd2:0); // gowin-lint: allow EX3791
 end else begin symbol<={1'b0,qm[8],qm[7:0]}; disparity<=disparity+qm_delta-(qm[8]?0:6'sd2); end // gowin-lint: allow EX3791
end endmodule
`ifndef __ICARUS__
module HdmiSerializer10(input wire pixel_clk,input wire serial_clk,input wire [9:0] data,output wire serial);
OSER10 o(.Q(serial),.D0(data[0]),.D1(data[1]),.D2(data[2]),.D3(data[3]),.D4(data[4]),
 .D5(data[5]),.D6(data[6]),.D7(data[7]),.D8(data[8]),.D9(data[9]),
 .PCLK(pixel_clk),.FCLK(serial_clk),.RESET(1'b0));
defparam o.GSREN="false"; defparam o.LSREN="true";
endmodule
`endif
