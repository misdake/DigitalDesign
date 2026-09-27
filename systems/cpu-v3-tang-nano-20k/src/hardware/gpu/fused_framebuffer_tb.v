`timescale 1ns/1ps
module fused_framebuffer_tb #(parameter SECTOR=0,parameter HALF_CAPACITY=1,parameter EXEC_DELAY=0);
    localparam WIDE=1, ATOMIC_COMMIT=1, LANE_PORTS=1;
    localparam integer SLOT_BITS=(WIDE ? 3 : 2)+(SECTOR ? 2 : 0)-HALF_CAPACITY;
    localparam integer ROWS=SECTOR ? 4 : 16,SLOTS=32'd1<<SLOT_BITS,ADDR_BITS=9+WIDE-HALF_CAPACITY;
    localparam integer GROUP_PIXELS=(32'd1<<ADDR_BITS)*4;
    reg clk=0;always #(1000.0/108.0) clk=~clk;
    reg reset=1,iv=0,resident=1,ig=0;wire ir;
    reg [SLOT_BITS-1:0] way=0;reg [3:0] x=0,y=0,mask=15;
    reg [127:0] colors=0;reg [63:0] depths=0;
    reg de=1,dw=1;reg [2:0] df=1;reg [1:0] blend=0;
    wire ev,cv;reg er=1,cr=1;wire [3:0] cm;wire [63:0] oc,oz;
    reg [63:0] result_c;reg [3:0] result_m;
    reg mwv=0,mwg=0,mrv=0,mrg=0;wire mwr,mrr,msv;reg msr=1;
    reg [ADDR_BITS-1:0] mwa=0,mra=0;reg [63:0] mwd=0;reg [7:0] mwm=255;wire [63:0] msd;
    wire [WIDE+8:0] extended_mwa=mwa,extended_mra=mra;
    FusedFramebufferPipe #(.SLOT_BITS(SLOT_BITS),.HALF_CAPACITY(HALF_CAPACITY)) dut (
        .clk(clk),.reset(reset),.input_valid(iv),.input_ready(ir),.resident(resident),
        .input_group(ig),.input_way(way),.input_x(x),.input_y(y),.input_colors(colors),.input_depths(depths),
        .input_mask(mask),.depth_enable(de),.depth_write(dw),.depth_func(df),.blend_mode(blend),
        .execute_valid(ev),.execute_ready(er),.old_colors(oc),.old_depths(oz),.result_colors(result_c),.result_mask(result_m),
        .commit_valid(cv),.commit_ready(cr),.commit_mask(cm),
        .memory_write_valid(mwv),.memory_write_ready(mwr),.memory_write_group(mwg),.memory_write_address(extended_mwa),.memory_write_data(mwd),.memory_write_mask(mwm),
        .memory_read_valid(mrv),.memory_read_ready(mrr),.memory_read_group(mrg),.memory_read_address(extended_mra),
        .memory_response_valid(msv),.memory_response_ready(msr),.memory_response_data(msd));
    function depth_pass;
        input [15:0] n,o;input [2:0] f;
        begin case(f)
            0:depth_pass=0;1:depth_pass=n<o;2:depth_pass=n==o;3:depth_pass=n<=o;
            4:depth_pass=n>o;5:depth_pass=n!=o;6:depth_pass=n>=o;7:depth_pass=1;
        endcase end
    endfunction
    integer k;
    always @* begin
        result_m=0;result_c=0;
        for(integer lane=0;lane<4;lane=lane+1) begin
            result_m[lane]=!de || depth_pass(depths[16*lane +:16],oz[16*lane +:16],df);
            result_c[16*lane +:16]={colors[32*lane+27 +:5],colors[32*lane+18 +:6],colors[32*lane+11 +:5]};
            // Synthetic old-color dependency checks the callback boundary.
            // This XOR is deliberately NOT production blend arithmetic.
            if(blend!=0) result_c[16*lane +:16]=result_c[16*lane +:16]^oc[16*lane +:16];
        end
    end
    reg [15:0] pixels [0:2*GROUP_PIXELS-1];
    function integer pixel;
        input integer g,w,p,px,py;
        begin pixel=g*GROUP_PIXELS+w*(2*ROWS*16)+p*(ROWS*16)+py*16+px;end
    endfunction
    function integer mpixel;
        input integer g,a,lane;
        begin mpixel=pixel(g,a/(ROWS*8),(a/(ROWS*4))%2,(a%4)*4+lane,(a/4)%ROWS);end
    endfunction
    function [15:0] seed;
        input integer p;
        begin seed=(p*391)^(p/7)^16'ha537;end
    endfunction
    reg [63:0] expected_memory=0,expected_colors=0,expected_depths=0,expected_result=0;
    reg expected_memory_valid=0,expected_exec=0;reg [3:0] expected_mask=0;
    integer cycles=0,commits=0,covered=0,passed=0,reads=0,writes=0;
    integer background_reads=0,background_writes=0,miss_wait=0;
    integer raw_wait=0;
    reg held_memory=0,held_commit=0;reg [63:0] held_memory_data;reg [3:0] held_commit_mask;
    integer lane,p,b;
    always @(posedge clk) begin
        cycles=cycles+1;
        if(cycles>200000) $fatal(1,"fused watchdog");
        if(reset) begin expected_memory_valid=0;expected_exec=0;held_memory=0;held_commit=0;end
        else begin
            if(held_memory && (!msv || msd!==held_memory_data)) $fatal(1,"memory output changed under stall");
            if(!ATOMIC_COMMIT && held_commit && (!cv || cm!==held_commit_mask)) $fatal(1,"commit changed under stall");
            if(ATOMIC_COMMIT && cv && !cr) $fatal(1,"atomic event without completion permission");
            held_memory=msv && !msr;held_memory_data=msd;
            held_commit=cv && !cr;held_commit_mask=cm;
            if(msv) begin
                if(!expected_memory_valid || msd!==expected_memory) $fatal(1,"memory mismatch cycle=%0d got=%h expected=%h",cycles,msd,expected_memory);
                if(msr) expected_memory_valid=0;
            end
            if(mrv && mrr) begin
                if(expected_memory_valid) $fatal(1,"memory output overwritten");
                for(lane=0;lane<4;lane=lane+1) expected_memory[16*lane +:16]=pixels[mpixel(mrg,mra,lane)];
                expected_memory_valid=1;background_reads=background_reads+1;
            end
            if(mwv && mwr) begin
                for(b=0;b<8;b=b+1) if(mwm[b]) begin
                    p=mpixel(mwg,mwa,b/2);pixels[p][8*(b%2) +:8]=mwd[8*b +:8];
                end
                background_writes=background_writes+1;
            end
            if(iv && !resident) begin
                miss_wait=miss_wait+1;
                if(dut.rrv || dut.rwv || ir) $fatal(1,"miss accessed render ports");
            end
            if(ev) begin
                if(!expected_exec || oc!==expected_colors || oz!==expected_depths)
                    $fatal(1,"old quad mismatch cycle=%0d c=%h/%h z=%h/%h",cycles,oc,expected_colors,oz,expected_depths);
                if(result_c!==expected_result || (result_m & mask)!==expected_mask) $fatal(1,"execution golden mismatch");
            end
            // Audit each actual render write, not merely returned mask.
            if(dut.rwv && dut.rwr) begin
                for(lane=0;lane<(WIDE ? 4 : 2);lane=lane+1) begin
                    p=pixel(ig,way,0,x+(lane%2),dut.wy+lane/2);
                    if(dut.cm[lane]) pixels[p]=dut.wc[16*lane +:16];
                    if(dut.zm[lane]) pixels[p+ROWS*16]=dut.wz[16*lane +:16];
                end
                writes=writes+1;
            end
            if(dut.rrv && dut.rrr) reads=reads+1;
            if(ev && er && mrv && mrr && !dut.rwr) raw_wait=raw_wait+1;
            if(cv && cr) begin
                if(!iv || !ir || !expected_exec || cm!==expected_mask) $fatal(1,"commit/ownership mismatch");
                // Independent final per-quad expected C/Z (different from port audit).
                for(lane=0;lane<4;lane=lane+1) begin
                    p=pixel(ig,way,0,x+lane%2,y+lane/2);
                    if(pixels[p] !== (expected_mask[lane] ? expected_result[16*lane +:16] : expected_colors[16*lane +:16])) $fatal(1,"final color mismatch");
                    if(pixels[p+ROWS*16] !== (expected_mask[lane] && de && dw ? depths[16*lane +:16] : expected_depths[16*lane +:16])) $fatal(1,"final depth mismatch");
                    covered=covered+mask[lane];passed=passed+expected_mask[lane];
                end
                commits=commits+1;expected_exec=0;
            end
        end
    end
    task prepare;
        input integer id,g,w,px,py,m,f,enable,write_enable,mode;
        integer l,q;reg [31:0] rgba;reg [15:0] r,gc,bc,newz,oldz;
        begin
            iv=1;resident=1;ig=g;way=w;x=px;y=py;mask=m;df=f;de=enable;dw=write_enable;blend=mode;
            expected_mask=0;expected_result=0;
            for(l=0;l<4;l=l+1) begin
                q=pixel(g,w,0,px+l%2,py+l/2);
                expected_colors[16*l +:16]=pixels[q];oldz=pixels[q+ROWS*16];expected_depths[16*l +:16]=oldz;
                rgba=((id*1709+l*331)^32'h7fc17329);colors[32*l +:32]=rgba;
                case((id+l)%3) 0:newz=oldz;1:newz=oldz==0 ? 0 : oldz-1;2:newz=oldz==65535 ? 65535 : oldz+1;endcase
                depths[16*l +:16]=newz;
                case(f)
                    0:expected_mask[l]=0;1:expected_mask[l]=newz<oldz;2:expected_mask[l]=newz==oldz;3:expected_mask[l]=newz<=oldz;
                    4:expected_mask[l]=newz>oldz;5:expected_mask[l]=newz!=oldz;6:expected_mask[l]=newz>=oldz;7:expected_mask[l]=1;
                endcase
                expected_mask[l]=m[l] && (!enable || expected_mask[l]);
                r=(rgba/16777216)%256;gc=(rgba/65536)%256;bc=(rgba/256)%256;
                expected_result[16*l +:16]=(r/8)*2048+(gc/4)*32+(bc/8);
                if(mode!=0) expected_result[16*l +:16]=expected_result[16*l +:16]^pixels[q];
            end
            expected_exec=1;
        end
    endtask
    integer a,g,i,t,start,start_commits,start_covered,start_reads,start_writes,start_bg_r,start_bg_w;
    integer bg_group,bg_address,exec_age;reg random_stalls;
    task run_item;
        input integer id,group_id,way_id,px,py,m,f,en,we,mode,stress;
        begin
            prepare(id,group_id,way_id,px,py,m,f,en,we,mode);
            if(stress==1 && id%11==0) resident=0;
            t=0;exec_age=0;
            begin: wait_done
                forever begin
                    if(ev) exec_age=exec_age+1;else exec_age=0;
                    er=(!stress || t%7!=0) && (!ev || exec_age>EXEC_DELAY);cr=!stress || t%9!=0;msr=!stress || t%5!=0;
                    // Never overwrite this render quad. Background uses the final way.
                    mwv=stress && t%2==0;mrv=stress && t%2==1;
                    mwg=(id%3==0) ? group_id : !group_id;mrg=mwg;
                    mwa=(SLOTS-1)*(ROWS*8)+((id+t)%(ROWS*8));mra=(SLOTS-1)*(ROWS*8)+((id+t+1)%(ROWS*8));
                    mwd={32'h38e12a47+id+t,32'hf712397a-id-t};mwm=(id+t)%256;
                    if(stress==3) begin
                        er=!ev || exec_age>EXEC_DELAY;cr=1;msr=1;
                        mrv=1;mwv=t%2==0;mrg=group_id;mwg=group_id;
                        mra=(SLOTS-1)*(ROWS*8)+((id+t)%(ROWS*4));
                        mwa=(SLOTS-1)*(ROWS*8)+ROWS*4+((id+t+1)%(ROWS*4));mwm=255;
                    end
                    if(stress==4) begin
                        er=!ev || exec_age>EXEC_DELAY;cr=1;msr=1;mwv=0;mrv=t==1;mrg=group_id;
                        mra=way_id*(ROWS*8)+py*4+px/4;
                    end
                    if(!resident && t>=13) resident=1;
                    @(posedge clk);
                    if(ir) disable wait_done;
                    @(negedge clk);t=t+1;if(t>500) $fatal(1,"item stuck id=%0d state=%0d",id,dut.state);
                end
            end
            @(negedge clk);iv=0;mwv=0;mrv=0;er=1;cr=1;msr=1;
        end
    endtask
    initial begin
        for(a=0;a<2*GROUP_PIXELS;a=a+1) pixels[a]=16'hdead;
        repeat(3) @(negedge clk);reset=0;
        for(g=0;g<2;g=g+1) for(a=0;a<(1<<ADDR_BITS);a=a+1) begin
            @(negedge clk);mwv=1;mwg=g;mwa=a;mwm=255;
            mwd={seed(mpixel(g,a,3)),seed(mpixel(g,a,2)),seed(mpixel(g,a,1)),seed(mpixel(g,a,0))};
        end
        @(negedge clk);mwv=0;
        if(LANE_PORTS) begin
            // A port cannot read and write the same plane in one cycle.
            mwv=1;mrv=1;mwg=0;mrg=0;mwa=0;mra=1;mwm=255;mwd=64'h8139_42ad_f351_2817;
            @(posedge clk);if(mrr || !mwr) $fatal(1,"same-plane memory port overcommitted");
            @(negedge clk);mwv=0;
            @(posedge clk);if(!mrr) $fatal(1,"memory read did not resume");
            @(negedge clk);mrv=0;
        end
        // A zero-byte write is a no-op: it must not steal the same-plane read address.
        @(negedge clk);mwv=1;mrv=1;mwg=0;mrg=0;mwa=0;mra=2;mwm=0;
        @(posedge clk);if(!mrr || !mwr) $fatal(1,"zero-mask memory overlap blocked");
        @(negedge clk);mwv=0;mrv=0;mwm=255;
        // Every mask/function/state combination, including no-depth/no-write and same-quad RAW.
        for(i=0;i<1024;i=i+1) run_item(i,i%2,(i/128)%(SLOTS-1),2*((i/16)%8),2*((i/64)%(ROWS/2)),i%16,(i/16)%8,(i/128)%2,(i/256)%2,(i/512)%2,1);
        for(i=0;i<64;i=i+1) run_item(1200+i,0,0,0,0,15,1,1,1,i%2,1);
        if(LANE_PORTS) run_item(1500,0,0,0,0,15,7,1,1,2,4);
        // Render every physical quad, including all high way/address bits.
        for(g=0;g<2;g=g+1) for(a=0;a<SLOTS-1;a=a+1)
            for(integer qy=0;qy<ROWS;qy=qy+2) for(integer qx=0;qx<16;qx=qx+2)
                run_item(1600+a+qy+qx,g,a,qx,qy,15,7,0,0,2,0);
        start=cycles;start_commits=commits;start_covered=covered;start_reads=reads;start_writes=writes;
        for(i=0;i<256;i=i+1) run_item(2000+i,i%2,0,2*(i%8),2*((i/8)%(ROWS/2)),15,7,1,1,1,0);
        if (cycles-start != 256*(2+EXEC_DELAY)) $fatal(1,"dense quad interval regression");
        $display("RATE wide=%0d sector=%0d atomic=%0d quads=%0d cycles=%0d pixels=%0d reads=%0d writes=%0d",WIDE,SECTOR,ATOMIC_COMMIT,commits-start_commits,cycles-start,covered-start_covered,reads-start_reads,writes-start_writes);
        start=cycles;start_commits=commits;start_covered=covered;start_reads=reads;start_writes=writes;
        for(i=0;i<256;i=i+1) run_item(3000+i,i%2,0,2*(i%8),2*((i/8)%(ROWS/2)),15,7,1,1,2,2);
        $display("RATE_BG wide=%0d sector=%0d atomic=%0d quads=%0d cycles=%0d pixels=%0d reads=%0d writes=%0d",WIDE,SECTOR,ATOMIC_COMMIT,commits-start_commits,cycles-start,covered-start_covered,reads-start_reads,writes-start_writes);
        if(LANE_PORTS) begin
            start=cycles;start_commits=commits;start_covered=covered;start_bg_r=background_reads;start_bg_w=background_writes;
            for(i=0;i<256;i=i+1) run_item(4000+i,i%2,0,2*(i%8),2*((i/8)%(ROWS/2)),15,7,1,1,2,3);
            if(EXEC_DELAY==0 && ATOMIC_COMMIT && (background_reads-start_bg_r!=512 || background_writes-start_bg_w!=256 || cycles-start!=512))
                $fatal(1,"independent memory/render bandwidth not reached");
            $display("RATE_MEM_STREAM quads=%0d cycles=%0d pixels=%0d memory64_reads=%0d memory64_writes=%0d",commits-start_commits,cycles-start,covered-start_covered,background_reads-start_bg_r,background_writes-start_bg_w);
        end
        // Reset while stalled before any writes; stale response must not commit.
        @(negedge clk);prepare(9000,0,0,0,0,15,7,1,1,0);er=0;
        repeat(7) @(negedge clk);reset=1;iv=0;repeat(2) @(negedge clk);reset=0;er=1;
        run_item(9001,0,0,0,0,15,7,1,1,0,0);
        for(g=0;g<2;g=g+1) for(a=0;a<(1<<ADDR_BITS);a=a+1) begin
            @(negedge clk);mrv=1;mrg=g;mra=a;
            @(posedge clk);if(!mrr) $fatal(1,"final memory audit bubble");
        end
        @(negedge clk);mrv=0;repeat(3) @(negedge clk);
        if(background_reads<100 || background_writes<100 || miss_wait<100 || commits!=1601+2*(SLOTS-1)*(ROWS/2)*8+(LANE_PORTS ? 257 : 0)) $fatal(1,"coverage missing commits=%0d",commits);
        if(LANE_PORTS && EXEC_DELAY==0 && raw_wait==0) $fatal(1,"same-word RAW audit did not execute");
        $display("PASS fused framebuffer: wide=%0d sector=%0d commits=%0d covered=%0d passed=%0d cycles=%0d bg_r=%0d bg_w=%0d miss=%0d",WIDE,SECTOR,commits,covered,passed,cycles,background_reads,background_writes,miss_wait);
        $finish;
    end
endmodule
