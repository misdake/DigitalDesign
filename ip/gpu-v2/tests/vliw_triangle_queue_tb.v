`timescale 1ns/1ps
module tb;
 reg clk=0;always #5 clk=~clk;
`ifndef PORTABLE
 GSR GSR(.GSRI(1'b1));
`endif
 localparam DATA="@DATA@",NV=@NV@,NT=@NT@,NP=@NP@,DEPTH=@DEPTH@,MAXC=100000;
 reg reset=1,ce=1,start=0,slot=@SLOT@,dma_we=0;
 reg[9:0]dma_addr=0;reg[63:0]dma_data=0;
 reg output_ready=1,packet_ready=0,word_ready=0;
 wire ready,busy,done,dma_ready,packet_valid,word_valid,emit_valid;
 wire[3:0]error;wire[6:0]published_vertices;
 wire[35:0]descriptor,emit_data;wire[53:0]word_data;
 wire[4:0]word_index;wire[9:0]emit_addr;
 shared_meshlet dut(.*);
 reg[35:0]vertices[0:319],descriptors[0:95],admissions[0:95];
 reg[53:0]words[0:1535];
 integer cycle=0,vi=0,ti=0,pi=0,wi=0,retired=0,peak=0,wraps=0,full_cycles=0;
 integer ce_holds=0,j,dm,rc,addr;reg[63:0]value;
 reg saw_done=0,drain=0,full_seen=0,dma_taken,start_taken;
 reg[31:0]rng=32'h79a42f31;reg[31:0]frozen;
 reg header_held=0;reg[35:0]held_descriptor;
 always @(posedge clk)begin
  if(reset)header_held=0;
  else begin
   if(header_held&&(!packet_valid||descriptor!==held_descriptor))
    $fatal(1,"T descriptor changed before acceptance");
   header_held=packet_valid&&(!ce||!packet_ready);held_descriptor=descriptor;
  end
 end
 always @(posedge clk)begin
  cycle=cycle+1;if(cycle>MAXC)$fatal(1,"T queue watchdog");
  dma_taken=!reset&&dma_we&&dma_ready;
  start_taken=!reset&&ce&&start&&ready;
  if(reset)begin
   vi=0;ti=0;pi=0;wi=0;retired=0;peak=0;wraps=0;full_cycles=0;
   saw_done=0;full_seen=0;
  end else if(ce)begin
   if(error)$fatal(1,"meshlet error %h",error);
   if(done)saw_done=1;
   if(dut.core.count>peak)peak=dut.core.count;
   if(dut.core.count==DEPTH)begin
    full_seen=1;full_cycles=full_cycles+1;
    if(dut.core.launch_t)$fatal(1,"T borrowed same-edge full credit");
   end
   if(dut.core.publish&&dut.core.write_slot==DEPTH-1)wraps=wraps+1;
   if(dut.core.launch_t)begin
    if(vi!=NV*5||ti>=NT||dut.core.triangle_descriptor!==admissions[ti])$fatal(1,"T admission %0d",ti);
    ti=ti+1;
   end
   if(emit_valid&&output_ready)begin
    if(vi>=NV*5||emit_addr!==vi[9:0]||emit_data!==vertices[vi])$fatal(1,"V result %0d",vi);
    vi=vi+1;
   end
   if(packet_valid&&packet_ready)begin
    if(pi>=NP||descriptor!==descriptors[pi])$fatal(1,"T descriptor %0d",pi);
    pi=pi+1;wi=0;
   end
   if(word_valid&&word_ready)begin
    if(pi==0||wi>=16||word_index!==wi[4:0]||word_data!==words[(pi-1)*16+wi])
     $fatal(1,"T item packet%0d item%0d",pi-1,wi);
    wi=wi+1;if(wi==16)retired=retired+1;
   end
  end
 end
 task tick;begin
  @(negedge clk);rng=rng*32'd1664525+32'd1013904223;
  ce=rng[2:0]!=0;output_ready=rng[5:3]!=0;
  packet_ready=drain&&rng[8:6]!=0;
  word_ready=drain&&rng[11:9]!=0&&!(cycle%137>=40&&cycle%137<62);
  #1;frozen={dut.core.count,dut.core.write_slot,dut.core.read_slot,dut.core.import_state};
  @(posedge clk);#2;
  if(!reset&&!ce)begin
   if(frozen!=={dut.core.count,dut.core.write_slot,dut.core.read_slot,dut.core.import_state})
    $fatal(1,"T queue changed during CE0");
   ce_holds=ce_holds+1;
  end
 end endtask
 task launch;begin
  while(!ready)tick();start=1;start_taken=0;
  while(!start_taken)tick();start=0;
 end endtask
 initial begin
  $readmemh({DATA,"/vertices.hex"},vertices,0,NV*5-1);
  $readmemh({DATA,"/admissions.hex"},admissions,0,NT-1);
  $readmemh({DATA,"/descriptors.hex"},descriptors,0,NP-1);
  $readmemh({DATA,"/words.hex"},words,0,NP*16-1);
  repeat(2)tick();reset=0;
  dm=$fopen({DATA,"/dma.txt"},"r");if(dm==0)$fatal(1,"DMA fixture");
  while(!$feof(dm))begin
   rc=$fscanf(dm,"%d %h",addr,value);
   if(rc==2)begin
    dma_addr=addr;dma_data=value;dma_we=1;dma_taken=0;
    while(!dma_taken)tick();
   end
  end
  $fclose(dm);dma_we=0;
  launch();while(!full_seen)tick();repeat(12)tick();
  // Cancel a full queue; stale descriptors and RAM contents must stay invalid.
  reset=1;repeat(2)tick();reset=0;
  launch();while(!full_seen)tick();repeat(12)tick();drain=1;
  while(!saw_done||!ready||retired!=NP)tick();
  if(vi!=NV*5||ti!=NT||pi!=NP||wi!=16||peak!=DEPTH||wraps<2||!ce_holds)
   $fatal(1,"T queue final counts %0d/%0d/%0d peak%0d wraps%0d",vi,ti,pi,peak,wraps);
  $display("PASS T_QUEUE slots=%0d packets=%0d peak=%0d wraps=%0d full=%0d ce0=%0d cycles=%0d",
   DEPTH,NP,peak,wraps,full_cycles,ce_holds,cycle);$finish;
 end
 initial begin #1000100;$fatal(1,"T queue time watchdog");end
endmodule
