// Owned bounded mixed-owner/latency probe; architectural oracle is unchanged.
use super::*;
#[test]
#[ignore = "bounded actual core shared-wide owner/latency replay"]
fn shared_wide_mixed_owner_latency() {
    let mut p = Vec::new();
    p.extend(cpu_v3::load_immediate16(7, 0));
    for f in 0..4u8 {
        p.extend(cpu_v3::load_immediate16(0, 0x8000));
        p.extend(cpu_v3::load_immediate16(1, u16::from(f) + 1));
        p.extend(cpu_v3::fpu_aux_raw(
            cpu_v3::FpuAuxKind::IntegerRegister,
            0,
            0,
            f,
            2,
            0,
        ));
        p.extend(cpu_v3::fpu_aux_raw(
            cpu_v3::FpuAuxKind::IntegerRegister,
            1,
            0,
            f,
            3,
            0,
        ));
    }
    for &x in &[0u16, 1, 0xffff, 0x8000, 0x8001, 0x7fff, 0x00ff, 0xff00] {
        for &window in &[
            cpu_v3::MultiplyWindow::Low,
            cpu_v3::MultiplyWindow::Shift8,
            cpu_v3::MultiplyWindow::Shift16,
        ] {
            p.extend(cpu_v3::load_immediate16(2, x));
            p.extend(cpu_v3::load_immediate16(3, 0xffff));
            p.push(cpu_v3::multiply(window, 2, 3));
            p.push(cpu_v3::alu(cpu_v3::AluOp::Xor, 7, 7, 2));
        }
        p.extend(cpu_v3::load_immediate16(2, x));
        p.extend(cpu_v3::prefixed(cpu_v3::multiply_immediate(2, 15), 0x800));
        p.push(cpu_v3::alu(cpu_v3::AluOp::Xor, 7, 7, 2));
        for &(subop, mode) in &[(2u8, 0u8), (12, 0), (13, 0), (14, 0), (14, 1), (14, 2)] {
            p.extend(cpu_v3::fpu_scalar_raw(0, 1, 8, subop, mode));
            p.extend(cpu_v3::fpu_aux_raw(
                cpu_v3::FpuAuxKind::IntegerRegister,
                4,
                8,
                0,
                4,
                0,
            ));
            p.push(cpu_v3::alu(cpu_v3::AluOp::Xor, 7, 7, 4));
            // Integer <-> FPU switches, including rd==rs and zero extension.
            p.push(cpu_v3::multiply(cpu_v3::MultiplyWindow::Shift8, 3, 3));
        }
        p.extend(cpu_v3::fpu_vector(
            0,
            0,
            16,
            cpu_v3::FpuVectorLength::Vec4,
            cpu_v3::FpuVectorSubop::VMul,
            0,
        )); // VMUL4, four II1 returns.
        p.extend(cpu_v3::fpu_vector(
            0,
            0,
            20,
            cpu_v3::FpuVectorLength::Vec4,
            cpu_v3::FpuVectorSubop::Dot,
            0,
        )); // DOT4, four II1 returns.
    }
    p.push(cpu_v3::signal(7, 0));
    let expected = run_program_signal(&p, 20000);
    let emu = run_core_emu_trace(&p, 20000);
    assert_eq!(emu.last().unwrap().halt_signal, expected);
    let directory = std::path::PathBuf::from(
        std::env::var_os("SHARED_WIDE_EVIDENCE").expect("study evidence directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let project = VerilogProject::generate::<CpuV3Core>().unwrap();
    project.write_to(directory.join("core-export")).unwrap();
    std::fs::write(
        directory.join("program.hex"),
        p.iter().map(|x| format!("{x:04x}\n")).collect::<String>(),
    )
    .unwrap();
    let monitor = r#"
reg mon_q1_valid=0,mon_q2_valid=0;
reg signed [71:0] mon_q1_product=0,mon_q2_product=0;
reg [8:0] mon_q1_tag=0,mon_q2_tag=0;
integer mon_returns=0;
always @(posedge clk) begin
  if (!reset) begin
    if (dut.u_fpu.sf_mul_in_valid && dut.u_fpu.product_mul_in_valid)
      $fatal(1,"two FPU owners issue together");
    if (dut.u_fpu.mul_pipe.abort) begin
      mon_q1_valid=0;mon_q2_valid=0;
    end else begin
      if (dut.u_fpu.mul_pipe.out_valid !== mon_q2_valid)
        $fatal(1,"shared valid dropped or fabricated");
      if(mon_q2_valid) begin
        if (dut.u_fpu.mul_pipe.out_product !== mon_q2_product[63:0] || dut.u_fpu.mul_pipe.out_tag !== mon_q2_tag)
          $fatal(1,"shared product/tag ownership mismatch");
        mon_returns=mon_returns+1;
      end
      mon_q2_valid=mon_q1_valid;mon_q2_product=mon_q1_product;mon_q2_tag=mon_q1_tag;
      mon_q1_valid=dut.u_fpu.mul_pipe.in_valid;
      mon_q1_product=$signed(dut.u_fpu.mul_pipe.in_a)*$signed(dut.u_fpu.mul_pipe.in_b);
      mon_q1_tag=dut.u_fpu.mul_pipe.in_tag;
    end
    if (dut.state==2 && dut.instruction[15:12]==2 &&
        (dut.instruction[11:8]==8 || dut.instruction[11:8]==9 || dut.instruction[11:8]==10 || dut.instruction[11:8]==12) && !hold)
      $display("INT_ISSUE %0d %h",cycles,dut.instruction);
    if (dut.state==6 && !hold) $display("INT_COMMIT %0d %h %h",cycles,dut.instruction,dut.multiplier_product);
    if (dut.u_fpu.special_path.load_now)
      $display("SF_ISSUE %0d %0d %0d",cycles,dut.u_fpu.special_path.subop,dut.u_fpu.special_path.mode);
    if (dut.u_fpu.sf_write_enable)
      $display("SF_WRITE %0d %0d %h",cycles,dut.u_fpu.sf_write_address,dut.u_fpu.sf_write_data);
    if(halted) $display("OWNERS_OK %0d",mon_returns);
  end
end
"#;
    let integer_monitor = r#"
always @(posedge clk) if(!reset) begin
  if(dut.integer_mul_issue && (dut.fpu2_busy || dut.u_fpu.sf_mul_in_valid || dut.u_fpu.product_mul_in_valid))
    $fatal(1,"integer/FPU owners overlap or drop a request");
  if(dut.u_fpu.integer_out_valid && dut.u_fpu.mul_out_valid)
    $fatal(1,"integer return delivered to FPU owner");
  if(dut.state==6 && !hold && dut.u_fpu.mul_out_tag !== 9'h100)
    $fatal(1,"integer commit does not own retained product");
end
"#;
    for pause in [false, true] {
        let mut tb = build_core_cosim_tb(&p, &CpuV3Core::verilog_identity().module_name(), 20000);
        tb = tb.replacen(
            "endmodule",
            &format!("{monitor}\n{integer_monitor}\nendmodule"),
            1,
        );
        if pause {
            let pauses = r#"
integer pause_phase=0,pause_remaining=0;
always @(negedge clk) begin
  if(pause_remaining>0) begin
    pause_remaining=pause_remaining-1;
    if(pause_remaining==0) hold=0;
  end else if(!reset && pause_phase==0 && dut.state==5) begin
    hold=1;pause_remaining=5;pause_phase=1;
  end else if(!reset && pause_phase==1 && dut.state==6) begin
    hold=1;pause_remaining=7;pause_phase=2;
  end
end
"#;
            tb = tb.replacen("endmodule", &format!("{pauses}\nendmodule"), 1);
        }
        std::env::set_var(
            "SHARED_WIDE_TRACE_FILE",
            directory.join(if pause {
                "integer-hold.log"
            } else {
                "nominal.log"
            }),
        );
        std::fs::write(
            directory.join(if pause {
                "integer-hold-tb.v"
            } else {
                "nominal-tb.v"
            }),
            &tb,
        )
        .unwrap();
        let rtl = run_core_rtl_trace(&tb);
        assert!(rtl.last().unwrap().halted, "bounded replay must halt");
        assert_eq!(
            rtl.last().unwrap().halt_signal,
            expected,
            "architectural numeric result"
        );
        if !pause {
            assert_eq!(rtl.len(), emu.len());
            for (cycle, (a, b)) in emu.iter().zip(&rtl).enumerate() {
                assert!(
                    b.equal_core(a),
                    "mixed owner core cycle {cycle}: emu={a:?}, rtl={b:?}"
                );
            }
        }
        println!(
            "SHARED_WIDE replay pause={pause} words={} cycles={} signal={expected}",
            p.len(),
            rtl.len()
        );
    }
    std::env::remove_var("SHARED_WIDE_TRACE_FILE");
}
