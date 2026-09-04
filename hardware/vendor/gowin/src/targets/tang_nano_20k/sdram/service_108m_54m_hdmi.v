wire video_serial_clock;
wire video_pixel_clock;
wire video_locked;
TangNano20KVideoPll720p u_video_pll(
    .clkin(clk), .serial_clock(video_serial_clock),
    .pixel_clock(video_pixel_clock), .locked(video_locked)
);

wire logic_clk;
wire controller_clk;
wire sdram_clk_180;
wire sdram_pll_locked;
reg [1:0] sdram_reset_sync = 2'b00;
wire [31:0] controller_read_data_32;
wire controller_init_done_108;
wire controller_command_ack_108;
reg controller_command_valid_108 = 0;
reg [2:0] controller_command_108 = 3'b111;
reg controller_precharge_108 = 0;
reg [20:0] controller_address_108 = 0;
reg [3:0] controller_write_mask_108 = 0;
reg [7:0] controller_burst_length_108 = 0;
reg command_seen_108 = 0;

wire sdram_command_valid;
wire [2:0] sdram_command;
wire sdram_precharge;
wire [20:0] sdram_address;
wire [3:0] sdram_write_mask;
wire [63:0] sdram_write_data;
wire sdram_write_data_valid;
wire [7:0] sdram_burst_length;
reg [63:0] sdram_read_data = 0;
reg sdram_read_valid = 0;
reg sdram_command_ack = 0;
wire sdram_init_done;
wire sdram_write_data_ready = 1'b1;

(* syn_ramstyle = "registers" *) reg [63:0] write_buffer [0:3];
reg [1:0] write_capture = 0;
always @(posedge logic_clk) begin
    if (sdram_write_data_valid) begin
        write_buffer[write_capture] <= sdram_write_data;
        write_capture <= write_capture + 1'b1;
    end
end

reg ack_event_108 = 0;
reg ack_toggle_108 = 0;
reg [3:0] read_phase_108 = 0;
wire controller_read_valid_108 = read_phase_108 >= 3 && read_phase_108 <= 10;
reg read_is_line_108 = 0;
reg read_word_published_108 = 0;
reg read_half_108 = 0;
reg [31:0] read_low_108 = 0;
reg [63:0] read_pair_108 = 0;
reg read_pair_event_108 = 0;
reg read_pair_toggle_108 = 0;
reg write_active_108 = 0;
reg [2:0] write_physical_beat_108 = 0;

wire [1:0] write_pair_108 = write_physical_beat_108[2:1];
wire [31:0] controller_write_data_32 = write_physical_beat_108[0] ?
    write_buffer[write_pair_108][63:32] : write_buffer[write_pair_108][31:0];

always @(posedge controller_clk) begin
    if (!sdram_pll_locked) begin
        controller_command_valid_108 <= 0;
        command_seen_108 <= 0;
        ack_event_108 <= 0;
        read_phase_108 <= 0;
        read_half_108 <= 0;
        read_word_published_108 <= 0;
        read_pair_event_108 <= 0;
        write_active_108 <= 0;
        write_physical_beat_108 <= 0;
    end else begin
        if (!sdram_command_valid)
            command_seen_108 <= 0;
        if (sdram_command_valid && !command_seen_108 && !controller_command_valid_108) begin
            controller_command_108 <= sdram_command;
            controller_precharge_108 <= sdram_precharge;
            controller_address_108 <= sdram_address;
            controller_write_mask_108 <= sdram_write_mask;
            controller_burst_length_108 <= sdram_burst_length;
            controller_command_valid_108 <= 1;
            command_seen_108 <= 1;
            read_is_line_108 <= sdram_burst_length != 0;
            read_word_published_108 <= 0;
        end
        if (controller_command_valid_108 && controller_command_ack_108) begin
            controller_command_valid_108 <= 0;
            ack_event_108 <= !ack_event_108;
            if (controller_command_108 == 3'b101)
                read_phase_108 <= 1;
            if (controller_command_108 == 3'b100 && controller_burst_length_108 != 0) begin
                write_active_108 <= 1;
                write_physical_beat_108 <= 1;
            end
        end
        if (read_phase_108 != 0 && read_phase_108 < 11)
            read_phase_108 <= read_phase_108 + 1'b1;
        else if (read_phase_108 == 11)
            read_phase_108 <= 0;

        if (controller_read_valid_108) begin
            if (!read_is_line_108 && !read_word_published_108) begin
                read_pair_108 <= {32'b0, controller_read_data_32};
                read_pair_event_108 <= !read_pair_event_108;
                read_word_published_108 <= 1;
            end else if (!read_half_108) begin
                read_low_108 <= controller_read_data_32;
                read_half_108 <= 1;
            end else begin
                read_pair_108 <= {controller_read_data_32, read_low_108};
                read_pair_event_108 <= !read_pair_event_108;
                read_half_108 <= 0;
            end
        end

        if (write_active_108) begin
            if (write_physical_beat_108 == 7) begin
                write_active_108 <= 0;
                write_physical_beat_108 <= 0;
            end else begin
                write_physical_beat_108 <= write_physical_beat_108 + 1'b1;
            end
        end
    end
end

// Publish controller events halfway between controller posedges. CPU-clock
// edges are derived from every second controller edge, so both token and data
// have half a 108-MHz cycle of setup time without an arbitrary-ratio FIFO.
always @(negedge controller_clk) begin
    ack_toggle_108 <= ack_event_108;
    read_pair_toggle_108 <= read_pair_event_108;
end

reg ack_seen_54 = 0;
reg read_pair_seen_54 = 0;
reg init_sync_54 = 0;
always @(posedge logic_clk) begin
    init_sync_54 <= controller_init_done_108;
    sdram_command_ack <= 0;
    sdram_read_valid <= 0;
    if (ack_seen_54 != ack_toggle_108) begin
        ack_seen_54 <= ack_toggle_108;
        sdram_command_ack <= 1;
    end
    if (read_pair_seen_54 != read_pair_toggle_108) begin
        read_pair_seen_54 <= read_pair_toggle_108;
        sdram_read_data <= read_pair_108;
        sdram_read_valid <= 1;
    end
end
assign sdram_init_done = init_sync_54;

TangNano20KSdramPll108M54M u_sdram_pll(
    .clkin(clk), .controller_clk(controller_clk), .logic_clk(logic_clk),
    .sdram_clk(sdram_clk_180), .locked(sdram_pll_locked)
);
always @(posedge controller_clk)
    sdram_reset_sync <= {sdram_reset_sync[0],sdram_pll_locked};

SDRAM_Controller_HS_QN88 u_sdram_controller(
 .O_sdram_clk(O_sdram_clk),.O_sdram_cke(O_sdram_cke),.O_sdram_cs_n(O_sdram_cs_n),
 .O_sdram_cas_n(O_sdram_cas_n),.O_sdram_ras_n(O_sdram_ras_n),.O_sdram_wen_n(O_sdram_wen_n),
 .O_sdram_dqm(O_sdram_dqm),.O_sdram_addr(O_sdram_addr),.O_sdram_ba(O_sdram_ba),.IO_sdram_dq(IO_sdram_dq),
 .I_sdrc_rst_n(sdram_reset_sync[1]),.I_sdrc_clk(controller_clk),.I_sdram_clk(sdram_clk_180),
 .I_sdrc_cmd_en(controller_command_valid_108),.I_sdrc_cmd(controller_command_108),
 .I_sdrc_precharge_ctrl(controller_precharge_108),.I_sdram_power_down(1'b0),.I_sdram_selfrefresh(1'b0),
 .I_sdrc_addr(controller_address_108),.I_sdrc_dqm(controller_write_mask_108),
 .I_sdrc_data(controller_write_data_32),.I_sdrc_data_len(controller_burst_length_108),
 .O_sdrc_data(controller_read_data_32),.O_sdrc_init_done(controller_init_done_108),
 .O_sdrc_cmd_ack(controller_command_ack_108));
