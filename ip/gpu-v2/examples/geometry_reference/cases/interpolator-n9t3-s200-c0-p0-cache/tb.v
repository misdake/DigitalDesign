`timescale 1ns/1ps
module tb;
reg clk=0;always #5 clk=~clk;GSR GSR(.GSRI(1'b1));
reg [0:0] ce=0;
reg [0:0] reset=0;
reg [0:0] packet_valid=0;
reg [0:0] packet_last=0;
reg [35:0] packet_data=0;
reg [0:0] result_ready=0;
reg [0:0] vertex_mode=0;
reg [0:0] matrix_write=0;
reg [0:0] vertex_output_ready=0;
reg [4:0] matrix_address=0;
reg [31:0] matrix_data=0;
wire [0:0] cfg_ready;
wire [0:0] pixel_ready;
wire [0:0] result_valid;
wire [8:0] result_x;
wire [7:0] result_y;
wire [1:0] result_lane;
wire [3:0] result_mask;
wire [14:0] result_origin;
wire [35:0] result_normal;
wire [31:0] result_ndc;
wire [35:0] result_uv;
wire [23:0] result_rgb;
wire [15:0] result_depth;
wire [0:0] result_coarse;
wire [0:0] result_fault;
wire [0:0] phase;
wire [3:0] stage_valid;
wire [17:0] rcp_product;
wire [35:0] ratio_product;
wire [35:0] depth_product;
wire [0:0] raster_busy;
wire [0:0] raster_done;
wire [0:0] raster_fault;
wire [0:0] packet_ready;
wire [0:0] vertex_valid;
wire [0:0] vertex_last;
wire [0:0] vertex_busy;
wire [0:0] vertex_fault;
wire [35:0] vertex_data;
reg [8:0] gold_x;
reg [7:0] gold_y;
reg [1:0] gold_lane;
reg [3:0] gold_mask;
reg [14:0] gold_origin;
reg [35:0] gold_normal;
reg [31:0] gold_ndc;
reg [35:0] gold_uv;
reg [23:0] gold_rgb;
reg [15:0] gold_depth;
reg [0:0] gold_coarse;
reg [0:0] gold_fault;
probe dut(.clk(clk),.ce(ce),.reset(reset),.packet_valid(packet_valid),.packet_last(packet_last),.packet_data(packet_data),.result_ready(result_ready),.cfg_ready(cfg_ready),.pixel_ready(pixel_ready),.result_valid(result_valid),.result_x(result_x),.result_y(result_y),.result_lane(result_lane),.result_mask(result_mask),.result_origin(result_origin),.result_normal(result_normal),.result_ndc(result_ndc),.result_uv(result_uv),.result_rgb(result_rgb),.result_depth(result_depth),.result_coarse(result_coarse),.result_fault(result_fault),.phase(phase),.stage_valid(stage_valid),.rcp_product(rcp_product),.ratio_product(ratio_product),.depth_product(depth_product),.raster_busy(raster_busy),.raster_done(raster_done),.raster_fault(raster_fault),.packet_ready(packet_ready),.vertex_mode(vertex_mode),.matrix_write(matrix_write),.vertex_output_ready(vertex_output_ready),.matrix_address(matrix_address),.matrix_data(matrix_data),.vertex_valid(vertex_valid),.vertex_last(vertex_last),.vertex_busy(vertex_busy),.vertex_fault(vertex_fault),.vertex_data(vertex_data));
reg [35:0] vertex_store[0:319],vinput[0:319],vgold[0:319];reg [31:0] matrices[0:20];
integer vi=0,vo=0,vk=0,mk=0,tf,tr,ref0,ref1,ref2,mode,ti,selected;reg vertex_held=0;reg [36:0] vertex_saved;
integer cycle=0,outputs=0,f,r,gold_file,packet_gold,packet_words=0,last_out=-1,elapsed=0;
integer vertex_finish=-1,geometry_finish=-1,first_packet=-1,last_packet=-1;
integer streak=0,max_streak=0,first_out=-1;
reg [35:0] expected_word;reg expected_last;reg [183:0] held;reg hold_check=0;reg [31:0] cadence_count[0:31];integer k;
always @(negedge clk)if(!reset)begin
ce=0 ? (cycle%13!=4) : 1;
vertex_output_ready=0 ? !(cycle%19>=7 && cycle%19<=10) : 1;
result_ready=0 ? !(cycle%17>=5 && cycle%17<=8) : 1;
end
always @(posedge clk)if(!reset)begin
cycle=cycle+1;if(cycle>4000000)$fatal;
if(hold_check && {result_x,result_y,result_lane,result_mask,result_origin,result_normal,result_ndc,result_uv,result_rgb,result_depth,result_coarse,result_fault}!==held)$fatal;
hold_check=result_valid && !(ce && result_ready);held={result_x,result_y,result_lane,result_mask,result_origin,result_normal,result_ndc,result_uv,result_rgb,result_depth,result_coarse,result_fault};


if(ce && result_valid && result_ready)begin
r=$fscanf(gold_file,"%h %h %h %h %h %h %h %h %h %h %h %h\n",gold_x,gold_y,gold_lane,gold_mask,gold_origin,gold_normal,gold_ndc,gold_uv,gold_rgb,gold_depth,gold_coarse,gold_fault);
if(r!=12)begin $display("FAIL extra output %0d",outputs);$fatal;end
if(result_x!==gold_x)begin $display("FAIL output %0d x: %h != %h",outputs,result_x,gold_x);$fatal;end
if(result_y!==gold_y)begin $display("FAIL output %0d y: %h != %h",outputs,result_y,gold_y);$fatal;end
if(result_lane!==gold_lane)begin $display("FAIL output %0d lane: %h != %h",outputs,result_lane,gold_lane);$fatal;end
if(result_mask!==gold_mask)begin $display("FAIL output %0d mask: %h != %h",outputs,result_mask,gold_mask);$fatal;end
if(result_origin!==gold_origin)begin $display("FAIL output %0d origin: %h != %h",outputs,result_origin,gold_origin);$fatal;end
if(result_normal!==gold_normal)begin $display("FAIL output %0d normal: %h != %h",outputs,result_normal,gold_normal);$fatal;end
if(result_ndc!==gold_ndc)begin $display("FAIL output %0d ndc: %h != %h",outputs,result_ndc,gold_ndc);$fatal;end
if(result_uv!==gold_uv)begin $display("FAIL output %0d uv: %h != %h",outputs,result_uv,gold_uv);$fatal;end
if(result_rgb!==gold_rgb)begin $display("FAIL output %0d rgb: %h != %h",outputs,result_rgb,gold_rgb);$fatal;end
if(result_depth!==gold_depth)begin $display("FAIL output %0d depth: %h != %h",outputs,result_depth,gold_depth);$fatal;end
if(result_coarse!==gold_coarse)begin $display("FAIL output %0d coarse: %h != %h",outputs,result_coarse,gold_coarse);$fatal;end
if(result_fault!==gold_fault)begin $display("FAIL output %0d fault: %h != %h",outputs,result_fault,gold_fault);$fatal;end
if(last_out>=0 && cycle-last_out<32)cadence_count[cycle-last_out]=cadence_count[cycle-last_out]+1;
if(first_out<0)first_out=cycle;if(last_out>=0 && cycle-last_out==2)streak=streak+1;else streak=0;if(streak>max_streak)max_streak=streak;last_out=cycle;outputs=outputs+1;
end
if(vertex_held && {vertex_last,vertex_data}!==vertex_saved)$fatal;
vertex_held=vertex_valid && !(ce && vertex_output_ready);vertex_saved={vertex_last,vertex_data};
if(ce && vertex_valid && vertex_output_ready)begin if(vertex_data!==vgold[vo] || vertex_last!==(vo%5==4))begin $display("FAIL vertex word=%0d",vo);$fatal;end vertex_store[vo]=vertex_data;vo=vo+1;end
if(raster_fault)$fatal;
end
initial begin
for(k=0;k<32;k=k+1)cadence_count[k]=0;
reset=1;ce=1;result_ready=1;gold_file=$fopen("gold.hex","r");packet_gold=$fopen("packets.hex","r");
@(negedge clk);reset=0;

$readmemh("vertex-input.hex",vinput);$readmemh("vertex-gold.hex",vgold);$readmemh("matrix.hex",matrices);
vertex_mode=1;packet_valid=0;vertex_output_ready=1;
for(mk=0;mk<21;mk=mk+1)begin
@(negedge clk);matrix_write=1;matrix_address=mk;matrix_data=matrices[mk];
begin:waitmatrix forever begin @(posedge clk);if(ce)disable waitmatrix;end end
end
@(negedge clk);matrix_write=0;
for(vi=0;vi<45;vi=vi+1)begin
@(negedge clk);packet_valid=1;packet_data=vinput[vi];packet_last=vi%5==4;
begin:waitvertex forever begin @(posedge clk);if(ce && packet_ready)disable waitvertex;end end end
@(negedge clk);packet_valid=0;
while(vo<45 || vertex_busy)@(negedge clk);
vertex_mode=0;tf=$fopen("refs.hex","r");
while(!$feof(tf))begin tr=$fscanf(tf,"%h %h %h %h\n",ref0,ref1,ref2,mode);
if(tr==4)begin
@(negedge clk);packet_data=(ref0&63)|((ref1&63)<<6)|((ref2&63)<<12)|((mode&1)<<18);packet_valid=1;packet_last=1;
begin:waitreference forever begin @(posedge clk);if(ce && packet_ready)disable waitreference;end end
end end
@(negedge clk);packet_valid=0;

while(!(!raster_busy && stage_valid==0 && !result_valid))begin @(negedge clk);elapsed=elapsed+1;if(elapsed>4000000)$fatal;end
repeat(3)@(negedge clk);
if(outputs!=35924)begin $display("FAIL count %0d != 35924",outputs);$fatal;end
if(0 && packet_words!=45)$fatal;
$display("PASS backend_low_common5normal9cachechain-n9t3-s200-c0-p0 clocks=%0d outputs=%0d packet_words=%0d",cycle,outputs,packet_words);
$display("TIMES vertex_finish=%0d geometry_finish=%0d first_packet=%0d last_packet=%0d",vertex_finish,geometry_finish,first_packet,last_packet);
for(k=1;k<32;k=k+1)if(cadence_count[k])$display("GAP %0d %0d",k,cadence_count[k]);
$display("DENSE first_out=%0d last_out=%0d max_gap2_streak=%0d",first_out,last_out,max_streak);$display("CACHE vertices=%0d triangles=3 shared_wide=1",vo/5);$finish;end
initial begin #21000000;$fatal;end
endmodule
