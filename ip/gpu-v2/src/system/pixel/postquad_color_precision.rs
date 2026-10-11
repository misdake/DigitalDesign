//! Static closed-producer Color precision experiment. Generic/default Color
//! remains checked and unchanged. The real Coefficient conserves weight511;
//! Membership/Packet partitions each nonzero tap once. Only that producer may
//! use this lowering: quantized guards do not detect every old raw-domain error.
//! Per-tap quantization is independent of Group splitting. RGB8 ports, calendar,
//! public IDs, capacities and twelve hard multipliers stay unchanged.
fn one(s: &mut String, old: &str, new: &str) {
    assert_eq!(s.matches(old).count(), 1, "Q9 anchor {old}");
    *s = s.replace(old, new);
}
fn floor(mut s: String) -> String {
    for(old,new)in [
        ("localparam [17:0] MAX_SUM = 18'd130305;","localparam [11:0] MAX_SUM = 12'd509;"),
        ("wire [18:0] part [0:2];","wire [10:0] part [0:2];"),
        ("reg [16:0] a_val [0:2];","reg [8:0] a_val [0:2];"),
        ("wire [16:0] prev [0:2];","wire [8:0] prev [0:2];"),
        ("wire [19:0] sumv [0:2];","wire [11:0] sumv [0:2];"),
        ("p_first ? 17'd0 : a_val[c]","p_first ? 9'd0 : a_val[c]"),
        ("// Exact nearest /511, N<=255*511. No division or quantization change.\n      wire [7:0] high = sumv[c][16:9];\n      wire [8:0] high_low = {1'b0,high} + {1'b0,sumv[c][7:0]};\n      assign normalized[c] = high + (sumv[c][8] | high_low[8]);","// Closed Q8.1: per-tap floor(product/256), conserved prefix <=509.\n      // RNE Q8.1 to displayed RGB8; check/round carry is separate from feedback9.\n      assign normalized[c] = sumv[c][8:1] + (sumv[c][0] & sumv[c][1]);"),
        ("a_val[j] <= sumv[j][16:0]","a_val[j] <= sumv[j][8:0]"),
        ("output wire [18:0] sum);","output wire [10:0] sum);"),
        ("wire [17:0] pair0={1'b0,p0}+{1'b0,p1};","wire [9:0] pair0={1'b0,p0[16:8]}+{1'b0,p1[16:8]};"),
        ("wire [17:0] pair1={1'b0,p2}+{1'b0,p3};","wire [9:0] pair1={1'b0,p2[16:8]}+{1'b0,p3[16:8]};"),
        ("wire [18:0] full_sum={1'b0,pair0}+{1'b0,pair1};","wire [10:0] full_sum={1'b0,pair0}+{1'b0,pair1};"),
        ("reg [18:0] q;","reg [10:0] q;"),
    ]{one(&mut s,old,new);}
    s
}
/// Independent visual alternative; still9-bit feedback, rounding is per tap.
fn nearest(mut s: String) -> String {
    one(
        &mut s,
        "localparam [11:0] MAX_SUM = 12'd509;",
        "localparam [11:0] MAX_SUM = 12'd511;",
    );
    one(&mut s,"wire [9:0] pair0={1'b0,p0[16:8]}+{1'b0,p1[16:8]};", "wire [8:0] q0=p0[16:8]+(p0[7]&&((|p0[6:0])||p0[8]));\n wire [8:0] q1=p1[16:8]+(p1[7]&&((|p1[6:0])||p1[8]));\n wire [8:0] q2=p2[16:8]+(p2[7]&&((|p2[6:0])||p2[8]));\n wire [8:0] q3=p3[16:8]+(p3[7]&&((|p3[6:0])||p3[8]));\n wire [9:0] pair0={1'b0,q0}+{1'b0,q1};");
    one(
        &mut s,
        "wire [9:0] pair1={1'b0,p2[16:8]}+{1'b0,p3[16:8]};",
        "wire [9:0] pair1={1'b0,q2}+{1'b0,q3};",
    );
    one(&mut s,"assign normalized[c] = sumv[c][8:1] + (sumv[c][0] & sumv[c][1]);","wire [8:0] rounded={1'b0,sumv[c][8:1]}+(sumv[c][0]&sumv[c][1]);\n      assign normalized[c]=rounded[8]?8'hff:rounded[7:0];");
    s
}

pub fn lower(source: String, round_taps: bool) -> String {
    // Normalize include_str line endings before matching qualified expressions.
    let source = floor(source.replace("\r\n", "\n"));
    if round_taps {
        nearest(source)
    } else {
        source
    }
}
/// Strict per-tap RNE9, regrouped into pair-local increments. The real unsigned
/// MULT9X9 inputs are weight9 and RGB8, so product <= 511*255 and a rounded tap
/// cannot overflow its original9-bit field. No group compensation is introduced.
pub fn nearest_pairs(mut source: String) -> Result<String, String> {
    for tap in 0..4 {
        let old =
            format!(" wire [8:0] q{tap}=p{tap}[16:8]+(p{tap}[7]&&((|p{tap}[6:0])||p{tap}[8]));\n");
        if source.matches(&old).count() != 1 {
            return Err(format!("RNE pair q anchor {tap}"));
        }
        source = source.replacen(&old, "", 1);
    }
    for pair in 0..2 {
        let a = pair * 2;
        let b = a + 1;
        let old = format!(" wire [9:0] pair{pair}={{1'b0,q{a}}}+{{1'b0,q{b}}};");
        let new = format!(" wire [1:0] bump{pair}={{1'b0,(p{a}[7]&&((|p{a}[6:0])||p{a}[8]))}}+{{1'b0,(p{b}[7]&&((|p{b}[6:0])||p{b}[8]))}};\n wire [9:0] pair{pair}={{1'b0,p{a}[16:8]}}+{{1'b0,p{b}[16:8]}}+{{8'b0,bump{pair}}};");
        if source.matches(&old).count() != 1 {
            return Err(format!("RNE pair sum anchor {pair}"));
        }
        source = source.replacen(&old, &new, 1);
    }
    Ok(source)
}

/// Project the already quantized taps with the actual conserved weight511.
/// For every legal Q8.1 sum N, round(N*256/511) is ceil(N/2); the rounded-tap
/// endpoint511 saturates to255. This cannot restore discarded tap precision.
pub fn normalize_511(mut source: String, round_taps: bool) -> String {
    if round_taps {
        one(
            &mut source,
            "wire [8:0] rounded={1'b0,sumv[c][8:1]}+(sumv[c][0]&sumv[c][1]);",
            "wire [8:0] rounded={1'b0,sumv[c][8:1]}+sumv[c][0];",
        );
    } else {
        one(
            &mut source,
            "assign normalized[c] = sumv[c][8:1] + (sumv[c][0] & sumv[c][1]);",
            "assign normalized[c] = sumv[c][8:1] + sumv[c][0];",
        );
    }
    source
}
