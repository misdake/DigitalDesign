`timescale 1ns/1ps
// Real Device4 command fetch, held request, completion and immediate resubmission.
module tb;
    reg clk=0,reset=1;
    always #5 clk=~clk;
    GSR GSR(.GSRI(1'b1));
    reg [2:0] device_index=4;
    reg [3:0] device_channel=2;
    reg device_read_enable=1,device_write_enable=0;
    reg [15:0] device_write_data=0;
    wire [15:0] device_read_data;
    reg gpu_ro_request_ready=0,gpu_ro_write_data_ready=0;
    reg gpu_ro_response_valid=0,gpu_ro_response_last=0,gpu_ro_error=0;
    reg [63:0] gpu_ro_read_data=0;
    reg gpu_fb_r_request_ready=0,gpu_fb_r_write_data_ready=0;
    reg gpu_fb_r_response_valid=0,gpu_fb_r_response_last=0,gpu_fb_r_error=0;
    reg [63:0] gpu_fb_r_read_data=0;
    reg gpu_fb_w_request_ready=0,gpu_fb_w_write_data_ready=0;
    reg gpu_fb_w_response_valid=0,gpu_fb_w_response_last=0,gpu_fb_w_error=0;
    wire gpu_ro_request_valid,gpu_ro_write;
    wire [21:0] gpu_ro_address;
    wire [1:0] gpu_ro_line_count_minus_1;
    wire [63:0] gpu_ro_write_data;
    wire gpu_fb_r_request_valid,gpu_fb_r_write;
    wire [21:0] gpu_fb_r_address;
    wire [1:0] gpu_fb_r_line_count_minus_1;
    wire [63:0] gpu_fb_r_write_data;
    wire gpu_fb_w_request_valid,gpu_fb_w_write;
    wire [21:0] gpu_fb_w_address;
    wire [1:0] gpu_fb_w_line_count_minus_1;
    wire [63:0] gpu_fb_w_write_data;
    CpuV3Gpu dut(.*);
    integer cycle=0,requests=0,returns=0;

    always @(posedge clk) begin
        cycle=cycle+1;
        if(cycle>2000)$fatal(1,"Device4 fixture watchdog");
        if(!reset)begin
            if(gpu_ro_write!==0||gpu_fb_r_write!==0||gpu_fb_w_write!==1)
                $fatal(1,"Shared-memory direction binding");
            if(gpu_fb_r_request_valid||gpu_fb_w_request_valid)
                $fatal(1,"Empty render unexpectedly accessed framebuffer");
            if(gpu_ro_request_valid&&gpu_ro_request_ready)requests=requests+1;
            if(gpu_ro_response_valid)returns=returns+1;
        end
    end

    task tick;begin @(posedge clk);#1;end endtask
    task status;input [15:0] expected;begin
        #1;if(device_read_data!==expected)
            $fatal(1,"Device4 status got %h expected %h",device_read_data,expected);
    end endtask
    task send;input [3:0] channel;input [15:0] value;begin
        @(negedge clk);
        device_channel=channel;device_write_data=value;device_write_enable=1;
        tick();device_write_enable=0;device_channel=2;
    end endtask
    task execute_buffer;input [15:0] base_sector;input [21:0] word_address;
        integer beat,hold;
        begin
            status(0);send(0,3);status(0); // Four u32 commands, not four bytes.
            send(1,base_sector);status(1);
            while(gpu_ro_request_valid!==1)tick();
            for(hold=0;hold<4;hold=hold+1)begin
                if(gpu_ro_address!==word_address||gpu_ro_line_count_minus_1!==3)
                    $fatal(1,"CB byte/word address or 128B length");
                tick();status(1);
                if(gpu_ro_request_valid!==1)$fatal(1,"Request dropped under backpressure");
            end
            @(negedge clk);gpu_ro_request_ready=1;tick();
            @(negedge clk);gpu_ro_request_ready=0;
            for(beat=0;beat<16;beat=beat+1)begin
                status(1);
                @(negedge clk);
                // NOP; WAIT(data+render); FLUSH; END. Remaining allocation is padding.
                case(beat)
                    0:gpu_ro_read_data=64'h4000000300000000;
                    1:gpu_ro_read_data=64'hf000000070000000;
                    default:gpu_ro_read_data=0;
                endcase
                gpu_ro_response_valid=1;gpu_ro_response_last=beat==15;tick();
                @(negedge clk);gpu_ro_response_valid=0;gpu_ro_response_last=0;
                tick();
                if(beat<15)status(1); // END data alone cannot finish an unretired read.
            end
            while(device_read_data[0]!==0)tick();
            status(0);
        end
    endtask

    initial begin
        tick();tick();tick();reset=0;tick();status(0);
        device_index=3;status(0);device_index=4;
        device_channel=1;status(0);device_channel=2;
        execute_buffer(16'h0800,22'h020000);
        execute_buffer(16'h0a00,22'h028000); // No result receipt or ACK between buffers.
        if(requests!=2||returns!=32)$fatal(1,"Command transaction conservation");
        repeat(4)begin tick();status(0);end
        $display("DIGITAL_DESIGN_PASS CpuV3Gpu Device4 requests=%0d returns=%0d cycles=%0d",requests,returns,cycle);
        $finish;
    end
endmodule
