//! Trusted ordered Runtime producer, unchanged generic PacketPool diagnostics.
//! Reserve is actual Packet E0 capture; same CE, E2 in-order write, no holes.
pub fn source(native: bool) -> String {
    format!(
        "module gpu_v2_packet_store_ordered #(parameter MODE=1, NATIVE={})(\n",
        u8::from(native)
    ) + BODY
}
const BODY: &str = r#"input wire clk,reset,ce,
 input wire reserve_valid,input wire [5:0] reserve_owner,
 input wire write_valid,input wire [71:0] write_payload,
 input wire owner_request,consume,
 output wire producer_ready,front_valid,output wire [71:0] front_payload,
 output reg fault,output wire bad,
 output wire [4:0] producer_count,unwritten_count,
 output wire [5:0] group_count,row_count,
 output wire [5:0] allocate_row,write_row,transfer_row,read_row,reclaim_row,
 output wire [3:0] owner_address,
 output wire [5:0] strobes,
 output wire [1:0] head_valid_out,output wire pending_out,
 output wire [5:0] owner_value,
 output wire [71:0] returned_payload);
 reg [5:0] alloc,wptr,tptr,rptr,reclaim;
 reg [4:0] pc,uw;
 reg [5:0] gc,rows;
 reg [1:0] hv;
 reg rh,ph,pending,phead;
 reg [5:0] prow;
 reg [71:0] head0,head1;
 reg [5:0] na,nw,nt,nr,nreclaim;
 reg [4:0] npc,nuw;
 reg [5:0] ngc,nrows;
 reg [1:0] nhv;
 reg nrh,nph,npending,nphead;
 reg [5:0] nprow;
 reg fire_t,fire_r,invalid;
 wire [3:0] owner_ra=wptr[3:0]+write_valid;
 wire running=ce&&!fault&&!reset;
 assign bad=running&&invalid;
 wire commit=running&&!bad;
 wire ow=commit&&reserve_valid;
 wire pw=commit&&write_valid;
 wire pr=commit&&fire_r;
 wire take=commit&&consume;
 wire cap=commit&&pending;
 wire transfer=commit&&fire_t;
 wire [71:0] data_do;
 assign producer_ready=!fault&&(pc<16);
 assign front_valid=hv[ph];
 assign front_payload=ph?head1:head0;
 assign producer_count=pc;assign unwritten_count=uw;
 assign group_count=gc;assign row_count=rows;
 assign allocate_row=alloc;assign write_row=wptr;assign transfer_row=tptr;
 assign read_row=rptr;assign reclaim_row=reclaim;
 assign owner_address=owner_ra;
 assign strobes={take,cap,pr,transfer,pw,ow};
 assign head_valid_out=hv;assign pending_out=pending;
 assign owner_value=6'b0;assign returned_payload=data_do;
 always @* begin
  na=alloc;nw=wptr;nt=tptr;nr=rptr;nreclaim=reclaim;
  npc=pc;nuw=uw;ngc=gc;nrows=rows;nhv=hv;
  nrh=rh;nph=ph;npending=pending;nphead=phead;nprow=prow;
  invalid=0;fire_t=0;fire_r=0;
  if(running) begin
   if(consume)begin
    if(!hv[ph])invalid=1;
    nhv[ph]=0;nph=!ph;ngc=ngc-1'b1;
   end
   if(reserve_valid)begin
    if(pc>=16||rows>=48)invalid=1;
    na=alloc+1'b1;npc=npc+1'b1;nuw=nuw+1'b1;nrows=nrows+1'b1;
   end
   if(write_valid)begin
    if(uw==0)invalid=1;
    nw=wptr+1'b1;nuw=nuw-1'b1;
   end
   if(pending)begin
    nhv[phead]=1;npending=0;nreclaim=reclaim+1'b1;nrows=nrows-1'b1;
   end
   if(gc<32&&pc!=uw)begin
    fire_t=1;nt=tptr+1'b1;npc=npc-1'b1;ngc=ngc+1'b1;
   end
   if(rptr!=tptr&&!nhv[rh])begin
    fire_r=1;nr=rptr+1'b1;npending=1;nprow=rptr;nphead=rh;nrh=!rh;
   end
  end
 end
 always @(posedge clk)begin
  if(reset)begin
   alloc<=0;wptr<=0;tptr<=0;rptr<=0;reclaim<=0;pc<=0;uw<=0;gc<=0;rows<=0;
   hv<=0;rh<=0;ph<=0;pending<=0;phead<=0;prow<=0;
   fault<=0;
  end else if(running)begin
   if(bad)fault<=1;
   else begin
    alloc<=na;wptr<=nw;tptr<=nt;rptr<=nr;reclaim<=nreclaim;
    pc<=npc;uw<=nuw;gc<=ngc;rows<=nrows;hv<=nhv;
    rh<=nrh;ph<=nph;pending<=npending;phead<=nphead;prow<=nprow;
    if(pending)begin if(phead)head1<=data_do;else head0<=data_do;end
   end
  end
 end
 generate if(NATIVE)begin:payload_hardware
  genvar b;for(b=0;b<2;b=b+1)begin:banks
   SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.BLK_SEL_0(0),.BLK_SEL_1(0),.RESET_MODE("SYNC")) bank(
    .DO(data_do[b*36+:36]),.DI(write_payload[b*36+:36]),.BLKSELA(3'b0),.BLKSELB(3'b0),
    .ADA({3'b0,wptr,1'b0,4'b1111}),.ADB({3'b0,rptr,5'b0}),
    .CLKA(clk),.CLKB(clk),.CEA(pw),.CEB(pr),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
  end
 end else begin:payload_model
  reg [71:0] mem[0:63];reg [71:0] q;
  always @(posedge clk)begin
   if(pw)mem[wptr]<=write_payload;
   if(reset)q<=0;else if(pr)q<=mem[rptr];
  end
  assign data_do=q;
 end endgenerate
endmodule
"#;
