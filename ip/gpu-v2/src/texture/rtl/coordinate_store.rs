//! Same ordered control for FF baseline and split asynchronous SRAM candidate.
pub fn source(capacity: usize, native: bool) -> Result<String, String> {
    if !(1..=16).contains(&capacity) {
        return Err("Coordinate store RTL capacity".into());
    }
    Ok(format!(
        "module gpu_v2_coordinate_store #(parameter CAP={capacity}, NATIVE={})(\n",
        usize::from(native)
    ) + BODY)
}
const BODY: &str = r#" input wire clk,reset,ce,
 input wire issue,returned,take,
 input wire [37:0] owner_data,
 input wire [111:0] result_data,
 output wire in_ready,out_valid,bad,
 output wire [37:0] out_owner,
 output wire [111:0] out_result,
 output wire owner_write,result_write,consume,
 output wire [3:0] write_row,return_row,read_row,
 output wire [4:0] occupied,completed);
 localparam A=CAP>1?$clog2(CAP):1;
 localparam Q=$clog2(CAP+1);
 reg[A-1:0] wp,rp,cp;
 reg[Q-1:0] count,ready;
 assign in_ready=!reset && count<CAP;
 assign out_valid=!reset && ready!=0;
 assign bad=!reset && ce && ((issue && count==CAP) || (returned && count==ready) || (take && ready==0));
 assign owner_write=!reset && ce && !bad && issue;
 assign result_write=!reset && ce && !bad && returned;
 assign consume=!reset && ce && !bad && take;
 assign write_row=wp;assign return_row=rp;assign read_row=cp;
 assign occupied=count;assign completed=ready;
 generate if(NATIVE) begin:sram
   wire[39:0] padded_owner={2'b00,owner_data};
   wire[39:0] owner_read;
   wire[3:0] wa=wp,ra=rp,ca=cp;
   genvar i;
   for(i=0;i<10;i=i+1) begin:owner_bank
     RAM16SDP4 ram(.CLK(clk),.WRE(owner_write),.WAD(wa),.RAD(ca),
       .DI(padded_owner[i*4+:4]),.DO(owner_read[i*4+:4]));
   end
   for(i=0;i<28;i=i+1) begin:result_bank
     RAM16SDP4 ram(.CLK(clk),.WRE(result_write),.WAD(ra),.RAD(ca),
       .DI(result_data[i*4+:4]),.DO(out_result[i*4+:4]));
   end
   assign out_owner=owner_read[37:0];
 end else begin:ff
   (* syn_ramstyle="registers" *) reg[37:0] owners[0:CAP-1];
   (* syn_ramstyle="registers" *) reg[111:0] results[0:CAP-1];
   genvar i;
   for(i=0;i<CAP;i=i+1) begin:row
     always @(posedge clk) begin
       if(owner_write && wp==i) owners[i]<=owner_data;
       if(result_write && rp==i) results[i]<=result_data;
     end
   end
   assign out_owner=owners[cp];assign out_result=results[cp];
 end endgenerate
 always @(posedge clk) begin
   if(reset) begin wp<=0;rp<=0;cp<=0;count<=0;ready<=0;end
   else if(ce && !bad) begin
     if(issue) begin if(wp==CAP-1) wp<=0;else wp<=wp+1'b1;end
     if(returned) begin if(rp==CAP-1) rp<=0;else rp<=rp+1'b1;end
     if(take) begin if(cp==CAP-1) cp<=0;else cp<=cp+1'b1;end
     case({issue,take}) 2'b10:count<=count+1'b1;2'b01:count<=count-1'b1;default:count<=count;endcase
     case({returned,take}) 2'b10:ready<=ready+1'b1;2'b01:ready<=ready-1'b1;default:ready<=ready;endcase
   end
 end
endmodule
"#;

/// Selected fine-only result RAM. Generic 112-bit source remains unchanged.
pub fn compact_source(capacity: usize, native: bool) -> Result<String, String> {
    Ok(source(capacity, native)?
        .replace("[111:0]", "[35:0]")
        .replace("i<28", "i<9"))
}

/// Closed LOD owner24 storage with the unchanged wide observation interface.
/// Generic owner38 sources remain independent references. The parent may split
/// proposal CE from commit CE using the same template anchors as compact_source.
pub fn canonical_source(capacity: usize, native: bool) -> Result<String, String> {
    canonical_source_with_contract(capacity, native, true)
}
/// Static closed-producer variant: the actual selected LOD constructs the
/// omitted fields, while all old-state queue/return/consume guards remain.
pub fn canonical_source_trusted(capacity: usize, native: bool) -> Result<String, String> {
    canonical_source_with_contract(capacity, native, false)
}
fn canonical_source_with_contract(
    capacity: usize,
    native: bool,
    checked: bool,
) -> Result<String, String> {
    let source = compact_source(capacity, native)?
        .replace("localparam A=", r#"wire [23:0] canonical_in={owner_data[36:31],owner_data[30:27],owner_data[22:19],owner_data[18],owner_data[17:9]};
 wire [23:0] canonical_out;
 wire [3:0] n=canonical_out[13:10];
 wire [3:0] n1=(n==0)?4'd0:n-4'd1;
 assign out_owner={canonical_out[8:0]==0,canonical_out[23:18],canonical_out[17:14],n1,n,canonical_out[9],canonical_out[8:0],~canonical_out[8:0]};
 wire owner_bad=issue && ((owner_data[8:0]!=~owner_data[17:9]) || owner_data[22:19]>10 ||
   owner_data[26:23]!=((owner_data[22:19]==0)?4'd0:owner_data[22:19]-4'd1) ||
   owner_data[37]!=(owner_data[17:9]==0));
 localparam A="#)
        .replace("(take && ready==0));", "(take && ready==0) || owner_bad);")
        .replace("wire[39:0] padded_owner={2'b00,owner_data};", "wire[23:0] padded_owner=canonical_in;")
        .replace("wire[39:0] owner_read;", "wire[23:0] owner_read;")
        .replace("i<10", "i<6")
        .replace("assign out_owner=owner_read[37:0];", "assign canonical_out=owner_read;")
        .replace("reg[37:0] owners", "reg[23:0] owners")
        .replace("owners[i]<=owner_data;", "owners[i]<=canonical_in;")
        .replace("assign out_owner=owners[cp];", "assign canonical_out=owners[cp];");
    Ok(if checked {
        source
    } else {
        source.replace(" || owner_bad);", ");")
    })
}
