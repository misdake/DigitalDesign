module FramebufferHdmi(
    input wire clk, input wire reset,
    input wire pixel_clock, input wire serial_clock, input wire video_locked,
    input wire memory_request_ready, input wire memory_data_valid,
    input wire [63:0] memory_read_data, input wire memory_last, input wire memory_error,
    input wire [2:0] device_index, input wire [3:0] device_channel,
    input wire device_read_enable, input wire device_write_enable,
    input wire [15:0] device_write_data,
    output wire memory_request_valid, output wire memory_urgent,
    output wire [21:0] memory_address, output wire underflow,
    output reg [15:0] device_read_data,
    output wire tmds_clk_p, output wire tmds_clk_n,
    output wire [2:0] tmds_data_p, output wire [2:0] tmds_data_n
);
localparam [21:0] FB_BASE=22'h200000;
localparam [2:0] DISPLAY_DEVICE=3'd3;
localparam [31:0] LAST_VALID_FB_BASE=32'h003e8900;
localparam [15:0] BORDER_COLOR=16'h1082;
// Compile-time scanout configuration. The active mode's localparam block is
// injected by the Rust host model so the RTL, testbench, and board video PLL
// all derive from the single `ACTIVE_DISPLAY_CONFIG` constant.
__DISPLAY_CONFIG__
// Two published groups, each containing two source rows. A group is released
// only after the final vertical repeat of its second row. Toggle metadata is
// stable until release and crosses through the publish synchronizer.
reg [1:0] published=0, released=0;
reg [1:0] release_meta=0, release_sync=0;
reg fill_slot=0;
// fill_y/local_y identify the even first row of a two-row group. Fetch both
// rows of one tile before advancing burst_index to the next tile column.
reg [7:0] fill_y=0;
reg [21:0] active_base=FB_BASE, tile_row_base=FB_BASE;
reg [3:0] local_y=0;
reg [31:0] shadow_base=32'h00200000;
reg [21:0] pending_base=FB_BASE;
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
assign memory_request_valid = fill_slot_free && !burst_active &&
                              !frame_complete && !memory_error_sticky;
// Adjacent rows of each tile are fetched consecutively. The arbiter retains
// its fixed 32-byte display transactions: the pair is two requests, not a
// longer non-preemptible owner interval.
assign memory_address = tile_row_base + {9'b0,burst_index,8'b0} +
                        {14'b0,local_y,4'b0} + (fill_second ? 22'd16 : 22'd0);
// Each shared address holds four RGB565 pixels across the two banks. One row
// uses 100 addresses, one group 200; addresses 400..447 are unused and 448..511
// are the read-only sRGB tables. SDRAM beats can therefore bypass staging.
wire [8:0] fill_slot_base = fill_slot ? 9'd200 : 9'd0;
wire [8:0] line_write_address = fill_slot_base +
    (fill_second ? 9'd100 : 9'd0) + {2'b0,burst_index,2'b00} + {7'b0,beat_index};
wire line_write = burst_active && memory_data_valid && !memory_error &&
                  !memory_error_sticky && !reset;
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
        published<=0; fill_slot<=0; fill_y<=0; active_base<=FB_BASE;
        tile_row_base<=FB_BASE; local_y<=0;
        shadow_base<=32'h00200000; pending_base<=FB_BASE;
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
                fill_y<=0;
                local_y<=0;
                frame_complete<=0;
                if (next_pending) begin
                    active_base<=pending_base; active_linear<=pending_linear;
                    tile_row_base<=pending_base;
                    next_pending<=0;
                end else tile_row_base<=active_base;
            end
        end
        // Process device writes after the frame event so a NEXT_SWAP arriving
        // on the same clock edge remains pending for the following frame. The
        // active frame still receives the previously submitted address.
        if (device_write_enable && device_index==DISPLAY_DEVICE) begin
            if (device_channel==4'd1) begin
                shadow_base[15:0]<=device_write_data;
                next_low_written<=1;
                invalid_address<=0;
            end else if (device_channel==4'd2) begin
                shadow_base[31:16]<=device_write_data;
                next_high_written<=1;
                invalid_address<=0;
            end else if (device_channel==4'd4) begin
                shadow_linear<=device_write_data[0];
            end else if (device_channel==4'd3 && device_write_data==16'd1 &&
                         next_low_written && next_high_written) begin
                next_low_written<=0;
                next_high_written<=0;
                if (shadow_base[31:22]==0 && shadow_base[3:0]==0 &&
                    shadow_base<=LAST_VALID_FB_BASE) begin
                    pending_base<=shadow_base[21:0]; pending_linear<=shadow_linear;
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
                            fill_y<=fill_y+8'd2;
                            if (local_y==4'd14) begin
                                local_y<=0; tile_row_base<=tile_row_base+TILE_ROW_STRIDE;
                            end else local_y<=local_y+4'd2;
                        end
                    end else burst_index<=burst_index+1'b1;
                end
            end else beat_index<=beat_index+1'b1;
        end
    end
end
reg [1:0] publish_meta=0, publish_sync=0;
// Reset/PLL lock are asynchronous to pixel_clock. Delay their release here
// before scanout consumes the line-group publication toggles or queue state.
reg [2:0] pixel_reset_sync=0;
always @(posedge pixel_clock)
    pixel_reset_sync <= {pixel_reset_sync[1:0], ~(reset | ~video_locked)};
wire pixel_reset = ~pixel_reset_sync[2];
reg display_slot=0, display_second=0;
wire [8:0] display_slot_base = (display_slot ? 9'd200 : 9'd0) +
                                             (display_second ? 9'd100 : 9'd0);
reg [1:0] vertical_repeat=0;
reg started=0;
reg [10:0] h_count=0;
reg [9:0] v_count=0;
wire hsync = h_count < H_SYNC_END;
wire vsync = v_count < V_SYNC_END;
wire active = h_count>=H_ACTIVE_START && h_count<H_ACTIVE_END &&
               v_count>=V_ACTIVE_START && v_count<V_ACTIVE_END;
wire framebuffer_x = active && h_count>=H_ACTIVE_START+SIDE_BORDER &&
                     h_count<H_ACTIVE_END-SIDE_BORDER;
wire line_ready = publish_sync[display_slot] != released[display_slot];
reg scan_line_ready=0, scan_linear=0;
reg visible_pipe3=0, framebuffer_pipe3=0;
reg hsync_pipe3=0, vsync_pipe3=0;
reg [23:0] rgb_pipe=0;
// Independent bank addresses use seven of eight read slots per pixel pair:
// source qword, then two R, two G, two B lookups. The next source read
// overlaps the previous pair's blue capture. Initiation interval is four.
reg [2:0] read_phase=0;
reg [7:0] fetch_pair=0;
reg pair_bank=0;
reg [31:0] raw_pair=0;
reg [7:0] r0=0,r1=0,g0=0,g1=0;
wire [31:0] source_pair = pair_bank ? line_read_data_b : line_read_data_a;
wire [5:0] source_r0 = {source_pair[15:11],source_pair[15]};
wire [5:0] source_r1 = {source_pair[31:27],source_pair[31]};
wire [5:0] raw_g0 = raw_pair[10:5], raw_g1 = raw_pair[26:21];
wire [5:0] raw_b0 = {raw_pair[4:0],raw_pair[4]};
wire [5:0] raw_b1 = {raw_pair[20:16],raw_pair[20]};
reg [1:0] fifo_write=0, fifo_read=0;
reg [2:0] fifo_count=0;
// Converted pairs enter a four-slot FIFO; the output register retains a popped
// pair while its two pixels repeat SCALE times. Both sides use pixel_clock.
// Conversion can start every four clocks; consumption takes four clocks in 2x
// and six in 3x, so 3x relies on the occupancy guard to pause conversion.
reg [47:0] output_pair=0;
reg [2:0] output_repeat=0;
reg output_second=0;
wire push_pair = read_phase==4;
wire pop_pair = started && framebuffer_x && scan_line_ready &&
                output_repeat==0 && !output_second && fifo_count!=0;
// Starting with at most two queued pairs reserves room for the pair still in
// flight. At phase 4, the previous push can overlap the next source read, so
// admission must account for that push before the new conversion completes.
// Sixteen clocks of row lead-in prefill the queue before the first pop.
wire start_pair = (read_phase==0 || read_phase==4) && started && scan_line_ready &&
    v_count>=V_ACTIVE_START && v_count<V_ACTIVE_END &&
    h_count>=H_ACTIVE_START+SIDE_BORDER-16 && h_count<H_ACTIVE_END-SIDE_BORDER &&
    fetch_pair<FB_WIDTH/2 && fifo_count<3;
wire [47:0] fifo_head;
wire [47:0] current_pair = pop_pair ? fifo_head : output_pair;
wire [23:0] current_rgb = output_second ? current_pair[47:24] : current_pair[23:0];
function [23:0] expand565;
    input [15:0] p;
    begin expand565={p[15:11],p[15:13],p[10:5],p[10:9],p[4:0],p[4:2]}; end
endfunction
wire [47:0] fifo_write_data = scan_linear ?
    {r1,g1,line_read_data_b[7:0],r0,g0,line_read_data_a[7:0]} :
    {expand565(raw_pair[31:16]),expand565(raw_pair[15:0])};
__PAIR_FIFO__ u_pair_fifo(.write_clock(pixel_clock),
    .write_enable(push_pair && !pixel_reset && h_count!=0),
    .write_address(fifo_write), .write_data(fifo_write_data),
    .read_address(fifo_read), .read_data(fifo_head));
always @* begin
    line_read_address_a=display_slot_base+{2'b0,fetch_pair[7:1]};
    line_read_address_b=line_read_address_a;
    case (read_phase)
        1: begin line_read_address_a={3'b111,source_r0}; line_read_address_b={3'b111,source_r1}; end
        2: begin line_read_address_a={3'b111,raw_g0}; line_read_address_b={3'b111,raw_g1}; end
        3: begin line_read_address_a={3'b111,raw_b0}; line_read_address_b={3'b111,raw_b1}; end
        default: begin end
    endcase
end
always @(posedge pixel_clock) begin
    publish_meta<=published; publish_sync<=publish_meta;
    if (pixel_reset) begin
        h_count<=0; v_count<=0; released<=0; display_slot<=0; display_second<=0; frame_toggle<=0;
        vertical_repeat<=0; started<=0; underflow_sticky<=0;
        visible_pipe3<=0; framebuffer_pipe3<=0; hsync_pipe3<=0; vsync_pipe3<=0; rgb_pipe<=0;
        scan_line_ready<=0; scan_linear<=0; read_phase<=0; fetch_pair<=0;
        fifo_write<=0; fifo_read<=0; fifo_count<=0; output_repeat<=0; output_second<=0;
    end else begin
        if (h_count==H_TOTAL-1) begin
            h_count<=0;
            if (v_count==V_ACTIVE_END-1) frame_toggle<=~frame_toggle;
            if (v_count==V_TOTAL-1) begin
                v_count<=0;
                if (!started && publish_sync!=released) started<=1;
            end else v_count<=v_count+1'b1;
        end else h_count<=h_count+1'b1;
        visible_pipe3<=started && active;
        framebuffer_pipe3<=started && framebuffer_x;
        hsync_pipe3<=hsync; vsync_pipe3<=vsync;
        rgb_pipe<=0;
        if (started && active) begin
            if (!framebuffer_x) rgb_pipe<=expand565(BORDER_COLOR);
            else if (scan_line_ready) begin
                if (output_repeat==0 && !output_second && fifo_count==0) underflow_sticky<=1;
                else rgb_pipe<=current_rgb;
                if (output_repeat==SCALE-1) begin
                    output_repeat<=0; output_second<=~output_second;
                end else output_repeat<=output_repeat+1'b1;
            end
        end
        if (h_count==0) begin
            // Readiness is sampled for the whole output row. A late publish
            // cannot reveal a partial row or consume data from a failed fill.
            scan_line_ready<=line_ready; scan_linear<=group_linear[display_slot];
            read_phase<=0; fetch_pair<=0; fifo_write<=0; fifo_read<=0; fifo_count<=0;
            output_repeat<=0; output_second<=0;
        end else begin
            case (read_phase)
                1: begin raw_pair<=source_pair; read_phase<=2; end
                2: begin r0<=line_read_data_a[7:0]; r1<=line_read_data_b[7:0]; read_phase<=3; end
                3: begin g0<=line_read_data_a[7:0]; g1<=line_read_data_b[7:0]; read_phase<=4; end
                default: begin
                    read_phase<=0;
                    if (start_pair) begin
                        pair_bank<=fetch_pair[0]; fetch_pair<=fetch_pair+1'b1; read_phase<=1;
                    end
                end
            endcase
            if (push_pair) begin
                fifo_write<=fifo_write+1'b1;
            end
            // The asynchronous old head feeds this edge's RGB and is retained
            // for subsequent repeats. Simultaneous push/pop preserves count.
            if (pop_pair) begin output_pair<=fifo_head; fifo_read<=fifo_read+1'b1; end
            case ({push_pair,pop_pair})
                2'b10: fifo_count<=fifo_count+1'b1;
                2'b01: fifo_count<=fifo_count-1'b1;
                default: begin end
            endcase
        end
        if (started && h_count==H_ACTIVE_END-1 && v_count>=V_ACTIVE_START && v_count<V_ACTIVE_END) begin
            if (vertical_repeat==LAST_REPEAT) begin
                vertical_repeat<=0;
                if (scan_line_ready) begin
                    display_second<=~display_second;
                    if (display_second) begin
                        released[display_slot]<=~released[display_slot]; display_slot<=~display_slot;
                    end
                end else underflow_sticky<=1;
            end else vertical_repeat<=vertical_repeat+1'b1;
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
wire [7:0] red=rgb_pipe[23:16], green=rgb_pipe[15:8], blue=rgb_pipe[7:0];

wire [9:0] blue_symbol,green_symbol,red_symbol;
HdmiTmdsEncoder u_blue(.clk(pixel_clock),.reset(pixel_reset),.de(visible_pipe3),
 .control({vsync_pipe3,hsync_pipe3}),.data(blue),.symbol(blue_symbol));
HdmiTmdsEncoder u_green(.clk(pixel_clock),.reset(pixel_reset),.de(visible_pipe3),
 .control(2'b00),.data(green),.symbol(green_symbol));
HdmiTmdsEncoder u_red(.clk(pixel_clock),.reset(pixel_reset),.de(visible_pipe3),
 .control(2'b00),.data(red),.symbol(red_symbol));
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

module HdmiTmdsEncoder(input wire clk,input wire reset,input wire de,
 input wire [1:0] control,input wire [7:0] data,output reg [9:0] symbol);
integer i; reg [3:0] data_ones,qm_ones; reg [8:0] qm; reg signed [5:0] disparity=0;
wire signed [5:0] qm_delta=$signed({1'b0,qm_ones,1'b0})-6'sd8;
always @* begin
 data_ones=data[0]+data[1]+data[2]+data[3]+data[4]+data[5]+data[6]+data[7]; qm[0]=data[0];
 if(data_ones>4 || (data_ones==4 && data[0]==0)) begin
  for(i=1;i<8;i=i+1) qm[i]=~(qm[i-1]^data[i]); qm[8]=0;
 end else begin for(i=1;i<8;i=i+1) qm[i]=qm[i-1]^data[i]; qm[8]=1; end
 qm_ones=qm[0]+qm[1]+qm[2]+qm[3]+qm[4]+qm[5]+qm[6]+qm[7];
end
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
