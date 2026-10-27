//! Explicit canonical-description experiments for the frozen cadence factory.
//! The checked standalone ingress and production factory remain independent.
#[derive(Clone, Copy, Debug)]
pub enum Authority {
    HeadWire,
    Ingress,
}
pub fn lower(mut source: String, authority: Authority) -> Result<String, String> {
    source = source.replace("\r\n", "\n");
    let patches = match authority {
        Authority::HeadWire => HEAD_WIRE,
        Authority::Ingress => INGRESS_AUTHORITY,
    };
    for &(old, new) in patches {
        if source.matches(old).count() != 1 {
            return Err(format!(
                "Quad authority anchor {}",
                old.lines().next().unwrap_or("")
            ));
        }
        source = source.replacen(old, new, 1);
    }
    Ok(source)
}
const HEAD_WIRE: &[(&str, &str)] = &[
    (
        r####" assign lr=compute_ce;assign sr=compute_ce;
 assign light_result=advance&&lv&&lr;assign sample_result=sfire;
 reg active;reg[22:0]header;
 reg[2:0]state;reg[1:0]issue_lane,out_lane;
 reg paid;reg[3:0]depth_valid,depth_reserved;
"####,
        r####" assign lr=compute_ce;assign sr=compute_ce;
 assign light_result=advance&&lv&&lr;assign sample_result=sfire;
 reg active;wire[22:0]header=head_descriptor;
 reg[2:0]state;reg[1:0]issue_lane,out_lane;
 reg paid;reg[3:0]depth_valid,depth_reserved;
"####,
    ),
    (
        r####"    if(sample_result&&sl)begin sample_complete<=1;sample_last<=sample_address[6:2];end
    if(!active&&published_count!=0)begin
     active<=1;header<=descriptors[retire_slot];out_lane<=0;
    end
    if(active)case(state)
"####,
        r####"    if(sample_result&&sl)begin sample_complete<=1;sample_last<=sample_address[6:2];end
    if(!active&&published_count!=0)begin
     active<=1;out_lane<=0;
    end
    if(active)case(state)
"####,
    ),
    (
        r####"    if(lane_accept)begin out_lane<=out_lane+1'b1;color_paid<=0;depth_valid[out_lane]<=0;depth_reserved[out_lane]<=0;end
    if(quad_accepted)begin
     if(published_count>1)begin active<=1;header<=descriptors[retire_slot+5'd1];end
     else active<=0;
    end
"####,
        r####"    if(lane_accept)begin out_lane<=out_lane+1'b1;color_paid<=0;depth_valid[out_lane]<=0;depth_reserved[out_lane]<=0;end
    if(quad_accepted)begin
     if(published_count>1)begin active<=1;end
     else active<=0;
    end
"####,
    ),
];
const INGRESS_AUTHORITY: &[(&str, &str)] = &[
    (
        r####" wire final_in_ready,final_out_valid;wire[23:0]final_RGB;
 wire[7:0]basic_address;wire[23:0]basic_data;
 wire[4:0]retire_slot;wire[1:0]light_used;wire[3:0]sample_used;
 wire lv,lr,ll;wire[6:0]lk;wire[8:0]lg,lh;
 wire sv,sr,sl,sfire,public_publish;wire[5:0]sk;wire[23:0]sRGB;
"####,
        r####" wire final_in_ready,final_out_valid;wire[23:0]final_RGB;
 wire[7:0]basic_address;wire[23:0]basic_data;
 reg[4:0]issue_slot;wire[22:0]issue_descriptor,head_descriptor;wire[4:0]retire_slot;wire[1:0]light_used;wire[3:0]sample_used;
 wire lv,lr,ll;wire[6:0]lk;wire[8:0]lg,lh;
 wire sv,sr,sl,sfire,public_publish;wire[5:0]sk;wire[23:0]sRGB;
"####,
    ),
    (
        r####" .in_ready(in_ready),.in_row(in_row),.advance(advance),.preflight_bad(front_bad),.fault(front_fault),
 .published(published),.published_slot(published_slot),.published_begin(published_begin),
 .basic_read(basic_read),.basic_read_address(basic_address),.basic_return_valid(basic_valid),
 .basic_return_data(basic_data),.basic_collision(basic_collision),
"####,
        r####" .in_ready(in_ready),.in_row(in_row),.advance(advance),.preflight_bad(front_bad),.fault(front_fault),
 .published(published),.published_slot(published_slot),.published_begin(published_begin),
 .descriptor_issue_slot(issue_slot),.descriptor_head_slot(quad_accepted?retire_slot+5'd1:retire_slot),.descriptor_issue(issue_descriptor),.descriptor_head(head_descriptor),
 .basic_read(basic_read),.basic_read_address(basic_address),.basic_return_valid(basic_valid),
 .basic_return_data(basic_data),.basic_collision(basic_collision),
"####,
    ),
    (
        r####" (* syn_ramstyle="distributed_ram" *)reg public_upper[0:15];
 wire[6:0]sample_address={public_upper[sk[5:2]],sk};
 (* syn_ramstyle="distributed_ram" *)reg[22:0]descriptors[0:31];
 reg light_complete,sample_complete;reg[4:0]light_last,sample_last;wire[22:0]head_descriptor=descriptors[retire_slot];
 reg[5:0]published_count;
 assign lr=compute_ce;assign sr=compute_ce;
"####,
        r####" (* syn_ramstyle="distributed_ram" *)reg public_upper[0:15];
 wire[6:0]sample_address={public_upper[sk[5:2]],sk};

 reg light_complete,sample_complete;reg[4:0]light_last,sample_last;
 reg[5:0]published_count;
 assign lr=compute_ce;assign sr=compute_ce;
"####,
    ),
    (
        r####" reg paid;reg[3:0]depth_valid,depth_reserved;
 reg[15:0]depth_rows[0:3];
 reg[4:0]issue_slot;
 reg depth_pending;reg[1:0]depth_tag,input_bypass;
 localparam J_READ=0,J_WAIT=1,J_ISSUE=2,J_DEPTH=3,J_DEPTH_WAIT=4,J_DRAIN=5;
 wire issue_ahead=(issue_slot!=retire_slot);
 wire[22:0]issue_header=descriptors[issue_slot];
 wire issue_exists=published_count>{5'd0,issue_ahead};
 // For the sole next quad, an ordered completed prefix must extend past
"####,
        r####" reg paid;reg[3:0]depth_valid,depth_reserved;
 reg[15:0]depth_rows[0:3];
 reg depth_pending;reg[1:0]depth_tag,input_bypass;
 localparam J_READ=0,J_WAIT=1,J_ISSUE=2,J_DEPTH=3,J_DEPTH_WAIT=4,J_DRAIN=5;
 wire issue_ahead=(issue_slot!=retire_slot);
 wire[22:0]issue_header=issue_descriptor;
 wire issue_exists=published_count>{5'd0,issue_ahead};
 // For the sole next quad, an ordered completed prefix must extend past
"####,
    ),
    (
        r####"     2'b01:published_count<=published_count-1'b1;
    endcase
    if(published)begin
     descriptors[published_slot]<=published_begin;
     // Bypasses are read from the canonical head descriptor.
    end
    if(quad_accepted&&retire_slot==light_last)light_complete<=0;
    if(light_result&&ll)begin light_complete<=1;light_last<=lk[6:2];end
"####,
        r####"     2'b01:published_count<=published_count-1'b1;
    endcase
    if(quad_accepted&&retire_slot==light_last)light_complete<=0;
    if(light_result&&ll)begin light_complete<=1;light_last<=lk[6:2];end
"####,
    ),
    (
        r####"    if(sample_result&&sl)begin sample_complete<=1;sample_last<=sample_address[6:2];end
    if(!active&&published_count!=0)begin
     active<=1;header<=descriptors[retire_slot];out_lane<=0;
    end
    if(active)case(state)
"####,
        r####"    if(sample_result&&sl)begin sample_complete<=1;sample_last<=sample_address[6:2];end
    if(!active&&published_count!=0)begin
     active<=1;header<=head_descriptor;out_lane<=0;
    end
    if(active)case(state)
"####,
    ),
    (
        r####"    if(lane_accept)begin out_lane<=out_lane+1'b1;color_paid<=0;depth_valid[out_lane]<=0;depth_reserved[out_lane]<=0;end
    if(quad_accepted)begin
     if(published_count>1)begin active<=1;header<=descriptors[retire_slot+5'd1];end
     else active<=0;
    end
"####,
        r####"    if(lane_accept)begin out_lane<=out_lane+1'b1;color_paid<=0;depth_valid[out_lane]<=0;depth_reserved[out_lane]<=0;end
    if(quad_accepted)begin
     if(published_count>1)begin active<=1;header<=head_descriptor;end
     else active<=0;
    end
"####,
    ),
    (
        r####" output wire basic_write,output wire[7:0] basic_write_address,
 output wire[23:0] basic_write_data,
 input wire basic_read,input wire[7:0] basic_read_address,
 output wire basic_return_valid,output wire[23:0] basic_return_data,
"####,
        r####" output wire basic_write,output wire[7:0] basic_write_address,
 output wire[23:0] basic_write_data,
 input wire[4:0]descriptor_issue_slot,descriptor_head_slot,output wire[22:0]descriptor_issue,descriptor_head,
 input wire basic_read,input wire[7:0] basic_read_address,
 output wire basic_return_valid,output wire[23:0] basic_return_data,
"####,
    ),
    (
        r####" .published(published),.published_slot(published_slot),.published_begin(published_begin),
 .basic_write(basic_write),.basic_write_address(basic_write_address),.basic_write_data(basic_write_data),
 .basic_read(basic_read),.basic_read_address(basic_read_address),
 .basic_return_valid(basic_return_valid),.basic_return_data(basic_return_data),.basic_collision(basic_collision),
"####,
        r####" .published(published),.published_slot(published_slot),.published_begin(published_begin),
 .basic_write(basic_write),.basic_write_address(basic_write_address),.basic_write_data(basic_write_data),
 .descriptor_issue_slot(descriptor_issue_slot),.descriptor_head_slot(descriptor_head_slot),.descriptor_issue(descriptor_issue),.descriptor_head(descriptor_head),
 .basic_read(basic_read),.basic_read_address(basic_read_address),
 .basic_return_valid(basic_return_valid),.basic_return_data(basic_return_data),.basic_collision(basic_collision),
"####,
    ),
    (
        r####" output wire [7:0] basic_write_address,
 output wire [23:0] basic_write_data,
 input wire basic_read,
 input wire [7:0] basic_read_address,
"####,
        r####" output wire [7:0] basic_write_address,
 output wire [23:0] basic_write_data,
 input wire[4:0]descriptor_issue_slot,descriptor_head_slot,output wire[22:0]descriptor_issue,descriptor_head,
 input wire basic_read,
 input wire [7:0] basic_read_address,
"####,
    ),
    (
        r####");
 reg [2:0] row;
 reg [22:0] held_begin;
 reg [5:0] alloc_ptr, publish_ptr, retire_ptr;
 reg [1:0] light_alloc, light_consume;
"####,
        r####");
 reg [2:0] row;
 // Canonical description is paid at live row0; publication owns visibility.
 (* syn_ramstyle="distributed_ram" *)reg[22:0]descriptors[0:31];
 reg[6:0]held_control;
 assign descriptor_issue=descriptors[descriptor_issue_slot];
 assign descriptor_head=descriptors[descriptor_head_slot];
 reg [5:0] alloc_ptr, publish_ptr, retire_ptr;
 reg [1:0] light_alloc, light_consume;
"####,
    ),
    (
        r####" (* syn_ramstyle="distributed_ram" *) reg [9:0] light_desc[0:1];
 (* syn_ramstyle="distributed_ram" *) reg [10:0] sample_desc[0:7];
 wire [22:0] current_begin = row == 0 ? in_begin : held_begin;
 wire live = |current_begin[3:0];
 wire lit = !current_begin[5];
"####,
        r####" (* syn_ramstyle="distributed_ram" *) reg [9:0] light_desc[0:1];
 (* syn_ramstyle="distributed_ram" *) reg [10:0] sample_desc[0:7];
 wire [22:0] current_begin = row == 0 ? in_begin : {16'd0,held_control};
 wire live = |current_begin[3:0];
 wire lit = !current_begin[5];
"####,
    ),
    (
        r####" assign published = push && live && row == 7;
 assign published_slot = slot;
 assign published_begin = current_begin;
 assign basic_write = push && live;
 assign basic_write_address = {slot,row};
"####,
        r####" assign published = push && live && row == 7;
 assign published_slot = slot;
 assign published_begin = row==0?in_begin:descriptors[slot];
 assign basic_write = push && live;
 assign basic_write_address = {slot,row};
"####,
    ),
    (
        r####" always @(posedge clk) begin
  if(reset) begin
   row<=0;held_begin<=0;alloc_ptr<=0;publish_ptr<=0;retire_ptr<=0;
   light_alloc<=0;light_consume<=0;sample_alloc<=0;sample_consume<=0;fault<=0;
  end else begin
"####,
        r####" always @(posedge clk) begin
  if(reset) begin
   row<=0;held_control<=0;alloc_ptr<=0;publish_ptr<=0;retire_ptr<=0;
   light_alloc<=0;light_consume<=0;sample_alloc<=0;sample_consume<=0;fault<=0;
  end else begin
"####,
    ),
    (
        r####"   if(push) begin
    if(row == 0) begin
     held_begin<=in_begin;
     if(live) begin
      alloc_ptr<=alloc_ptr+1'b1;
      if(lit) begin
"####,
        r####"   if(push) begin
    if(row == 0) begin
     held_control<=in_begin[6:0];
     if(live) begin
      descriptors[alloc_ptr[4:0]]<=in_begin;
      alloc_ptr<=alloc_ptr+1'b1;
      if(lit) begin
"####,
    ),
];
