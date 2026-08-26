module FramebufferHdmi(
    input wire clk, input wire reset,
    input wire pixel_clock, input wire serial_clock, input wire video_locked,
    input wire memory_request_ready, input wire memory_data_valid,
    input wire [31:0] memory_read_data, input wire memory_last, input wire memory_error,
    output wire memory_request_valid, output wire memory_urgent,
    output wire [21:0] memory_address, output wire underflow,
    output wire tmds_clk_p, output wire tmds_clk_n,
    output wire [2:0] tmds_data_p, output wire [2:0] tmds_data_n
);
localparam [21:0] FB_BASE=22'h200100;
reg [2:0] published=0;
reg [2:0] released=0;
reg [2:0] release_meta=0, release_sync=0;
reg [1:0] fill_slot=0;
reg [7:0] fill_y=0;
reg [21:0] row_address=FB_BASE;
reg [4:0] burst_index=0;
reg [2:0] beat_index=0;
reg burst_active=0;
reg memory_error_sticky=0;
wire fill_slot_free = published[fill_slot] == release_sync[fill_slot];
wire [1:0] ready_count =
    (published[0] != release_sync[0]) +
    (published[1] != release_sync[1]) +
    (published[2] != release_sync[2]);
assign memory_urgent = ready_count <= 1;
assign memory_request_valid = fill_slot_free && !burst_active && !memory_error_sticky;
assign memory_address = row_address + {13'b0,burst_index,4'b0};

wire line_write = burst_active && memory_data_valid;
wire [8:0] line_write_address = fill_slot * 9'd160 + burst_index * 9'd8 + beat_index;
reg [8:0] line_read_address=0;
wire [31:0] line_read_data;
__LINE_BUFFER__ u_line_buffer(
    .write_clock(clk), .write_enable(line_write), .write_address(line_write_address),
    .write_data(memory_read_data), .read_clock(pixel_clock),
    .read_address(line_read_address), .read_data(line_read_data)
);

always @(posedge clk) begin
    release_meta <= released;
    release_sync <= release_meta;
    if (reset) begin
        published<=0; fill_slot<=0; fill_y<=0; row_address<=FB_BASE;
        burst_index<=0; beat_index<=0; burst_active<=0; memory_error_sticky<=0;
    end else begin
        if (memory_error) memory_error_sticky<=1;
        if (memory_request_valid && memory_request_ready) begin
            burst_active<=1; beat_index<=0;
        end
        if (line_write) begin
            if (memory_last || beat_index==7) begin
                burst_active<=0; beat_index<=0;
                if (burst_index==19) begin
                    published[fill_slot] <= ~published[fill_slot];
                    burst_index<=0;
                    fill_slot <= fill_slot==2 ? 0 : fill_slot+1'b1;
                    if (fill_y==239) begin fill_y<=0; row_address<=FB_BASE; end
                    // Rows 0..202 end at physical 0x20FEBF; the 0xFF00 offset
                    // page is the fixed MMIO window, so the framebuffer
                    // continues in segment 0x21 and row 203 starts at
                    // physical 0x210000.
                    else if (fill_y==202) begin fill_y<=fill_y+1'b1; row_address<=22'h210000; end
                    else begin fill_y<=fill_y+1'b1; row_address<=row_address+22'd320; end
                end else burst_index<=burst_index+1'b1;
            end else beat_index<=beat_index+1'b1;
        end
    end
end

reg [2:0] publish_meta=0, publish_sync=0;
// The board reset and the video PLL lock live outside the pixel domain.
// Synchronize their release into pixel_clock before any pixel-domain logic
// consumes them; the synchronizer input is an intended asynchronous crossing.
reg [2:0] pixel_reset_sync=0;
always @(posedge pixel_clock)
    pixel_reset_sync <= {pixel_reset_sync[1:0], ~(reset | ~video_locked)};
wire pixel_reset = ~pixel_reset_sync[2];
reg [1:0] display_slot=0;
reg [1:0] vertical_repeat=0;
reg started=0, underflow_sticky=0;
reg [10:0] h_count=0;
reg [9:0] v_count=0;
wire hsync = h_count < 40;
wire vsync = v_count < 5;
wire active = h_count>=260 && h_count<1540 && v_count>=25 && v_count<745;
wire [10:0] active_x = h_count-260;
wire [9:0] active_y = v_count-25;
wire framebuffer_x = active && active_x>=160 && active_x<1120;
wire [9:0] scaled_x = active_x-160;
// Counter division by three is confined to the timing RTL; synthesis reduces
// these fixed-width divisions to ordinary logic.
wire [8:0] source_x = scaled_x / 3;
wire line_ready = publish_sync[display_slot] != released[display_slot];
wire visible_request = started && framebuffer_x && line_ready;
reg visible_pipe=0, visible_pipe2=0, visible_pipe3=0;
reg lane_pipe=0, lane_pipe2=0;
reg hsync_pipe=0, hsync_pipe2=0, hsync_pipe3=0;
reg vsync_pipe=0, vsync_pipe2=0, vsync_pipe3=0;
reg [15:0] pixel565_pipe=0;
// The lane select uses the twice-delayed lane so it matches the line buffer's
// two-cycle address-to-data latency.
wire [15:0] pixel565 = lane_pipe2 ? line_read_data[31:16] : line_read_data[15:0];

always @(posedge pixel_clock) begin
    publish_meta<=published; publish_sync<=publish_meta;
    if (pixel_reset) begin
        h_count<=0; v_count<=0; released<=0; display_slot<=0;
        vertical_repeat<=0; started<=0; underflow_sticky<=0;
        visible_pipe<=0; visible_pipe2<=0; visible_pipe3<=0;
        lane_pipe<=0; lane_pipe2<=0;
        hsync_pipe<=0; hsync_pipe2<=0; hsync_pipe3<=0;
        vsync_pipe<=0; vsync_pipe2<=0; vsync_pipe3<=0;
        pixel565_pipe<=0;
    end else begin
        if (h_count==1649) begin
            h_count<=0;
            if (v_count==749) begin
                v_count<=0;
                if (!started && publish_sync!=released) started<=1;
            end else v_count<=v_count+1'b1;
        end else h_count<=h_count+1'b1;
        if (framebuffer_x)
            line_read_address <= display_slot*9'd160 + source_x[8:1];
        // Pipeline alignment: the line buffer data for a position arrives two
        // pixel clocks late (address register, then synchronous RAM read), so
        // the lane select and the visible/sync strobes are delayed to match,
        // and the registered pixel word adds a third stage for the encoders.
        lane_pipe<=source_x[0]; lane_pipe2<=lane_pipe;
        visible_pipe<=visible_request; visible_pipe2<=visible_pipe; visible_pipe3<=visible_pipe2;
        hsync_pipe<=hsync; hsync_pipe2<=hsync_pipe; hsync_pipe3<=hsync_pipe2;
        vsync_pipe<=vsync; vsync_pipe2<=vsync_pipe; vsync_pipe3<=vsync_pipe2;
        pixel565_pipe<=pixel565;
        if (started && h_count==1539 && v_count>=25 && v_count<745) begin
            if (vertical_repeat==2) begin
                vertical_repeat<=0;
                if (line_ready) begin
                    released[display_slot]<=~released[display_slot];
                    display_slot<=display_slot==2 ? 0 : display_slot+1'b1;
                end else underflow_sticky<=1;
            end else vertical_repeat<=vertical_repeat+1'b1;
        end
    end
end
assign underflow = underflow_sticky | memory_error_sticky;
// Register the pixel word before the RGB565 expansion and the TMDS encode:
// the line-buffer read, lane mux, color expansion, and transition-minimized
// encode do not meet the pixel clock as one combinational path. All video
// signals receive the same three-stage delay.
wire [7:0] red,green,blue;
__RGB565__ u_rgb(.pixel(pixel565_pipe),.visible(visible_pipe3),.red(red),.green(green),.blue(blue));

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
 end else if((disparity>0&&qm_ones>4)||(disparity<0&&qm_ones<4)) begin
  symbol<={1'b1,qm[8],~qm[7:0]}; disparity<=disparity-qm_delta+(qm[8]?6'sd2:0);
 end else begin symbol<={1'b0,qm[8],qm[7:0]}; disparity<=disparity+qm_delta-(qm[8]?0:6'sd2); end
end endmodule
`ifndef __ICARUS__
module HdmiSerializer10(input wire pixel_clk,input wire serial_clk,input wire [9:0] data,output wire serial);
OSER10 o(.Q(serial),.D0(data[0]),.D1(data[1]),.D2(data[2]),.D3(data[3]),.D4(data[4]),
 .D5(data[5]),.D6(data[6]),.D7(data[7]),.D8(data[8]),.D9(data[9]),
 .PCLK(pixel_clk),.FCLK(serial_clk),.RESET(1'b0));
defparam o.GSREN="false"; defparam o.LSREN="true";
endmodule
`endif
