//! Fixed-cadence ingress context transfers into ordered return context.
pub fn lower(mut source: String) -> Result<String, String> {
    source = source.replace("\r\n", "\n");
    let old_headers =
        "    reg [8:0]  s_hx [0:1];\n    reg [7:0]  s_hy [0:1];\n    reg [3:0]  s_hm [0:1];";
    let start = source
        .find("    // Incoming old operands are read only for an accepted first row/pair.")
        .ok_or("stream origin start")?;
    let end = start
        + source[start..]
            .find("    // Raw proposal is independent from its validation")
            .ok_or("stream origin end")?;
    source.replace_range(
        start..end,
        r#"    // On a row0 proposal, the previous row7 has been accepted and its first
    // result has already transferred the immutable origin to return context.
    wire owner_overlap = fill_row==0 && rop_active && !raw_retire
        && {row_x,row_y}=={return_x,return_y};
    assign source_ready = (fill_row!=0) || (!mt_active && hit && !owner_overlap);
"#,
    );
    let patches=[
        (old_headers,"    reg [8:0] work_x,return_x;\n    reg [7:0] work_y,return_y;\n    reg [3:0] work_mask,return_mask;"),
        ("    reg [2:0] rop_line;","    reg [2:0] ingress_line,return_line;\n    // A depth owner always wins, including key wrap after the last Color.\n    wire first_color_context;\n    wire [2:0] write_line=first_color_context?ingress_line:return_line;\n    wire [8:0] write_x=first_color_context?work_x:return_x;\n    wire [7:0] write_y=first_color_context?work_y:return_y;"),
        ("if(row_index==0) begin prep<=P_COLOR; rop_line<=hit_line; end","if(row_index==0) begin prep<=P_COLOR; ingress_line<=hit_line; end"),
        ("s_hx[tail] <= row_x;","work_x <= row_x;"),
        ("s_hy[tail] <= row_y;","work_y <= row_y;"),
        ("s_hm[tail] <= row_mask;","work_mask <= row_mask;"),
        ("if(quad_color_dirty)dirc[rop_line]<=1;","if(quad_color_dirty)dirc[return_line]<=1;"),
        ("if(quad_depth_dirty)dird[rop_line]<=1;","if(quad_depth_dirty)dird[return_line]<=1;"),
        ("commit_valid<=1;commit_x<=s_hx[head];","commit_valid<=1;commit_x<=return_x;"),
        ("commit_y<=s_hy[head];commit_mask<=s_hm[head];","commit_y<=return_y;commit_mask<=return_mask;"),
        ("                    if(return_fire)begin","                    if(return_fire)begin\n                        if(return_key==0)begin\n                            return_line<=ingress_line;return_x<=work_x;\n                            return_y<=work_y;return_mask<=work_mask;\n                        end"),
        ("row_index[2:1] ^ {s_hx[tail][1],1'b0}","row_index[2:1] ^ {work_x[1],1'b0}"),
        ("s_hm[tail][row_index[2:1]]","work_mask[row_index[2:1]]"),
        ("leaf_out_key ^ {s_hx[head][1],1'b0}","leaf_out_key ^ {write_x[1],1'b0}"),
        ("depth_key ^ {s_hx[head][1],1'b0}","depth_key ^ {return_x[1],1'b0}"),
        ("    // Two immutable headers are the only outstanding quad descriptors.\n    // The current tail is a private prefix; the head owns ordered returns.","    // Existing head/tail flags pay ordered credit, never select payload.\n    // Work owns accepted rows; the first Color transfers to return context."),
    ];
    for (old, new) in patches {
        if source.matches(old).count() != 1 {
            return Err(format!("Stream context anchor {old}"));
        }
        source = source.replacen(old, new, 1);
    }
    source=source.replacen("    wire        leaf_out_valid;", "    wire        leaf_out_valid;\n    assign first_color_context=leaf_out_valid&&(return_key==0)&&!depth_pending;", 1);
    let old = "(row0_fire ? hit_line : rop_line)";
    if source.matches(old).count() != 4 {
        return Err("Stream source address count".into());
    }
    source = source.replace(old, "(row0_fire ? hit_line : ingress_line)");
    let old = "{depth_fire,rop_line,s_hy[head][3:1],";
    if source.matches(old).count() != 4 {
        return Err("Stream result address count".into());
    }
    source = source.replace(old, "{depth_fire,write_line,write_y[3:1],");
    source = source
        .replace("s_hx[tail][1]", "work_x[1]")
        .replace("s_hx[head][1]", "write_x[1]")
        .replace("s_hx[head][3:2]", "write_x[3:2]");
    if ["s_hx", "s_hy", "s_hm", "rop_line"]
        .iter()
        .any(|s| source.contains(s))
    {
        return Err("Array-selected header remains".into());
    }
    Ok(source)
}

/// Independent FIFO of accepted lane tokens; not a work-to-return shift model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    pub slot: u8,
    pub line: u8,
    pub x: u16,
    pub y: u8,
    pub mask: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub context: Context,
    pub lane: u8,
}
#[derive(Default)]
pub struct TokenOracle {
    pub input: Option<Context>,
    pub next_row: u8,
    pub issued: std::collections::VecDeque<Token>,
    pub pending_depth: Option<Token>,
    pub pending_commit: Option<Context>,
}
impl TokenOracle {
    pub fn accept_row(&mut self, row: u8, context: Context) {
        assert_eq!(row, self.next_row);
        if row == 0 {
            self.input = Some(context);
        }
        assert_eq!(self.input, Some(context));
        if row & 1 != 0 {
            self.issued.push_back(Token {
                context,
                lane: row / 2,
            });
        }
        self.next_row = (self.next_row + 1) & 7;
    }
    pub fn color(&mut self, lane: u8) -> Token {
        assert!(self.pending_depth.is_none());
        let token = self.issued.pop_front().expect("unpaid Color result");
        assert_eq!(token.lane, lane);
        self.pending_depth = Some(token);
        token
    }
    pub fn depth(&mut self, lane: u8) -> Token {
        let token = self.pending_depth.take().expect("unpaid Depth result");
        assert_eq!(token.lane, lane);
        if lane == 3 {
            assert!(self.pending_commit.is_none());
            self.pending_commit = Some(token.context);
        }
        token
    }
}
