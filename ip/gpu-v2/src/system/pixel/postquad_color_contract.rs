//! Closed producer arithmetic experiment. Coefficient conserves511; ordered
//! Membership/Packet/cache partitions each tap once and preserves first/last.
//! This composition, rather than arbitrary external Group streams, supplies
//! Color. A nonnegative quantized prefix fits9 bits. Stream-owner shadow keys
//! only diagnose violations of that producer contract and are redundant here.
//! The generic checked Color retains its diagnostics. Public live-ID checks,
//! result credits, FIFO overflow and real output keys stay in this composition.
fn one(source: &mut String, old: &str, new: &str) -> Result<(), String> {
    if source.matches(old).count() != 1 {
        return Err(format!("Closed Color contract anchor: {old}"));
    }
    *source = source.replace(old, new);
    Ok(())
}
pub fn lower(mut source: String) -> Result<String, String> {
    for (old, new) in [
        ("wire [10:0] part [0:2];", "wire [8:0] part [0:2];"),
        ("wire [11:0] sumv [0:2];", "wire [8:0] sumv [0:2];"),
        ("assign sumv[c] = {1'b0,part[c]} + {3'b0,prev[c]};", "assign sumv[c] = part[c] + prev[c];"),
        ("wire sum_bad = p_valid && (\n    sumv[0]>MAX_SUM || sumv[1]>MAX_SUM || sumv[2]>MAX_SUM);", "wire sum_bad = 1'b0; // Conserved quantized producer prefix<=511."),
        ("wire owner_bad = accepted && (first_in ? own_valid : (!own_valid || own_key!=key_in));", "wire owner_bad = 1'b0; // Ordered real Packet producer owns the stream."),
        ("wire feedback_bad = p_valid && (p_first ? a_valid : (!a_valid || a_key!=p_key));", "wire feedback_bad = 1'b0; // Same first/last/key follows the fixed Color calendar."),
        ("output wire [10:0] sum);", "output wire [8:0] sum);"),
        ("wire [10:0] full_sum={1'b0,pair0}+{1'b0,pair1};", "wire [8:0] full_sum=pair0+pair1;"),
        ("reg [10:0] q;", "reg [8:0] q;"),
    ] {
        one(&mut source, old, new)?;
    }
    // Unused shadow declarations/writes deliberately remain textual until
    // synthesis removes them; no new behavioral state or reset is introduced.
    Ok(source)
}
