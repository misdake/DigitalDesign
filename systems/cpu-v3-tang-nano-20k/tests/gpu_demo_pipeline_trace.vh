// Two offscreen submissions, keeping Display Front A protected throughout.
integer pipe_slot=0,pipe_submit=0,pipe_completions=0;
integer pipe_preparing=0,pipe_overlap=0,pipe_prepared_busy=0,pipe_dump=-1;
reg pipe_was_busy=0;
always @(posedge clk)if(!reset)begin
 if(wen&&di==7&&ch==8)begin pipe_preparing=1;$display("PIPE_PREP_BEGIN cycle=%0d slot=%0d gpu_busy=%0d",cycle,wdata,gpu.busy);end
 if(wen&&di==7&&ch==9)begin
  if(gpu.busy)pipe_prepared_busy=pipe_prepared_busy+1;
  pipe_preparing=0;$display("PIPE_PREP_END cycle=%0d slot=%0d gpu_busy=%0d",cycle,wdata,gpu.busy);
 end
 if(pipe_preparing&&gpu.busy)pipe_overlap=pipe_overlap+1;
 if(wen&&di==7&&ch==10)pipe_slot=wdata;
 if(wen&&di==4&&ch==1)begin
  if(gpu.busy)$fatal(1,"Overlapping GPU submissions");
  pipe_dump=pipe_submit;pipe_submit=pipe_submit+1;
  $display("PIPE_SUBMIT cycle=%0d slot=%0d base=%h",cycle,pipe_slot,wdata);
 end
 if(pipe_was_busy&&!gpu.busy)begin
  pipe_completions=pipe_completions+1;
  $display("PIPE_GPU_DONE cycle=%0d slot=%0d",cycle,pipe_slot);
 end
 pipe_was_busy=gpu.busy;
 if(gpu.busy&&dq&&dr&&dw)begin
  if((da>='h490000/2+pipe_slot*16384&&da<'h490000/2+(pipe_slot+1)*16384)||
     (da>='h4a0000/2+pipe_slot*16384&&da<'h4a0000/2+(pipe_slot+1)*16384))$fatal(1,"CPU modified active GPU input slot");
 end
 if(cycle>2000000)$fatal(1,"pipeline watchdog");
end
always @(negedge clk)if(pipe_dump>=0)begin
 if(pipe_dump==0)$writememh("__PIPE_OUT__/submit-0.hex",mem,'h490000/8,'h4b0000/8-1);
 else if(pipe_dump==1)$writememh("__PIPE_OUT__/submit-1.hex",mem,'h490000/8,'h4b0000/8-1);
 pipe_dump=-1;
end
