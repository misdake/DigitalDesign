//! Complete ordered Runtime packet pool controller, three storage alternatives.
//! MODE0 is newly lowered descriptor-array reference; no forced FF attributes.
//! MODE1 uses synchronous owner BSRAM; MODE2 uses asynchronous owner SSRAM.
//! Payload RAM, heads, visibility, credits and fault checks are common.
pub fn source(mode: u8, native: bool) -> Result<String, String> {
    if mode > 2 {
        return Err("packet store RTL mode".into());
    }
    Ok(format!(
        "module gpu_v2_packet_store #(parameter MODE={mode}, NATIVE={})(\n",
        u8::from(native)
    ) + BODY)
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
 reg [15:0] published;
 reg [1:0] hv;
 reg rh,ph,pending,phead,owner_valid;
 reg [5:0] prow;
 reg [3:0] owner_row;
 reg [71:0] head0,head1;
 reg [5:0] na,nw,nt,nr,nreclaim;
 reg [4:0] npc,nuw;
 reg [5:0] ngc,nrows;
 reg [15:0] npub;
 reg [1:0] nhv;
 reg nrh,nph,npending,nphead,nowner_valid;
 reg [5:0] nprow;
 reg [3:0] nowner_row;
 reg fire_t,fire_r,invalid;
 wire [2:0] heads={2'b0,hv[0]}+{2'b0,hv[1]};
 wire [2:0] held=heads+pending;
 wire [5:0] unread=gc-heads-pending;
 wire [5:0] da_t=alloc-tptr,da_w=alloc-wptr,da_c=alloc-reclaim,dt_r=tptr-rptr,dr_c=rptr-reclaim;
 wire [3:0] owner_ra=wptr[3:0]+write_valid;
 wire running=ce&&!fault&&!reset;
 assign bad=running&&invalid;
 wire commit=running&&!bad;
 wire ow=commit&&reserve_valid;
 wire or_en=commit&&owner_request;
 wire pw=commit&&write_valid;
 wire pr=commit&&fire_r;
 wire take=commit&&consume;
 wire cap=commit&&pending;
 wire transfer=commit&&fire_t;
 wire [71:0] data_do;
 wire [5:0] stored_owner;
 wire [5:0] referenced_write_row,referenced_transfer_row,referenced_read_row;
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
 assign owner_value=stored_owner;assign returned_payload=data_do;
 always @* begin
  na=alloc;nw=wptr;nt=tptr;nr=rptr;nreclaim=reclaim;
  npc=pc;nuw=uw;ngc=gc;nrows=rows;npub=published;nhv=hv;
  nrh=rh;nph=ph;npending=pending;nphead=phead;nprow=prow;
  nowner_valid=owner_valid;nowner_row=owner_row;
  invalid=0;fire_t=0;fire_r=0;
  if(running) begin
   if(pc>16||gc>32||rows>48||uw>pc||held>2||gc<held||rows!=pc+gc-heads)invalid=1;
   if((rh^ph)!=held[0]||(heads!=0&&!hv[ph])||(pending&&(phead==rh||hv[phead])))invalid=1;
   if(da_t!=pc||da_w!=uw||da_c!=rows||dt_r!=unread||dr_c!=pending)invalid=1;
   if(owner_valid&&owner_row!=wptr[3:0])invalid=1;
   if(pc!=0&&published[tptr[3:0]]!=(pc>uw))invalid=1;
   if(consume)begin
    if(!hv[ph])invalid=1;
    nhv[ph]=0;nph=!ph;ngc=ngc-1'b1;
   end
   if(reserve_valid)begin
    if(pc>=16||rows>=48||published[alloc[3:0]])invalid=1;
    na=alloc+1'b1;npc=npc+1'b1;nuw=nuw+1'b1;nrows=nrows+1'b1;
   end
   if(write_valid)begin
    if(uw==0||!owner_valid||owner_row!=wptr[3:0]||published[wptr[3:0]]||referenced_write_row!=wptr)invalid=1;
    if({write_payload[69:66],write_payload[71:70]}!=stored_owner)invalid=1;
    nw=wptr+1'b1;nuw=nuw-1'b1;npub[wptr[3:0]]=1;nowner_valid=0;
   end
   if(owner_request)begin
    if(nowner_valid||uw<=write_valid||(reserve_valid&&owner_ra==alloc[3:0]))invalid=1;
    nowner_valid=1;nowner_row=owner_ra;
   end
   if(pending)begin
    if(nhv[phead]||prow!=reclaim||rows==0)invalid=1;
    nhv[phead]=1;npending=0;nreclaim=reclaim+1'b1;nrows=nrows-1'b1;
   end
   if(pc!=0&&gc<32&&published[tptr[3:0]])begin
    if((write_valid&&wptr==tptr)||referenced_transfer_row!=tptr)invalid=1;
    fire_t=1;nt=tptr+1'b1;npub[tptr[3:0]]=0;npc=npc-1'b1;ngc=ngc+1'b1;
   end
   if(unread!=0&&!nhv[rh])begin
    if((write_valid&&wptr==rptr)||referenced_read_row!=rptr)invalid=1;
    fire_r=1;nr=rptr+1'b1;npending=1;nprow=rptr;nphead=rh;nrh=!rh;
   end
  end
 end
 always @(posedge clk)begin
  if(reset)begin
   alloc<=0;wptr<=0;tptr<=0;rptr<=0;reclaim<=0;pc<=0;uw<=0;gc<=0;rows<=0;
   published<=0;hv<=0;rh<=0;ph<=0;pending<=0;phead<=0;prow<=0;
   owner_valid<=0;owner_row<=0;fault<=0;
  end else if(running)begin
   if(bad)fault<=1;
   else begin
    alloc<=na;wptr<=nw;tptr<=nt;rptr<=nr;reclaim<=nreclaim;
    pc<=npc;uw<=nuw;gc<=ngc;rows<=nrows;published<=npub;hv<=nhv;
    rh<=nrh;ph<=nph;pending<=npending;phead<=nphead;prow<=nprow;
    owner_valid<=nowner_valid;owner_row<=nowner_row;
    if(pending)begin if(phead)head1<=data_do;else head0<=data_do;end
   end
  end
 end
 generate if(MODE==0)begin:descriptor_reference
  reg [11:0] producer[0:15];reg [5:0] groups[0:31];
  reg [4:0] gw,gr;
  always @(posedge clk)begin
   if(reset)begin gw<=0;gr<=0;end
   else begin if(transfer)gw<=gw+1'b1;if(pr)gr<=gr+1'b1;end
   if(ow)producer[alloc[3:0]]<={alloc,reserve_owner};
   if(transfer)groups[gw]<=producer[tptr[3:0]][11:6];
  end
  assign stored_owner=producer[wptr[3:0]][5:0];
  assign referenced_write_row=producer[wptr[3:0]][11:6];
  assign referenced_transfer_row=producer[tptr[3:0]][11:6];
  assign referenced_read_row=groups[gr];
 end else begin:ordered
  assign referenced_write_row=wptr;assign referenced_transfer_row=tptr;assign referenced_read_row=rptr;
  if(MODE==1)begin:synchronous_owner
   if(NATIVE)begin:hardware
    wire [35:0] q;
    SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.BLK_SEL_0(0),.BLK_SEL_1(0),.RESET_MODE("SYNC")) bank(
     .DO(q),.DI({30'b0,reserve_owner}),.BLKSELA(3'b0),.BLKSELB(3'b0),
     .ADA({5'b0,alloc[3:0],1'b0,4'b1111}),.ADB({5'b0,owner_ra,5'b0}),
     .CLKA(clk),.CLKB(clk),.CEA(ow),.CEB(or_en),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
    assign stored_owner=q[5:0];
   end else begin:model
    reg [5:0] mem[0:15];reg [5:0] q;
    always @(posedge clk)begin
     if(ow)mem[alloc[3:0]]<=reserve_owner;
     if(reset)q<=0;else if(or_en)q<=mem[owner_ra];
    end
    assign stored_owner=q;
   end
  end else begin:asynchronous_owner
   if(NATIVE)begin:hardware
    wire [7:0] q;wire[7:0] d={2'b0,reserve_owner};
    genvar i;for(i=0;i<2;i=i+1)begin:cells
     RAM16SDP4 bank(.DO(q[4*i+:4]),.DI(d[4*i+:4]),.WAD(alloc[3:0]),.RAD(wptr[3:0]),.CLK(clk),.WRE(ow));
    end
    assign stored_owner=q[5:0];
   end else begin:model
    reg [5:0] mem[0:15];always @(posedge clk)if(ow)mem[alloc[3:0]]<=reserve_owner;
    assign stored_owner=mem[wptr[3:0]];
   end
  end
 end endgenerate
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
