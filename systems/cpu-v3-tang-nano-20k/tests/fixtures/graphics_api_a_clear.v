    integer cycle=0,beat=0,beats=0,read_base=0;
    integer wbeat=0,wbeats=0,write_base=0,writes=0,i;
    integer argument_count=0,argument_base=0;
    reg reading=0,writing=0,reply=0;
    reg [63:0] memory[0:1048575];
    always @(negedge clk) begin
        gpu_ro_request_ready=!reading;
        gpu_ro_response_valid=reading;
        gpu_ro_response_last=reading && beat+1==beats;
        gpu_ro_read_data=memory[read_base+beat];
        gpu_fb_w_request_ready=!writing&&!reply;
        gpu_fb_w_write_data_ready=writing;
        gpu_fb_w_response_valid=reply;
        gpu_fb_w_response_last=reply;
    end
    always @(posedge clk) if(!reset)begin
        cycle=cycle+1;
        if(cycle>160000)$fatal(1,"public Clear watchdog status=%h",device_read_data);
        if(gpu_fb_r_request_valid)$fatal(1,"Clear/Load without draws read framebuffer");
        if(gpu_ro_request_valid&&gpu_ro_request_ready)begin
            reading=1;read_base=gpu_ro_address>>2;beat=0;
            beats=(gpu_ro_line_count_minus_1+1)*4;
        end else if(gpu_ro_response_valid)begin
            if(gpu_ro_response_last)reading=0;else beat=beat+1;
        end
        reply=0;
        if(gpu_fb_w_request_valid&&gpu_fb_w_request_ready)begin
            writing=1;write_base=gpu_fb_w_address>>2;wbeat=0;
            wbeats=(gpu_fb_w_line_count_minus_1+1)*4;
            if(write_base<524288 || write_base+wbeats>549888)
                $fatal(1,"write outside physical color plane %h",gpu_fb_w_address);
        end else if(gpu_fb_w_write_data_ready)begin
            memory[write_base+wbeat]=gpu_fb_w_write_data;
            writes=writes+1;
            if(wbeat+1==wbeats)begin writing=0;reply=1;end
            else wbeat=wbeat+1;
        end
    end
    task tick;begin @(posedge clk);#1;end endtask
    task send;input [3:0] channel;input [15:0] value;begin
        @(negedge clk);device_channel=channel;device_write_data=value;device_write_enable=1;
        tick();device_write_enable=0;device_channel=2;
    end endtask
    initial begin
        for(i=0;i<1048576;i=i+1)memory[i]=64'ha55aa55aa55aa55a;
        $readmemh("input.hex",memory);
        if(!$value$plusargs("COUNT=%d",argument_count)||!$value$plusargs("BASE=%d",argument_base))
            $fatal(1,"missing actual public submission");
        repeat(4)tick();reset=0;repeat(4)tick();
        send(0,argument_count);send(1,argument_base);#1;
        if(device_read_data!==1)$fatal(1,"START did not acquire GPU");
        while(device_read_data[0])tick();
        if(device_read_data!==0||writes!=25600)$fatal(1,"Clear completion/write count %h %d",device_read_data,writes);
        for(i=524288;i<549888;i=i+1)if(memory[i]!==64'h1357135713571357)
            $fatal(1,"color mismatch %d %h",i,memory[i]);
        for(i=549888;i<557056;i=i+1)if(memory[i]!==64'ha55aa55aa55aa55a)
            $fatal(1,"color allocation tail modified");
        for(i=589824;i<622592;i=i+1)if(memory[i]!==64'ha55aa55aa55aa55a)
            $fatal(1,"Load depth/allocation tail modified");
        $writememh("color.hex",memory,524288,549887);
        $display("PUBLIC_GPU_CLEAR_PASS cycles=%0d qwords=%0d",cycle,writes);
        $finish;
    end
endmodule
