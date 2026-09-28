`timescale 1ns/1ps
module tb;
reg clk=0;always #5 clk=~clk;
reg reset=1,load_valid=0,load_slot=0,commit_valid=0,commit_slot=0;
reg [8:0] load_address=0;
reg [63:0] load_data=0;
reg [6:0] commit_vertex_count=0;
reg [7:0] commit_triangle_count=0;
wire load_ready,commit_ready,output_valid,output_end,output_last,output_error;
wire output_slot,release_valid,release_slot;
wire [7:0] output_source;
wire [95:0] output_xy;
wire [707:0] output_planes;
wire [5:0] output_scale;
integer cycles=0,release_count=0,config_count=0,end_count=0,bad_count=0;
reg stalled=0,prior_final=0,prior_slot=0;
reg [821:0] held;
wire output_ready=(cycles%23<7);
GpuMeshletMicrocore dut(clk,reset,load_valid,load_ready,load_slot,
    load_address,load_data,commit_valid,commit_ready,commit_slot,
    commit_vertex_count,commit_triangle_count,output_valid,output_ready,
    output_end,output_last,output_error,output_slot,output_source,
    output_xy,output_planes,output_scale,release_valid,release_slot);
always @(posedge clk) begin
    cycles<=cycles+1;
    if(reset) begin stalled<=0;prior_final<=0;end
    else begin
        if(stalled && (!output_valid ||
            {output_end,output_last,output_error,output_slot,output_source,
             output_xy,output_planes,output_scale}!==held))
            $fatal(1,"output changed under backpressure");
        stalled<=output_valid&&!output_ready;
        held<={output_end,output_last,output_error,output_slot,output_source,
              output_xy,output_planes,output_scale};
        prior_final<=output_valid&&output_ready&&output_end&&output_last;
        if(output_valid&&output_ready&&output_end&&output_last)
            prior_slot<=output_slot;
        if(release_valid) begin
            if(!prior_final || release_slot!==prior_slot)
                $fatal(1,"slot released before final marker acceptance");
            if(release_count==0 && release_slot!==0 ||
               release_count==1 && release_slot!==1 ||
               release_count==2 && release_slot!==0)
                $fatal(1,"release order");
            release_count<=release_count+1;
        end
        if(output_valid&&output_ready) begin
            if(output_end) begin
                end_count<=end_count+1;
                if(output_error) begin
                    if(output_slot!==1 || output_source!==1 || !output_last)
                        $fatal(1,"invalid index did not retire correctly");
                    bad_count<=bad_count+1;
                end else if(output_slot==1 && output_source==0 && output_last)
                    $fatal(1,"early source_last on slot 1");
            end else begin
                if(output_error) $fatal(1,"unexpected triangle error");
                if(config_count==0 && (output_slot!==0 || output_source!==0 ||
                    output_xy!=={16'd960,16'd3200,16'd2880,16'd4800,
                                 16'd2880,16'd1600}))
                    $fatal(1,"first triangle config");
                if(output_slot==1 && output_source!=0) $fatal(1,"wrong fan source");
                config_count<=config_count+1;
            end
        end
    end
end
task write_word;
input slot;input [8:0] address;input [63:0] value;
begin
    @(negedge clk);
    load_slot=slot;load_address=address;load_data=value;load_valid=1;
    #1;
    if(!load_ready) $fatal(1,"load blocked for free slot");
    @(negedge clk);load_valid=0;
end
endtask
task write_matrix;
input slot;
begin
    write_word(slot,0,{32'd0,32'h00010000});write_word(slot,1,0);
    write_word(slot,2,{32'h00010000,32'd0});write_word(slot,3,0);
    write_word(slot,4,0);write_word(slot,5,{32'd0,32'h00010000});
    write_word(slot,6,0);write_word(slot,7,{32'h00010000,32'd0});
end
endtask
task write_vertex;
input slot;input [5:0] index;input [127:0] value;input [15:0] color;
reg [8:0] address;
begin
    address=9'd8+{3'd0,index}*9'd3;
    write_word(slot,address,value[63:0]);
    write_word(slot,address+9'd1,value[127:64]);
    write_word(slot,address+9'd2,{48'd0,color});
end
endtask
task publish;
input slot;input [7:0] count;
begin
    @(negedge clk);
    commit_slot=slot;commit_vertex_count=3;commit_triangle_count=count;
    commit_valid=1;
    #1;
    while(!commit_ready) @(negedge clk);
    @(negedge clk);commit_valid=0;
end
endtask
function [127:0] vertex;
input [31:0] x,y,z;
begin vertex={32'h00010000,z,y,x};end
endfunction
task fill_inside;
input slot;
begin
    write_matrix(slot);
    write_vertex(slot,0,vertex(32'hffff8000,32'hffff8000,32'h00008000),16'hf800);
    write_vertex(slot,1,vertex(32'h00008000,32'hffff8000,32'h00008000),16'h07e0);
    write_vertex(slot,2,vertex(32'd0,32'h00008000,32'h00008000),16'h001f);
    write_word(slot,200,64'h0000000000020100);
end
endtask
initial begin
    repeat(3) @(negedge clk);reset=0;
    fill_inside(0);publish(0,1);
    @(negedge clk);
    load_slot=0;load_address=0;load_data=64'hdeadbeef;load_valid=1;
    #1;
    if(load_ready) $fatal(1,"committed slot writable");
    @(negedge clk);load_valid=0;
    write_matrix(1);
    write_vertex(1,0,vertex(32'hffff8000,32'hffff8000,32'hffff8000),16'hf800);
    write_vertex(1,1,vertex(32'h00008000,32'hffff8000,32'h00008000),16'h07e0);
    write_vertex(1,2,vertex(32'd0,32'h00008000,32'h00008000),16'h001f);
    write_word(1,200,64'h0000000000020100);
    write_word(1,201,64'h00000000003f0100);
    publish(1,2);
    while(release_count<1) @(negedge clk);
    fill_inside(0);publish(0,1);
    while(release_count<3) @(negedge clk);
    if(config_count!=4 || end_count!=4 || bad_count!=1)
        $fatal(1,"wrong records: config=%0d end=%0d bad=%0d",
            config_count,end_count,bad_count);
    $display("DIGITAL_DESIGN_PASS meshlet pingpong %0d cycles",cycles);
    $finish;
end
initial begin #1000000;$fatal(1,"meshlet scheduler watchdog");end
endmodule
