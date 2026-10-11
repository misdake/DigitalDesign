//! Exact narrow RGB565 quantization, retaining the original ROP calendar.
//! Applies independently to generic or closed-cache sources. It changes only
//! stage-six arithmetic and stage-seven extraction, not visible lane semantics.
pub fn lower(mut source: String) -> String {
    let functions = r#"    function [4:0] exactq5;
        input [7:0] c;
        reg [4:0] head;
        reg [2:0] low;
        reg down,up;
        begin
            head=c[7:3];low=c[2:0];
            down=((low==3'd0)&&(head>=5'd19))||((low==3'd1)&&(head>=5'd23))||((low==3'd2)&&(head>=5'd28));
            up=((low==3'd5)&&(head<=5'd3))||((low==3'd6)&&(head<=5'd8))||((low==3'd7)&&(head<=5'd12));
            exactq5=head+{4'd0,up}-{4'd0,down};
        end
    endfunction

    function [5:0] exactq6;
        input [7:0] c;
        reg [5:0] head;
        reg [1:0] low;
        reg down,up;
        begin
            head=c[7:2];low=c[1:0];
            down=(low==2'd0)&&(head>=6'd43);
            up=(low==2'd3)&&(head<=6'd20);
            exactq6=head+{5'd0,up}-{5'd0,down};
        end
    endfunction

"#;
    replace_once(
        &mut source,
        "    function [8:0] div255;",
        &format!("{functions}    function [8:0] div255;"),
    );
    for (old, new) in [
        (
            "qmr6 <= ({9'd0, br5} << 5) - {9'd0, br5} + 17'd127;",
            "qmr6 <= {12'd0,exactq5(br5)};",
        ),
        (
            "qmg6 <= ({9'd0, bg5} << 6) - {9'd0, bg5} + 17'd127;",
            "qmg6 <= {11'd0,exactq6(bg5)};",
        ),
        (
            "qmb6 <= ({9'd0, bb5} << 5) - {9'd0, bb5} + 17'd127;",
            "qmb6 <= {12'd0,exactq5(bb5)};",
        ),
        (
            "wire [8:0] div_qr = div255(qmr6);",
            "wire [8:0] div_qr = {4'd0,qmr6[4:0]};",
        ),
        (
            "wire [8:0] div_qg = div255(qmg6);",
            "wire [8:0] div_qg = {3'd0,qmg6[5:0]};",
        ),
        (
            "wire [8:0] div_qb = div255(qmb6);",
            "wire [8:0] div_qb = {4'd0,qmb6[4:0]};",
        ),
    ] {
        replace_once(&mut source, old, new);
    }
    source
}

fn replace_once(source: &mut String, anchor: &str, replacement: &str) {
    assert_eq!(
        source.matches(anchor).count(),
        1,
        "ROP quantization anchor: {anchor}"
    );
    *source = source.replacen(anchor, replacement, 1);
}
