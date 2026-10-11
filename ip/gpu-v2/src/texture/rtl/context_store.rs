//! Explicit fixed-bank port controller and a freshly lowered union reference.
//! Ordered heads/counts/type/next/reclaim remain in Runtime. Their combinational
//! validation enters control_bad; advance gates their common commit edge.
pub fn source(capacity: usize, native: bool, union_reference: bool) -> Result<String, String> {
    source_with_lod_ready(capacity, native, union_reference, false)
}

/// Runtime-only Raw/Ready SRAM. `held_derived` must be wired to D's actual
/// terminal bank outputs regardless of out_valid; loan_owner is old d_head-1.
/// The final identity read precedes the same-edge next D acceptance at phase0.
pub fn runtime_source(capacity: usize, native: bool) -> Result<String, String> {
    let mut source = source_with_lod_ready(capacity, native, false, true)?;
    source = source.replace(
        "input wire[52:0] derived_data,",
        "input wire[52:0] derived_data,held_derived,\n input wire[3:0] loan_owner,",
    );
    source = source.replace(
        "control_bad||address_bad",
        "control_bad||(lr && (loan_owner!=derived_ra || loan_owner>=CAP))||address_bad",
    );
    source = source.replace(
        "assign writes={l_return,d_return,admit}",
        "assign writes={l_return,1'b0,admit}",
    );
    let start = source.find(" generate if(UNION)").unwrap();
    let end = source.find(" end else if(NATIVE) begin:sram").unwrap();
    source.replace_range(start..end, " generate if(NATIVE) begin:sram");
    source = source.replacen(
        " generate if(NATIVE) begin:sram end else if(NATIVE) begin:sram",
        " generate if(NATIVE) begin:sram",
        1,
    );
    let start = source.find("   for(i=0;i<14;").unwrap();
    let end = source[start..].find("   if(LOD_READY)").unwrap() + start;
    source.replace_range(start..end, "   assign derived_q={3'b0,held_derived};\n");
    source = source.replace(
        "reg[35:0] raw_ram[0:15];reg[55:0] derived_ram[0:15];",
        "reg[35:0] raw_ram[0:15];",
    );
    source = source.replace(
        "     if(writes[1]) derived_ram[d_owner]<=padded_derived;\n",
        "",
    );
    source = source.replace(
        "assign derived_q=derived_ram[derived_ra];",
        "assign derived_q={3'b0,held_derived};",
    );
    Ok(source)
}

/// General bank controller with raw proposals independent of joined commit.
pub fn source_shared(
    capacity: usize,
    native: bool,
    union_reference: bool,
) -> Result<String, String> {
    shared(source(capacity, native, union_reference)?)
}

/// Selected Raw/Ready Runtime banks and actual D-terminal loan. `admit` writes
/// canonical Raw at UV row zero; Context frontend admission is a separate
/// row-three proposal. There is no additional metadata copy or read port.
pub fn runtime_source_shared(capacity: usize, native: bool) -> Result<String, String> {
    shared(runtime_source(capacity, native)?)
}

fn shared(source: String) -> Result<String, String> {
    if !source.contains("input wire clk,reset,ce,fault,control_bad,")
        || !source.contains("assign advance=!reset && ce && !fault && !bad;")
    {
        return Err("context shared-preflight template changed".into());
    }
    let proposal_writes = if source.contains("assign writes={l_return,1'b0,admit}") {
        "{l_return,1'b0,admit}"
    } else {
        "{l_return,d_return,admit}"
    };
    Ok(source
        .replace("module gpu_v2_context_store ", "module gpu_v2_context_store_shared ")
        .replace("input wire clk,reset,ce,fault,control_bad,", "input wire clk,reset,proposal_ce,commit_allow,fault,control_bad,")
        .replace("output wire bad,advance,", "output wire bad,preflight_bad,advance,\n output wire[2:0]proposal_reads,proposal_writes,")
        .replace("assign bad=!reset && ce", "assign bad=!reset && proposal_ce")
        .replace("assign advance=!reset && ce && !fault && !bad;", &format!("assign advance=!reset && proposal_ce && !fault && !bad && commit_allow;\n assign preflight_bad=bad;\n assign proposal_reads={{c_read,lr,rr}}&{{3{{!reset && proposal_ce && !fault}}}};\n assign proposal_writes={proposal_writes}&{{3{{!reset && proposal_ce && !fault}}}};")))
}

/// Runtime uses canonical decision26; independent general callers keep Ready83.
pub fn source_with_lod_ready(
    capacity: usize,
    native: bool,
    union_reference: bool,
    lod_ready: bool,
) -> Result<String, String> {
    if !(1..=8).contains(&capacity) {
        return Err("context RTL capacity".into());
    }
    Ok(format!(
        "module gpu_v2_context_store #(parameter CAP={capacity}, NATIVE={}, UNION={}, LOD_READY={})(\n",
        usize::from(native),
        usize::from(union_reference),
        usize::from(lod_ready)
    ) + &BODY.replace("// READY_CAPTURE_DECODE", &ready_decode("capture", "ready_code"))
        .replace("// READY_READ_DECODE", &ready_decode("read", "decision_q")))
}

/// Pure decode wiring; subtraction creates an18-bit two's-complement shift.
/// The signed result is assigned separately from all conditional expressions.
pub(crate) fn ready_decode(prefix: &str, code: &str) -> String {
    format!(
        r#"wire[3:0] {prefix}_n0={code}[3:0];
 wire[3:0] {prefix}_n1=({prefix}_n0==0)?4'd0:{prefix}_n0-4'd1;
 wire[3:0] {prefix}_physical0=({prefix}_n0<1)?4'd1:{prefix}_n0;
 wire[3:0] {prefix}_physical1=({prefix}_n1<1)?4'd1:{prefix}_n1;
 wire signed[17:0] {prefix}_shift={{14'd0,{prefix}_physical0}}-18'd8;
 wire[11:0] {prefix}_side0=12'd1<<{prefix}_physical0;
 wire[11:0] {prefix}_side1=12'd1<<{prefix}_physical1;
 wire[8:0] {prefix}_parent1={code}[13:5],{prefix}_parent0=9'd511-{prefix}_parent1;
 wire[82:0] {prefix}_context={{{code}[25:14],({prefix}_parent1==0),{prefix}_n1,{prefix}_n0,{prefix}_parent1,{prefix}_parent0,{prefix}_side1,{prefix}_side0,({prefix}_n0>1),{code}[4],{prefix}_shift}};
"#
    )
}
const BODY: &str = r#"input wire clk,reset,ce,fault,control_bad,
 input wire[2:0] phase,
 input wire admit,d_issue,l_issue,c_read,d_return,l_return,
 input wire[3:0] allocate,d_head,l_head,c_head,d_owner,l_owner,
 input wire[35:0] raw_data,
 input wire[52:0] derived_data,
 input wire[82:0] ready_data,
 output wire bad,advance,
 output wire[2:0] writes,reads,
 output wire[3:0] raw_ra,derived_ra,ready_ra,
 output wire[35:0] raw_q,
 output wire[55:0] derived_q,
 output wire[83:0] ready_q);
 assign raw_ra=d_return?d_owner:d_head;
 assign derived_ra=l_return?l_owner:l_head;
 assign ready_ra=c_head;
 wire rr=d_issue||d_return,lr=l_issue||l_return;
 wire address_bad=(admit && allocate>=CAP)||(rr && raw_ra>=CAP)||
   (lr && derived_ra>=CAP)||(c_read && ready_ra>=CAP)||
   (d_return && d_owner>=CAP)||(l_return && l_owner>=CAP);
 wire phase_bad=(d_issue && phase!=0)||(d_return && phase!=3)||
   (l_issue && phase!=4)||(l_return && phase!=0);
 wire collision_bad=(admit && rr && allocate==raw_ra)||
   (d_return && lr && d_owner==derived_ra)||(l_return && c_read && l_owner==ready_ra);
 wire identity_bad=(d_return && raw_q[20:17]!=derived_data[37:34])||
   (l_return && derived_q[37:34]!=ready_data[74:71]);
 wire signed[15:0] raw_bias=raw_data[15:0],derived_bias=derived_data[33:18];
 // Signed checks stand alone; no unsigned conditional can reinterpret them.
 wire raw_bias_bad=raw_bias < -16'sd8192 || raw_bias > 16'sd8192;
 wire derived_bias_bad=derived_bias < -16'sd8192 || derived_bias > 16'sd8192;
 wire[25:0] ready_code={ready_data[82:71],ready_data[61:53],ready_data[18],ready_data[65:62]};
 // READY_CAPTURE_DECODE
 wire range_bad=(admit && (raw_bias_bad || raw_data[32:29]>10 || raw_data[35:34]>2))||
   (d_return && (derived_bias_bad || derived_data[49:46]>10 || derived_data[52:51]>2))||
   (LOD_READY && l_return && (ready_code[3:0]>10 || ready_data!=capture_context));
 assign bad=!reset && ce && !fault && (control_bad||address_bad||phase_bad||collision_bad||identity_bad||range_bad);
 assign advance=!reset && ce && !fault && !bad;
 assign writes={l_return,d_return,admit}&{3{advance}};
 assign reads={c_read,lr,rr}&{3{advance}};
 wire[55:0] padded_derived={3'b000,derived_data};
 wire[83:0] padded_ready={1'b0,ready_data};
 wire[27:0] decision_data={2'b0,ready_code};
 generate if(UNION) begin:reference_union
   // Faithful newly lowered Rust tagged union payload. This is a comparison
   // model, not a claim that an existing complete Runtime RTL was synthesized.
   reg[82:0] row[0:CAP-1];
   genvar i;
   for(i=0;i<CAP;i=i+1) begin:word
     always @(posedge clk) begin
       if(writes[0] && allocate==i) row[i]<={47'd0,raw_data};
       if(writes[1] && d_owner==i) row[i]<={30'd0,derived_data};
       if(writes[2] && l_owner==i) row[i]<=ready_data;
     end
   end
   assign raw_q=row[raw_ra][35:0];
   assign derived_q={3'b0,row[derived_ra][52:0]};
   assign ready_q={1'b0,row[ready_ra]};
 end else if(NATIVE) begin:sram
   genvar i;
   for(i=0;i<9;i=i+1) begin:raw_bank
     RAM16SDP4 ram(.CLK(clk),.WRE(writes[0]),.WAD(allocate),.RAD(raw_ra),.DI(raw_data[i*4+:4]),.DO(raw_q[i*4+:4]));
   end
   for(i=0;i<14;i=i+1) begin:derived_bank
     RAM16SDP4 ram(.CLK(clk),.WRE(writes[1]),.WAD(d_owner),.RAD(derived_ra),.DI(padded_derived[i*4+:4]),.DO(derived_q[i*4+:4]));
   end
   if(LOD_READY)begin:compact_ready
     wire[27:0] decision_q;
     for(i=0;i<7;i=i+1)begin:bank
       RAM16SDP4 ram(.CLK(clk),.WRE(writes[2]),.WAD(l_owner),.RAD(ready_ra),.DI(decision_data[i*4+:4]),.DO(decision_q[i*4+:4]));
     end
     // READY_READ_DECODE
     assign ready_q={1'b0,read_context};
   end else begin:full_ready
   for(i=0;i<21;i=i+1) begin:ready_bank
     RAM16SDP4 ram(.CLK(clk),.WRE(writes[2]),.WAD(l_owner),.RAD(ready_ra),.DI(padded_ready[i*4+:4]),.DO(ready_q[i*4+:4]));
   end
   end
 end else begin:portable
   reg[35:0] raw_ram[0:15];reg[55:0] derived_ram[0:15];
   always @(posedge clk) begin
     if(writes[0]) raw_ram[allocate]<=raw_data;
     if(writes[1]) derived_ram[d_owner]<=padded_derived;
   end
   assign raw_q=raw_ram[raw_ra];assign derived_q=derived_ram[derived_ra];
   if(LOD_READY)begin:compact_ready
     reg[25:0] ready_ram[0:15];wire[25:0] decision_q=ready_ram[ready_ra];
     always @(posedge clk)if(writes[2])ready_ram[l_owner]<=ready_code;
     // READY_READ_DECODE
     assign ready_q={1'b0,read_context};
   end else begin:full_ready
     reg[83:0] ready_ram[0:15];
     always @(posedge clk)if(writes[2])ready_ram[l_owner]<=padded_ready;
     assign ready_q=ready_ram[ready_ra];
   end
 end endgenerate
endmodule
"#;
