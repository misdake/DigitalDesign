//! Explicit RTL fixture perturbations, never a public driver/game interface.
//! Cursor seeds represent an unused allocated prefix; transport cases use real errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Success,
    CrossWindow,
    Capacity,
    GpuError,
    DisplayError,
    PublicationError,
}
impl Case {
    // Admit exactly the deliberately injected peer error at one passive monitor.
    // All datapath text and every unexpected-error assertion remain unchanged.
    pub fn expected_error_monitor(self, source: &str) -> Option<String> {
        if self != Self::GpuError {
            return None;
        }
        let check = "$fatal(1,\"GPU contract: ro_response_error\");";
        let count = source.matches(check).count();
        if count == 0 {
            return None;
        }
        assert_eq!(count, 1);
        Some(source.replace(check, &format!(
            "begin if(tb.diag_error&&tb.rov&&tb.roe)$display(\"NATIVE_EXPECTED_GPU_RO_ERROR\");else {check} end"
        )))
    }
    pub fn from_env() -> Self {
        match std::env::var("GPU_RENDERER_NATIVE_CASE")
            .unwrap_or_else(|_| "success".into())
            .as_str()
        {
            "success" => Self::Success,
            "cross-window" => Self::CrossWindow,
            "capacity" => Self::Capacity,
            "gpu-error" => Self::GpuError,
            "display-error" => Self::DisplayError,
            "publication-error" => Self::PublicationError,
            other => panic!("unknown native case {other}"),
        }
    }
    pub fn source(self, source: &str) -> String {
        if self != Self::CrossWindow {
            return source.to_owned();
        }
        assert_eq!(source.matches("tick < 4").count(), 1);
        let source = source.replace("tick < 4", "tick < 3");
        let call = "            pass_draw(&mut pass, &mut mesh, &mut model, &mut material);";
        assert_eq!(source.matches(call).count(), 1);
        source.replace(call,&format!("{call}\n{}",r#"            if tick==1 {
                // Mutating CPU owners cannot rewrite the draw already recorded.
                camera_pose(&mut camera,vec3::new(fix32::from_words(8192,0),z,z),vec3::zero());
                model_pose(&mut model,vec3::new(fix32::from_words(16384,0),z,fix32::from_int(-3)),vec3::zero(),vec3::new(one,one,one));
                material_color(&mut material,0,0,255,255);
            }"#))
    }
    pub fn testbench(self, mut tb: String, frame_entry: usize) -> String {
        if self == Self::Success {
            return tb;
        }
        tb=tb.replace("GSR GSR", "wire[15:0]diag_wd;wire diag_error;reg injected=0,fence_active=0;integer cross_cpu=0,cross_gpu=0,frame_calls=0;reg frame_seen=0;\nGSR GSR");
        tb = tb.replace(".cpu_write_data(wd)", ".cpu_write_data(diag_wd)");
        tb = tb.replace("shadow[da]<=wd", "shadow[da]<=diag_wd");
        tb = tb.replace("assign ae=1'b0;", "assign ae=diag_error;");
        let seed = match self {
            Self::CrossWindow => 16320,
            Self::Capacity => 32768,
            _ => 0,
        };
        let seed_condition = if seed != 0 {
            "!injected&&starts==1&&dq&&dr&&dw&&da==SLOTB+8&&wd==16"
        } else {
            "1'b0"
        };
        let error_condition = match self {
            Self::GpuError => {
                "!injected&&starts==2&&ms==2&&mi==mn-1&&mb>='h7e0000/8&&mb<'h7f0000/8"
            }
            Self::DisplayError => {
                "!injected&&starts==2&&display.next_pending&&ms==2&&mb>='h430000/8&&mb<'h45ee00/8"
            }
            Self::PublicationError => {
                "!injected&&starts==0&&ms==5&&mwrite&&fence_active&&shadow[PAGES]==2"
            }
            _ => "1'b0",
        };
        let decl = format!(
            r#"
localparam FRAME_ENTRY={frame_entry};
wire seed_cursor={seed_condition};
assign diag_wd=seed_cursor?16'd{seed}:wd;
assign diag_error={error_condition};
always @(posedge clk)if(!reset)begin
 if(wen&&di==0&&(ch==1||ch==4))fence_active<=1;
 if(done||error)fence_active<=0;
 if(pc!=FRAME_ENTRY)frame_seen<=0;
 if(pc==FRAME_ENTRY&&!frame_seen)begin frame_seen<=1;frame_calls<=frame_calls+1;$display("NATIVE_ACQUIRE ordinal=%0d cycle=%0d",frame_calls,cycle);end
 if(wen&&di==7&&ch==9)$display("NATIVE_RECORD_END tick=%0d cycle=%0d gpu=%0d",wdata,cycle,gpu.core.busy);
 if(wen&&di==7&&ch==10)$display("NATIVE_FRAME_RETURN tick=%0d cycle=%0d gpu=%0d pending=%0d",wdata,cycle,gpu.core.busy,display.next_pending);
 if(cross_cpu==0&&dq&&dr&&dw&&da>=('h7f0000+32768)/2&&da<'h800000/2)$display("NATIVE_CPU_CROSS cycle=%0d address=%h",cycle,da);
 if(cross_gpu==0&&roq&&ror&&roa>=('h7f0000+32768)/2&&roa<'h800000/2)$display("NATIVE_GPU_CROSS cycle=%0d address=%h",cycle,roa);

 if(seed_cursor)begin injected<=1;$display("NATIVE_CURSOR_SEED cycle=%0d value=%0d",cycle,diag_wd);end
 if(diag_error&&av&&arr)begin injected<=1;$display("NATIVE_TRANSPORT_INJECT cycle=%0d address=%h write=%0d",cycle,mb*8,mwrite);end
 if(dq&&dr&&dw&&da>=('h7f0000+32768)/2&&da<'h800000/2)cross_cpu<=cross_cpu+1;
 if(roq&&ror&&roa>=('h7f0000+32768)/2&&roa<'h800000/2)cross_gpu<=cross_gpu+1;
end
"#
        );
        let anchor = "function[15:0]word_at;";
        assert!(tb.contains(anchor));
        tb = tb.replace(anchor, &(decl + anchor));
        tb = tb.replace(
            "2:if(arr)begin if(mi==mn-1)",
            "2:if(arr)begin if(diag_error||mi==mn-1)",
        );
        tb = tb.replace(
            "for(k=0;k<16;k=k+1)if(k<mn)mem[mb+k]<=write_buffer[k];",
            "if(!diag_error)for(k=0;k<16;k=k+1)if(k<mn)mem[mb+k]<=write_buffer[k];",
        );
        if self == Self::CrossWindow {
            // Shift the free-running frame calendar away from the first GPU
            // terminal, avoiding an otherwise redundant whole blank frame.
            // Actual Display retirement/scanout still controls both presents.
            assert_eq!(tb.matches("pixel_half_period=2.5").count(), 1);
            tb = tb.replace("pixel_half_period=2.5", "pixel_half_period=2.6");
            assert_eq!(tb.matches("maintenance<3").count(), 1);
            tb = tb.replace("maintenance<3", "maintenance<2");
            tb=tb.replace(" if(starts!=3", " $display(\"NATIVE_CROSS_DETAIL maintenance=%0d FST=%0d CE0=%0d blocked=%0d overlap=%0d cross=%0d/%0d DSEG=%0d,%0d,%0d,%0d\",maintenance,fst,ce0,blocked,overlap,cross_cpu,cross_gpu,cpu.data_segment_0,cpu.data_segment_1,cpu.data_segment_2,cpu.data_segment_3);\n if(starts!=3");
            tb = tb.replace(
                "200,120)!==16'hf800",
                "200,120)!==(terminals==0?16'hf800:16'h001f)",
            );
            tb=tb.replace("   if(pixel_at('h460000", "   if(pixel_at(terminals%2==0?'h430000:'h400000,258,175)!==(terminals==0?16'hf800:16'h1113))$fatal(1,\"recorded pose snapshot changed\");\n   if(pixel_at('h460000");
            tb = tb.replace(
                "starts!=3||terminals!=3||swaps!=3||presents!=3",
                "starts!=2||terminals!=2||swaps!=2||presents!=2",
            );
            tb = tb.replace("ui_starts!=3||ui_done!=3", "ui_starts!=2||ui_done!=2");
            tb=tb.replace(" $display(\"GPU_RENDERER_NATIVE_PASS", " if(cpu.data_segment_0!=0||cpu.data_segment_1!=1||cpu.data_segment_2!=2||cpu.data_segment_3!=3)$fatal(1,\"DSEG restore failed\");\n if(!injected||cross_cpu==0||cross_gpu==0)$fatal(1,\"32KiB mapping not exercised\");\n $display(\"NATIVE_CROSS_WINDOW cpuStores=%0d GPUreads=%0d\",cross_cpu,cross_gpu);\n $display(\"GPU_RENDERER_NATIVE_PASS");
            return tb;
        }
        if self == Self::PublicationError {
            // The actual full-maintenance contract is hardware fail-stop,
            // not a guaranteed software return through potentially dirty stack.
            tb = tb.replace(
                " wait(halted||fault);#1;",
                " wait(injected);\n repeat(1024)begin @(negedge clk);if(cache.state!=cache.ST_FAULT||ready||dr||mq||starts!=0||presents!=0)$fatal(1,\"cache failure admitted new work\");end\n #1;",
            );
        }
        if self == Self::DisplayError {
            // Cold Display initially emits black without reading DRAM. Inject
            // only after the first swap, against that real Front on the next
            // present. Output health is expected to fail after this read error.
            tb = tb.replace(
                "if(display_underflow||display_start_error)",
                "if((display_underflow||display_start_error)&&!injected)",
            );
            tb = tb.replace("&&swaps>0)begin", "&&swaps>0&&!injected)begin");
        }
        // Failed GPU terminal never qualifies rendered output or a successful page release.
        let pixel = "   if(pixel_at(terminals%2==0?'h430000:'h400000,3,2)";
        assert!(tb.contains(pixel));
        tb = tb.replace(pixel, &("   if(!injected)begin\n".to_owned() + pixel));
        tb = tb.replace(
            "   $display(\"NATIVE_GPU_DONE",
            "   end\n   $display(\"NATIVE_GPU_DONE",
        );
        let final_checks = match self {
            Self::DisplayError => {
                r#"
 if(!injected||!display.memory_error_sticky||fault||signal[15:12]!=4'he||starts!=2||presents!=2||swaps!=1)$fatal(1,"Display error did not stop later submit/present");
 if(shadow[PIPE]!=3||shadow[RENDERER]!=5||shadow[PAGES]!=2||shadow[PIPE+2]!=1)$fatal(1,"Display failure released its queued input");
 $display("GPU_RENDERER_NATIVE_DISPLAY_ERROR_PASS cycle=%0d",cycle);
"#
            }
            Self::Capacity => {
                r#"
 if(!injected||fault||signal!==16'he030||starts!=1||presents!=0)$fatal(1,"capacity did not fail stop");
 if(shadow[PIPE]!=3||shadow[RENDERER]!=5||shadow[PAGES]!=3||shadow[PAGES+4]!=0||shadow[PIPE+1]!=2)$fatal(1,"capacity released a live owner");
 $display("GPU_RENDERER_NATIVE_CAPACITY_PASS cycle=%0d",cycle);
"#
            }
            Self::GpuError => {
                r#"
 if(!injected||!gpu.core.transport_error||fault||signal[15:12]!=4'he||starts!=2||presents!=1||swaps!=1)$fatal(1,"GPU error did not stop later submit/present");
 if(shadow[PIPE]!=3||shadow[RENDERER]!=5||shadow[PAGES+4]!=3||shadow[PIPE+3]!=2)$fatal(1,"GPU failure released its input reference");
 $display("GPU_RENDERER_NATIVE_GPU_ERROR_PASS cycle=%0d",cycle);
"#
            }
            Self::PublicationError => {
                r#"
 if(!injected||starts!=0||presents!=0||swaps!=0)$fatal(1,"publication error launched GPU");
 if(shadow[PAGES]!=2||shadow[PIPE+2]!=1)$fatal(1,"publication failure lost recorded input");
 if(cache.state!=cache.ST_FAULT||!error||!busy||ready)$fatal(1,"publication error not a hardware fail stop");
 $display("GPU_RENDERER_NATIVE_PUBLICATION_ERROR_PASS cycle=%0d cacheState=%0d cpuFault=%0d signal=%h",cycle,cache.state,fault,signal);
"#
            }
            _ => unreachable!(),
        };
        let (head, _) = tb
            .split_once(" if(fault||signal!==16'h600d)")
            .expect("final guard");
        head.to_owned() + final_checks + "\n $finish;\nend\nendmodule\n"
    }
    pub fn marker(self) -> &'static str {
        match self {
            Self::Success | Self::CrossWindow => "GPU_RENDERER_NATIVE_PASS",
            Self::Capacity => "GPU_RENDERER_NATIVE_CAPACITY_PASS",
            Self::GpuError => "GPU_RENDERER_NATIVE_GPU_ERROR_PASS",
            Self::DisplayError => "GPU_RENDERER_NATIVE_DISPLAY_ERROR_PASS",
            Self::PublicationError => "GPU_RENDERER_NATIVE_PUBLICATION_ERROR_PASS",
        }
    }
}
