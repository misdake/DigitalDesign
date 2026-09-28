`timescale 1ns/1ps
module tb;
reg clk=0;always #5 clk=~clk;
reg reset=1,start_valid=0,source_last=0;
reg [383:0] positions;
reg [47:0] colors;
wire ready0,ready1,valid0,valid1,end0,end1,last0,last1,error0,error1;
wire [9:0] address0,address1;
reg [63:0] data0,data1;
wire [95:0] xy0,xy1;
wire [707:0] planes0,planes1;
wire [5:0] scale0,scale1;
reg [63:0] matrix [0:7];
integer cycles=0,completed=0,triangles=0,source_triangles=0,source=0;
reg stalled=0;
reg [811:0] held;
wire output_ready=(cycles%17<5);
GpuGeometryMicrocore #(.SHARED_MULTIPLIER(1)) shared(
    clk,reset,start_valid,ready0,positions,colors,10'd0,source_last,
    address0,data0,valid0,output_ready,end0,last0,error0,xy0,planes0,scale0);
GpuGeometryMicrocore #(.SHARED_MULTIPLIER(0)) separate(
    clk,reset,start_valid,ready1,positions,colors,10'd0,source_last,
    address1,data1,valid1,output_ready,end1,last1,error1,xy1,planes1,scale1);
always @(posedge clk) begin
    data0<=matrix[address0[2:0]];
    data1<=matrix[address1[2:0]];
    cycles<=cycles+1;
    if(reset) stalled<=0;
    else begin
        if(ready0!==ready1 || valid0!==valid1 || address0!==address1)
            $fatal(1,"control mismatch at cycle %0d",cycles);
        if(error0!==error1) $fatal(1,"error mismatch at cycle %0d",cycles);
        if(valid0 && {end0,last0,xy0,planes0,scale0} !==
            {end1,last1,xy1,planes1,scale1})
            $fatal(1,"shared DSP result mismatch at cycle %0d",cycles);
        if(stalled && (!valid0 || {end0,last0,xy0,planes0,scale0}!==held))
            $fatal(1,"unstable stalled output at cycle %0d",cycles);
        stalled<=valid0 && !output_ready;
        held<={end0,last0,xy0,planes0,scale0};
        if(valid0 && output_ready) begin
            if(error0 !== (source==5))
                $fatal(1,"unexpected geometry error at source %0d",source);
            if(end0) begin
                if(last0 !== source_last) $fatal(1,"source_last mismatch");
                completed<=completed+1;
            end else begin
                if(source==0 && source_triangles==0 && xy0!==
                    {16'd960,16'd3200,16'd2880,16'd4800,16'd2880,16'd1600})
                    $fatal(1,"incorrect viewport positions %h",xy0);
                triangles<=triangles+1;
                source_triangles<=source_triangles+1;
            end
        end
    end
end
task send;
    input [127:0] p0,p1,p2;
    input [15:0] c0,c1,c2;
    input last;
    input integer expected_triangles;
    integer previous;
    begin
        @(negedge clk);
        while(!ready0) @(negedge clk);
        positions={p2,p1,p0};colors={c2,c1,c0};source_last=last;
        start_valid=1;source_triangles=0;previous=completed;
        @(negedge clk);start_valid=0;
        while(completed==previous) @(negedge clk);
        if(source_triangles!=expected_triangles)
            $fatal(1,"source %0d triangles %0d expected %0d",source,
                source_triangles,expected_triangles);
        source=source+1;
    end
endtask
function [127:0] vertex;
    input [31:0] x,y,z;
    begin vertex={32'h00010000,z,y,x};end
endfunction
initial begin
    matrix[0]={32'd0,32'h00010000};matrix[1]=0;
    matrix[2]={32'h00010000,32'd0};matrix[3]=0;
    matrix[4]=0;matrix[5]={32'd0,32'h00010000};
    matrix[6]=0;matrix[7]={32'h00010000,32'd0};
    repeat(3) @(negedge clk);reset=0;
    // Entirely inside: one triangle and known snapped screen coordinates.
    send(vertex(32'hffff8000,32'hffff8000,32'h00008000),
         vertex(32'h00008000,32'hffff8000,32'h00008000),
         vertex(32'd0,32'h00008000,32'h00008000),
         16'hf800,16'h07e0,16'h001f,0,1);
    // One vertex behind near plane: a quad, triangulated into two outputs.
    send(vertex(32'hffff8000,32'hffff8000,32'hffff8000),
         vertex(32'h00008000,32'hffff8000,32'h00008000),
         vertex(32'd0,32'h00008000,32'h00008000),
         16'hf800,16'h07e0,16'h001f,0,2);
    // Far and guard clipping take different branch paths and arithmetic.
    send(vertex(32'hffff8000,32'hffff8000,32'h00018000),
         vertex(32'h00008000,32'hffff8000,32'h00008000),
         vertex(32'd0,32'h00008000,32'h00008000),
         16'hf800,16'h07e0,16'h001f,0,2);
    send(vertex(32'h00030000,32'hffff8000,32'h00008000),
         vertex(32'h00008000,32'hffff8000,32'h00008000),
         vertex(32'd0,32'h00008000,32'h00008000),
         16'hf800,16'h07e0,16'h001f,0,2);
    // Entirely behind near plane: no triangle, but still one source end.
    send(vertex(32'hffff8000,32'hffff8000,32'hffff8000),
         vertex(32'h00008000,32'hffff8000,32'hffff8000),
         vertex(32'd0,32'h00008000,32'hffff8000),
         16'hf800,16'h07e0,16'h001f,0,0);
    // MVP overflow must retire the source without entering clip/setup.
    @(negedge clk);matrix[0]={32'd0,32'h00020000};
    send(vertex(32'h60000000,32'hffff8000,32'h00008000),
         vertex(32'h00008000,32'hffff8000,32'h00008000),
         vertex(32'd0,32'h00008000,32'h00008000),
         16'hf800,16'h07e0,16'h001f,1,0);
    if(completed!=6 || triangles!=7) $fatal(1,"unexpected total");
    $display("DIGITAL_DESIGN_PASS geometry microcore %0d cycles",cycles);
    $finish;
end
initial begin #500000;$fatal(1,"geometry microcore watchdog");end
endmodule
