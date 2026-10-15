//! Closed weight511 producer: integer RGB8 tap sums rather than Q8.1.
//! Each positive tap rounds to at most floor(weight/2), because RGB<=255.
//! Thus every partitioned prefix is <=floor(511/2)=255, independent of
//! group/tile splitting. Zero taps stay zero; CE, latency and owners unchanged.
pub fn lower(mut source: String) -> String {
    let mut one = |old: &str, new: &str| {
        assert_eq!(source.matches(old).count(), 1, "Filter8 anchor {old}");
        source = source.replace(old, new);
    };
    one("wire [8:0] part [0:2];", "wire [7:0] part [0:2];");
    one("reg [8:0] a_val [0:2];", "reg [7:0] a_val [0:2];");
    one("wire [8:0] prev [0:2];", "wire [7:0] prev [0:2];");
    one("wire [8:0] sumv [0:2];", "wire [7:0] sumv [0:2];");
    one("p_first ? 9'd0 : a_val[c]", "p_first ? 8'd0 : a_val[c]");
    one("wire [8:0] rounded={1'b0,sumv[c][8:1]}+sumv[c][0];\n      assign normalized[c]=rounded[8]?8'hff:rounded[7:0];", "// Integer tap sums are bounded by255; final output needs no rounding.\n      assign normalized[c]=sumv[c];");
    one("a_val[j] <= sumv[j][8:0]", "a_val[j] <= sumv[j][7:0]");
    one("output wire [8:0] sum);", "output wire [7:0] sum);");
    for pair in 0..2 {
        let a = pair * 2;
        let b = a + 1;
        let old = format!(" wire [1:0] bump{pair}={{1'b0,(p{a}[7]&&((|p{a}[6:0])||p{a}[8]))}}+{{1'b0,(p{b}[7]&&((|p{b}[6:0])||p{b}[8]))}};\n wire [9:0] pair{pair}={{1'b0,p{a}[16:8]}}+{{1'b0,p{b}[16:8]}}+{{8'b0,bump{pair}}};");
        let new = format!(
            " wire [7:0] pair{pair}=p{a}[16:9]+p{b}[16:9]+{{7'b0,p{a}[8]}}+{{7'b0,p{b}[8]}};"
        );
        one(&old, &new);
    }
    one(
        "wire [8:0] full_sum=pair0+pair1;",
        "wire [7:0] full_sum=pair0+pair1;",
    );
    one("reg [8:0] q;", "reg [7:0] q;");
    source
}
