//! Reuse the frozen independent numerical oracle; only the physical driving
//! calendar and relative-address adapter differ from the legacy leaf test.
pub(super) fn special() -> String {
    let mut text = include_str!("cpu_v3_fpu_special_path_tb.v").to_string();
    for (before, after) in [
        (
            r###"wire [8:0] sf_read_b_address;
wire sf_write_enable;
wire [8:0] sf_write_address;
wire [31:0] sf_write_data;
wire busy;
"###,
            r###"wire [8:0] sf_read_b_address;
wire sf_write_enable;
wire [8:0] relative_write_address;
wire [8:0] sf_write_address={3'd0,word1_raw[15:10]}+relative_write_address;
wire [31:0] sf_write_data;
wire busy;
"###,
        ),
        (
            r###"
// Read address seen by the RF: the parked operand address before T0, the
// path's LUT address during T1 (RCP/RSQRT owns T0..T4). Readback
// for TB inspection uses the same port while idle.
reg [8:0] parked_a = 9'd0;
"###,
            r###"
// Read address seen by the RF: the parked operand address before T0, the
// path's LUT address during T1 (RCP/RSQRT owns T0..T2). Readback
// for TB inspection uses the same port while idle.
reg [8:0] parked_a = 9'd0;
"###,
        ),
        (
            r###"
// Shared-pipe stub: the real CpuV3FpuMulPipe lives in the unit top; this
// TB-local copy reproduces its exact signed-36 2-stage tag-carrying behavior
// so the leaf test needs no resource-claiming sibling modules (same pattern as
// the multiply-path TB).
"###,
            r###"
// Shared-pipe stub: the real CpuV3FpuMulPipe lives in the unit top; this
// TB-local copy reproduces its exact signed-36 1-stage tag-carrying behavior
// so the leaf test needs no resource-claiming sibling modules (same pattern as
// the multiply-path TB).
"###,
        ),
        (
            r###"wire [8:0] mul_out_tag;

reg mul_s1_valid = 0;
reg signed [35:0] mul_s1_a = 0;
reg signed [35:0] mul_s1_b = 0;
reg [8:0] mul_s1_tag = 0;
reg mul_s2_valid = 0;
reg signed [71:0] mul_s2_prod = 0;
reg [8:0] mul_s2_tag = 0;
always @(posedge clk) begin
    if (abort) begin
        mul_s1_valid <= 0; mul_s2_valid <= 0;
    end else begin
        mul_s2_valid <= mul_s1_valid;
        if (mul_s1_valid) begin mul_s2_prod <= mul_s1_a * mul_s1_b; mul_s2_tag <= mul_s1_tag; end
        mul_s1_valid <= sf_mul_in_valid;
        if (sf_mul_in_valid) begin
            mul_s1_a <= sf_mul_in_a; mul_s1_b <= sf_mul_in_b;
            mul_s1_tag <= sf_mul_in_tag;
        end
    end
end
assign mul_out_valid = mul_s2_valid && !abort;
assign mul_out_product = mul_s2_prod[63:0];
assign mul_out_tag = mul_s2_tag;

CpuV3FpuSpecialPath special_path (
    .clk(clk),
    .abort(abort),
"###,
            r###"wire [8:0] mul_out_tag;

reg mul_valid_r=0;
reg signed [71:0] mul_product_r=0;
always @(posedge clk)begin
 if(abort)mul_valid_r<=0;
 else begin
  mul_valid_r<=sf_mul_in_valid;
  if(sf_mul_in_valid)mul_product_r<=sf_mul_in_a*sf_mul_in_b;
 end
end
assign mul_out_valid=mul_valid_r&&!abort;
assign mul_out_product=mul_product_r[63:0];
assign mul_out_tag=0;

CpuV3FpuExecSpecial special_path (
    .clk(clk),
    .abort(abort),
"###,
        ),
        (
            r###"    .rf_read_b_address(sf_read_b_address),
    .rf_write_enable(sf_write_enable),
    .rf_write_address(sf_write_address),
    .rf_write_data(sf_write_data),
    .busy(busy),
"###,
            r###"    .rf_read_b_address(sf_read_b_address),
    .rf_write_enable(sf_write_enable),
    .rf_write_address(relative_write_address),
    .rf_write_data(sf_write_data),
    .busy(busy),
"###,
        ),
        (
            r###"endtask

// Issues one RCP/RSQRT and checks the T0..T4 window and the written
// word against `expected`.
task run_special;
"###,
            r###"endtask

// Issues one RCP/RSQRT and checks the T0..T2 window and the written
// word against `expected`.
task run_special;
"###,
        ),
        (
            r###"        #1;
        check_value({31'b0, busy}, 32'h1, "T0 busy");
        check_value({31'b0, sf_w_wait}, 32'h5, "T0 w_wait");
        check_value({31'b0, sf_x_wait}, 32'h5, "T0 x_wait");
        check_value({31'b0, sf_r_wait}, 32'h0, "T0 r_wait");
        check_value({31'b0, sf_write_enable}, 32'h0, "T0 write idle");
"###,
            r###"        #1;
        check_value({31'b0, busy}, 32'h1, "T0 busy");
        check_value({31'b0, sf_w_wait}, 32'h3, "T0 w_wait");
        check_value({31'b0, sf_x_wait}, 32'h3, "T0 x_wait");
        check_value({31'b0, sf_r_wait}, 32'h0, "T0 r_wait");
        check_value({31'b0, sf_write_enable}, 32'h0, "T0 write idle");
"###,
        ),
        (
            r###"        t = 1;
        while (busy) begin
            if (t == 4) begin
                check_value({31'b0, sf_write_enable}, 32'h1, "T4 write enable");
                check_value({23'b0, sf_write_address}, fd, "T4 write address");
                check_value(sf_write_data, expected, label);
            end else begin
"###,
            r###"        t = 1;
        while (busy) begin
            if (t == 2) begin
                check_value({31'b0, sf_write_enable}, 32'h1, "T2 write enable");
                check_value({23'b0, sf_write_address}, fd, "T2 write address");
                check_value(sf_write_data, expected, label);
            end else begin
"###,
        ),
        (
            r###"            #1;
        end
        check_value(busy_count, 5, "busy window length");
        check_value({31'b0, busy}, 32'h0, "busy clear");
        check_value({31'b0, sf_write_enable}, 32'h0, "write clear");
"###,
            r###"            #1;
        end
        check_value(busy_count, 3, "busy window length");
        check_value({31'b0, busy}, 32'h0, "busy clear");
        check_value({31'b0, sf_write_enable}, 32'h0, "write clear");
"###,
        ),
        (
            r###"        instr_opcode = 4'hD;
        word1_raw = {fd, SINCOS_SUBOP, 2'b00, mode};
        expected_cycles = ((mode == 2'b01) || (mode == 2'b10)) ? 7 : 8;
        #1;
        check_value({31'b0, busy}, 32'h1, "sincos T0 busy");
"###,
            r###"        instr_opcode = 4'hD;
        word1_raw = {fd, SINCOS_SUBOP, 2'b00, mode};
        expected_cycles = ((mode == 2'b01) || (mode == 2'b10)) ? 4 : 5;
        #1;
        check_value({31'b0, busy}, 32'h1, "sincos T0 busy");
"###,
        ),
        (
            r###"            // Assert the new physical calendar independently of DUT stage.
            check_value({31'b0,sf_write_enable},
                (t==6 || (mode==0 && t==7)), "sincos exact RF write edge");
            check_value({31'b0,sf_mul_in_valid},
                (t==4 || (mode==0 && t==5)), "sincos exact DSP issue edge");
            if(t==3 || (mode==0 && t==4))
                check_value({31'b0,sf_read_a_address[8]},1,"sincos synchronous LUT edge");
            else
"###,
            r###"            // Assert the new physical calendar independently of DUT stage.
            check_value({31'b0,sf_write_enable},
                (t==3 || (mode==0 && t==4)), "sincos exact RF write edge");
            check_value({31'b0,sf_mul_in_valid},
                (t==2 || (mode==0 && t==3)), "sincos exact DSP issue edge");
            if(t==1 || (mode==0 && t==2))
                check_value({31'b0,sf_read_a_address[8]},1,"sincos synchronous LUT edge");
            else
"###,
        ),
        (
            r###"        #1;
        check_value(published_writes - initial_writes,
            (abort_age > 6) + (abort_age > 7), "abort keeps only already published results");
        abort = 1'b0;
        @(negedge clk);
"###,
            r###"        #1;
        check_value(published_writes - initial_writes,
            (abort_age > 3) + (abort_age > 4), "abort keeps only already published results");
        abort = 1'b0;
        @(negedge clk);
"###,
        ),
        (
            r###"        parked_a=0;
        @(negedge clk);
        last_write=mode==0 ? 7 : 6;
        for(transaction=0;transaction<2;transaction=transaction+1) begin
            destination=8+transaction*2;
"###,
            r###"        parked_a=0;
        @(negedge clk);
        last_write=mode==0 ? 4 : 3;
        for(transaction=0;transaction<2;transaction=transaction+1) begin
            destination=8+transaction*2;
"###,
        ),
        (
            r###"                check_value({31'b0,busy},age<=last_write,"adjacent exact busy boundary");
                check_value({31'b0,sf_write_enable},
                    age==6 || (mode==0 && age==7),"adjacent exact write boundary");
                if(age==6) begin
                    check_value({23'b0,sf_write_address},destination,"adjacent first address");
                    check_value(sf_write_data,mode==2 ? sincos_cos_ref(a) : sincos_sin_ref(a),"adjacent first value");
                end
                if(mode==0 && age==7) begin
                    check_value({23'b0,sf_write_address},destination+1,"adjacent second address");
                    check_value(sf_write_data,sincos_cos_ref(a),"adjacent second value");
"###,
            r###"                check_value({31'b0,busy},age<=last_write,"adjacent exact busy boundary");
                check_value({31'b0,sf_write_enable},
                    age==3 || (mode==0 && age==4),"adjacent exact write boundary");
                if(age==3) begin
                    check_value({23'b0,sf_write_address},destination,"adjacent first address");
                    check_value(sf_write_data,mode==2 ? sincos_cos_ref(a) : sincos_sin_ref(a),"adjacent first value");
                end
                if(mode==0 && age==4) begin
                    check_value({23'b0,sf_write_address},destination+1,"adjacent second address");
                    check_value(sf_write_data,sincos_cos_ref(a),"adjacent second value");
"###,
        ),
    ] {
        assert_eq!(
            text.matches(before).count(),
            1,
            "special test calendar anchor"
        );
        text = text.replacen(before, after, 1);
    }
    text
}
