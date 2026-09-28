`timescale 1ns/1ps
module tb;
reg clk=0;always #5 clk=~clk;
reg reset=1,load_valid=0,load_slot=0,commit_valid=0,commit_slot=0;
reg [8:0] load_address=0;
reg [63:0] load_data=0;
reg [6:0] commit_vertex_count=0;
reg [7:0] commit_triangle_count=0;
wire load_ready,commit_ready,release_valid,release_slot;
wire output_valid,output_error,sticky_error;
wire [30:0] output_work;
wire [3:0] output_mask;
wire [127:0] output_color;
integer cycles=0,quads=0,markers=0,releases=0;
reg seen_clipped_fan=0;
reg stalled=0;
reg [163:0] held;
wire output_ready=(cycles%11<5);
GpuMeshletQuad dut(clk,reset,load_valid,load_ready,load_slot,
    load_address,load_data,commit_valid,commit_ready,commit_slot,
    commit_vertex_count,commit_triangle_count,release_valid,release_slot,
    output_valid,output_ready,output_work,output_mask,output_color,
    output_error,sticky_error);
always @(posedge clk) begin
    cycles<=cycles+1;
    if(reset) stalled<=0;
    else begin
        if(stalled && (!output_valid ||
            {output_work,output_mask,output_color,output_error}!==held))
            $fatal(1,"quad queue output changed under backpressure");
        stalled<=output_valid&&!output_ready;
        held<={output_work,output_mask,output_color,output_error};
        if(release_valid) releases<=releases+1;
        if(output_valid&&output_ready) begin
            if(output_error || sticky_error) $fatal(1,"unexpected color/raster error");
            if(output_work[28:27]==0) begin
                if(output_work[30]!==markers[0] || output_work[23:17]!=0)
                    $fatal(1,"quad order or source id");
                if(output_mask==0) $fatal(1,"empty quad");
                if((output_mask[0] && output_color[31:24]!=8'hff) ||
                   (output_mask[1] && output_color[63:56]!=8'hff))
                    $fatal(1,"invalid color alpha");
                if(output_work[30] && output_work[26:24]==3'd1)
                    seen_clipped_fan<=1;
                quads<=quads+1;
            end else if(output_work[28:27]==2) begin
                if(output_work[30]!==markers[0] || !output_work[29] ||
                   output_work[23:17]!=0)
                    $fatal(1,"source-end order or fields");
                markers<=markers+1;
            end
        end
    end
end
task write_word;
input slot;input [8:0] address;input [63:0] value;
begin
    @(negedge clk);
    load_slot=slot;load_address=address;load_data=value;load_valid=1;
    #1;if(!load_ready) $fatal(1,"load blocked");
    @(negedge clk);load_valid=0;
end
endtask
task fill;
input slot;input [15:0] rgb;
begin
    write_word(slot,0,{32'd0,32'h00010000});write_word(slot,1,0);
    write_word(slot,2,{32'h00010000,32'd0});write_word(slot,3,0);
    write_word(slot,4,0);write_word(slot,5,{32'd0,32'h00010000});
    write_word(slot,6,0);write_word(slot,7,{32'h00010000,32'd0});
    write_word(slot,8,{32'hffffe666,32'hffffe666});
    write_word(slot,9,{32'h00010000,(slot ? 32'hffff8000 : 32'h00008000)});
    write_word(slot,10,{48'd0,rgb});
    write_word(slot,11,{32'hffffe666,32'h0000199a});
    write_word(slot,12,{32'h00010000,32'h00008000});
    write_word(slot,13,{48'd0,rgb});
    write_word(slot,14,{32'h0000199a,32'd0});
    write_word(slot,15,{32'h00010000,32'h00008000});
    write_word(slot,16,{48'd0,rgb});
    write_word(slot,200,64'h0000000000010200);
end
endtask
task publish;
input slot;
begin
    @(negedge clk);commit_slot=slot;commit_vertex_count=3;
    commit_triangle_count=1;commit_valid=1;
    #1;while(!commit_ready) @(negedge clk);
    @(negedge clk);commit_valid=0;
end
endtask
initial begin
    repeat(3) @(negedge clk);reset=0;
    fill(0,16'hf800);publish(0);
    fill(1,16'h07e0);publish(1);
    while(markers<2) @(negedge clk);
    if(quads<2 || releases!=2 || !seen_clipped_fan)
        $fatal(1,"missing geometry/retirement: quads=%0d releases=%0d",quads,releases);
    $display("DIGITAL_DESIGN_PASS meshlet to quad %0d quads",quads);
    $finish;
end
initial begin #1000000;$fatal(1,"meshlet-to-quad watchdog");end
endmodule
