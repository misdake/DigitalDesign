//! Finite ROM-driven actual post-quad chain, independent DRAM byte golden.
#[path = "fixtures/postquad_rom.rs"]
mod rom;
use gpu_v2::{
    framebuffer::{ports::*, sim::oracle},
    system::pixel::{postquad, quad_ingress},
    texture::ports::Slot,
};
use std::{fmt::Write, fs, path::PathBuf, process::Command};
fn surface() -> MaterializedSurface {
    MaterializedSurface {
        color_base_bytes: rom::COLOR_BASE,
        depth_base_bytes: rom::DEPTH_BASE,
        width: 512,
        height: 256,
    }
}
fn slots() -> Vec<Slot> {
    vec![Slot {
        base_address: rom::TEX_BASE,
        max_size_log2: rom::MAX_N,
        has_full_mip: true,
        valid: true,
    }]
}
fn memory_index(address: u32) -> usize {
    if address >= rom::DEPTH_BASE {
        49152 + (address - rom::DEPTH_BASE) as usize
    } else if address >= rom::COLOR_BASE {
        32768 + (address - rom::COLOR_BASE) as usize
    } else {
        address as usize
    }
}
fn image(profile: rom::Profile, jobs: &[(rom::Profile, rom::Quad)]) -> (Vec<u8>, Vec<u8>) {
    let mut initial = vec![0; 65536];
    initial[49152..].fill(255);
    let asset = rom::texture_asset();
    initial[rom::TEX_BASE as usize..rom::TEX_BASE as usize + asset.len()].copy_from_slice(&asset);
    let mut gold = initial.clone();
    let context = Context {
        depth: if profile.depth_func() == 3 {
            DepthFunc::LessEqual
        } else {
            DepthFunc::Always
        },
        depth_write: true,
        blend: if profile.blend() == 1 {
            Blend::SrcOver
        } else {
            Blend::Replace
        },
    };
    for &(p, q) in jobs {
        for lane in 0..4 {
            let x = q.x + ((lane & 1) as u16);
            let y = q.y + ((lane >> 1) as u8);
            let tile = u32::from(y / 16) * 32 + u32::from(x / 16);
            let offset = tile * 512 + u32::from(y % 16) * 32 + u32::from(x % 16) * 2;
            let c = memory_index(rom::COLOR_BASE + offset);
            let d = memory_index(rom::DEPTH_BASE + offset);
            let old = oracle::Pixel {
                color: u16::from_le_bytes([gold[c], gold[c + 1]]),
                depth: u16::from_le_bytes([gold[d], gold[d + 1]]),
            };
            let rgb = rom::final_rgb(&q, lane, p);
            let out = oracle::pixel(
                old,
                Fragment {
                    rgba: [rgb[0], rgb[1], rgb[2], profile.alpha()],
                    depth: q.depth[lane],
                },
                q.mask & (1 << lane) != 0,
                context,
            )
            .pixel;
            gold[c..c + 2].copy_from_slice(&out.color.to_le_bytes());
            gold[d..d + 2].copy_from_slice(&out.depth.to_le_bytes());
        }
    }
    (initial, gold)
}
fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}\n")).collect()
}
fn bench(profile: rom::Profile, prime: usize, total: usize, finals: usize, stall: bool) -> String {
    let spec = profile.specular();
    let packed_spec = u32::from_le_bytes([spec[0], spec[1], spec[2], 0]);
    let mut prefix = format!(
        r#"module tb;
 `ifdef GPU_V2_GOWIN_DSP
 GSR GSR(.GSRI(1'b1));
 `endif
 reg clk=0,reset=1,ce=0,abort=0,in_valid=0,draw_rebind=0,flush_request=0;
 reg[22:0]in_begin=0;reg[31:0]in_basic=0;reg[35:0]in_light=0,in_uv=0;
 reg[26:0]draw_config=27'h{config:x};reg[23:0]specular=24'h{packed_spec:x};reg[7:0]alpha={alpha};
 reg[2:0]depth_func={depth};reg depth_write=1,blend={blend};
 reg[15:0]context_epoch=1;reg[1:0]context_mode={mode};reg[4:0]context_code=8;
 reg signed[15:0]light_x=0,light_y=0,light_z=16384,ray_x=-8192,ray_y=-8192,ray_k=8192;
 reg[8:0]ambient=32,directional=192;
 wire in_ready;wire[2:0]in_row;wire flush_complete,pipeline_idle,idle,fault,published;
 wire[4:0]published_slot,work_reserved,work_count;wire[5:0]global_used;
 wire light_result,sample_result,final_issue,final_result,quad_accepted,commit_valid;
 wire[8:0]commit_x;wire[7:0]commit_y;wire[3:0]commit_mask;
 wire mem_req_valid,mem_req_write,mem_write_valid;wire[31:0]mem_req_addr;wire[63:0]mem_write_data;
 reg mem_req_ready=0,mem_read_valid=0,mem_write_ready=0,mem_complete_valid=0,mem_complete_ok=1;reg[63:0]mem_read_data=0;
 gpu_v2_postquad dut(.*);
 localparam PRIME={prime},TOTAL={total},STALL={stall},N=256;
 reg[126:0]rom[0:TOTAL*8-1];reg[23:0]rgb_gold[0:TOTAL*4-1],sample_gold[0:TOTAL*4-1];reg[17:0]light_gold[0:TOTAL-1];
 reg[7:0]memory[0:65535],gold[0:65535];
 integer slot_job[0:31];integer pub_time[0:255],retire_time[0:255],commit_time[0:255];
 integer wall,cursor=0,pubs=0,retires=0,commits=0,phase=0,pending=0,beat=0,delay_count=0;
 integer reqs=0,txreqs=0,fbreads=0,fbwrites=0,readbeats=0,writebeats=0,ce0paid=0;
 integer coordinate_records=0,packet_groups=0,color_groups=0,enabled_edges=0,source_blocked=0;
 integer measured_start=0,source_done=0,drained=0,first_final=0,last_final=0,finals=0,peak_global=0,peak_work=0;
 integer address=0,i,k,job,lane,idx,prime_done=0;reg[31:0]held_req;reg held_valid=0,held_write;
 reg[63:0]held_data;reg held_wvalid=0;reg[126:0]held_source;reg held_source_valid=0;
 function integer mi;input[31:0]a;begin
  if(a>=32'h140000)mi=49152+a-32'h140000;
  else if(a>=32'h100000)mi=32768+a-32'h100000;else mi=a;
  if(mi<0||mi>65535)$fatal(1,"memory address %h",a);
 end endfunction
 initial begin #2000000;$fatal(1,"HDL watchdog");end
 initial begin
  $readmemh("rom.mem",rom);$readmemh("initial.mem",memory);$readmemh("gold.mem",gold);
  $readmemh("rgb.mem",rgb_gold);$readmemh("sample.mem",sample_gold);$readmemh("light.mem",light_gold);
  for(i=0;i<32;i=i+1)slot_job[i]=-1;
  #1;clk=1;#1;clk=0;#1;reset=0;
  for(wall=1;wall<200000;wall=wall+1)begin
   ce=!STALL||(wall%29!=3&&wall%29!=4);
   if(phase==0&&cursor==PRIME*8)phase=1;
   if(phase==1&&idle)begin phase=2;measured_start=wall;prime_done=wall;end
   in_valid=(phase==0||phase==2)&&cursor<TOTAL*8;
   if(in_valid)begin
    in_begin=rom[cursor][126:104];in_basic=rom[cursor][103:72];in_light=rom[cursor][71:36];in_uv=rom[cursor][35:0];
   end
   mem_req_ready=pending==0&&(!STALL||wall%7!=1);
   mem_read_valid=pending==1&&delay_count==0&&(!STALL||wall%5!=2);
   mem_write_ready=pending==3&&delay_count==0&&(!STALL||wall%7!=2);
   mem_complete_valid=(pending==2||pending==4)&&delay_count==0;
   mem_read_data=0;if(mem_read_valid)for(k=0;k<8;k=k+1)mem_read_data[k*8+:8]=memory[mi(address+beat*8+k)];
   #2;
   if(phase>=2&&phase<4)begin
    if(ce)enabled_edges=enabled_edges+1;
    if(in_valid&&!in_ready)source_blocked=source_blocked+1;
    if(dut.front.coefficient_accept)coordinate_records=coordinate_records+1;
    if(dut.front.demand.in_valid&&dut.front.demand.in_ready)packet_groups=packet_groups+1;
    if(dut.front.color.input_fire)color_groups=color_groups+1;
   end
   if(fault)$fatal(1,"top fault wall%0d source%0d phase%0d global%0d state%0d front%0d sink%0d final%0d",wall,cursor,phase,global_used,dut.state,dut.front_fault,dut.sink_fault,dut.final_fault);
   if(global_used>peak_global)peak_global=global_used;if(work_reserved>peak_work)peak_work=work_reserved;
   if(held_valid&&(!mem_req_valid||mem_req_addr!==held_req||mem_req_write!==held_write))$fatal(1,"request hold");
   held_valid=mem_req_valid&&!mem_req_ready;held_req=mem_req_addr;held_write=mem_req_write;
   if(held_wvalid&&(!mem_write_valid||mem_write_data!==held_data))$fatal(1,"write hold");
   held_wvalid=mem_write_valid&&!mem_write_ready;held_data=mem_write_data;
   if(held_source_valid&&in_valid&&rom[cursor]!==held_source)$fatal(1,"source changed while blocked");
   held_source_valid=in_valid&&!in_ready;held_source=in_valid?rom[cursor]:0;
   if(published)begin
    slot_job[published_slot]=pubs;
    if(pubs>=PRIME)pub_time[pubs-PRIME]=wall;
    pubs=pubs+1;
   end
   if(light_result)begin
    job=slot_job[dut.lk[6:2]];
    if(job<0||{{dut.lh,dut.lg}}!==light_gold[job])$fatal(1,"light job%0d g%0d h%0d expected%h",job,dut.lg,dut.lh,light_gold[job]);
   end
   if(sample_result)begin
    job=slot_job[dut.sample_address[6:2]];lane=dut.sk[1:0];
    if(job<0||dut.sRGB!==sample_gold[job*4+lane])$fatal(1,"sample job%0d lane%0d actual%h expected%h",job,lane,dut.sRGB,sample_gold[job*4+lane]);
   end
   if(final_result)begin
    job=slot_job[dut.retire_slot];lane=dut.out_lane;
    if(dut.final_RGB!==rgb_gold[job*4+lane])$fatal(1,"final job%0d lane%0d actual%h expected%h",job,lane,dut.final_RGB,rgb_gold[job*4+lane]);
    if(job>=PRIME)begin if(finals==0)first_final=wall;last_final=wall;finals=finals+1;end
   end
   if(quad_accepted)begin if(retires>=PRIME)retire_time[retires-PRIME]=wall;retires=retires+1;end
   if(commit_valid)begin if(commits>=PRIME)commit_time[commits-PRIME]=wall;commits=commits+1;end
   if(in_valid&&in_ready)begin
    if(in_row!==(cursor%8))$fatal(1,"source row cursor");cursor=cursor+1;
    if(cursor==TOTAL*8)begin source_done=wall;phase=3;end
   end
   if(mem_req_valid&&mem_req_ready)begin
    if(pending!=0)$fatal(1,"overlapping request");
    address=mem_req_addr;beat=0;delay_count=6;pending=mem_req_write?3:1;
    if(phase>=2)begin
     reqs=reqs+1;
     if(address<32'h100000)txreqs=txreqs+1;else if(mem_req_write)fbwrites=fbwrites+1;else fbreads=fbreads+1;
    end
   end else if(delay_count>0)delay_count=delay_count-1;
   if(mem_read_valid)begin
    if(phase>=2)readbeats=readbeats+1;
    if(!ce)ce0paid=ce0paid+1;
    beat=beat+1;if(beat==16)begin pending=2;delay_count=2;end
   end
   if(mem_write_valid&&mem_write_ready)begin
    for(k=0;k<8;k=k+1)memory[mi(address+beat*8+k)]=mem_write_data[k*8+:8];
    if(phase>=2)writebeats=writebeats+1;
    beat=beat+1;if(beat==16)begin pending=4;delay_count=2;end
   end
   if(mem_complete_valid)pending=0;
   if(phase==3&&idle)begin drained=wall;phase=4;flush_request=1;end
   if(phase==4&&flush_complete)begin
    if(pubs!=TOTAL||retires!=TOTAL||commits!=TOTAL||finals!={finals})$fatal(1,"completion counts %0d %0d %0d %0d",pubs,retires,commits,finals);
    for(i=0;i<65536;i=i+1)if(memory[i]!==gold[i])$fatal(1,"DRAM golden byte%0d got%h expected%h",i,memory[i],gold[i]);
    $display("METRICS start=%0d prime=%0d source_done=%0d drain=%0d flush=%0d pub_first=%0d pub_last=%0d retire_first=%0d retire_last=%0d commit_first=%0d commit_last=%0d pub_window=%0d retire_window=%0d commit_window=%0d window_intervals=127 final_first=%0d final_last=%0d finals=%0d requests=%0d tx=%0d fbread=%0d fbwrite=%0d readbeats=%0d writebeats=%0d ce0paid=%0d peak_global=%0d peak_work=%0d",measured_start,prime_done,source_done,drained,wall,pub_time[0],pub_time[255],retire_time[0],retire_time[255],commit_time[0],commit_time[255],pub_time[223]-pub_time[96],retire_time[223]-retire_time[96],commit_time[223]-commit_time[96],first_final,last_final,finals,reqs,txreqs,fbreads,fbwrites,readbeats,writebeats,ce0paid,peak_global,peak_work);
    $display("WORK coordinate_records=%0d packet_groups=%0d color_groups=%0d enabled_edges=%0d source_blocked=%0d",coordinate_records,packet_groups,color_groups,enabled_edges,source_blocked);
    $display("PASS actual postquad ROM, 256 measured quads, whole DRAM golden");$finish;
   end
   clk=1;#2;clk=0;#1;
  end
  $fatal(1,"wall bound phase%0d source%0d pub%0d ret%0d commit%0d global%0d state%0d pending%0d",phase,cursor,pubs,retires,commits,global_used,dut.state,pending);
 end endmodule
 "#,
        config = profile.sample_config27(),
        alpha = profile.alpha(),
        depth = profile.depth_func(),
        blend = profile.blend(),
        mode = if profile.feature.lit() { 3 } else { 0 },
        stall = u8::from(stall)
    );
    prefix = prefix.replace("in_row!==(cursor%8)", "in_row!==((cursor%8)&3'b111)");
    prefix
}
#[test]
#[ignore = "actual complete RTL, bounded ROM performance and native primitives"]
fn actual_postquad_rom_performance() {
    let selected = std::env::var("POSTQUAD_FINAL").unwrap_or("reference".into());
    let profile = match selected.as_str() {
        "folded" => postquad::FinalProfile::Folded,
        "threshold" => postquad::FinalProfile::Threshold,
        "shared" => postquad::FinalProfile::Shared,
        "single" => postquad::FinalProfile::SingleCaptured,
        "borrowed" => postquad::FinalProfile::SingleBorrowed,
        "forwarded" => postquad::FinalProfile::Forwarded,
        _ => postquad::FinalProfile::Reference,
    };
    let native = std::env::var("POSTQUAD_PORTABLE").is_err();
    let closed_depth = std::env::var("POSTQUAD_CLOSED_DEPTH").is_ok();
    let fused_join = std::env::var("POSTQUAD_FUSED_JOIN").is_ok();
    let sparse = std::env::var("POSTQUAD_SPARSE").is_ok();
    let spatial = std::env::var("POSTQUAD_SPATIAL").is_ok();
    let completion_frontiers = std::env::var("POSTQUAD_FRONTIERS").is_ok();
    let independent_sink = std::env::var("POSTQUAD_INDEPENDENT_SINK").is_ok();
    let mixed = std::env::var("POSTQUAD_MIXED").is_ok();
    let raw_proposals = std::env::var("POSTQUAD_RAW_PROPOSALS").is_ok();
    let closed_bank = std::env::var("POSTQUAD_CLOSED_BANK").is_ok();
    let reuse_temporaries = std::env::var("POSTQUAD_REUSE_TEMPORARIES").is_ok();
    let closed_sink_ingress = std::env::var("POSTQUAD_CLOSED_SINK_INGRESS").is_ok();
    let block_output = std::env::var("POSTQUAD_BLOCK_OUTPUT").is_ok();
    let closed_color = std::env::var("POSTQUAD_CLOSED_COLOR").is_ok();
    let narrow_quantization = std::env::var("POSTQUAD_NARROW_QUANTIZATION").is_ok();
    let hard_descriptor = std::env::var("POSTQUAD_HARD_DESCRIPTOR").is_ok();
    let descriptor_control = std::env::var("POSTQUAD_DESCRIPTOR_CONTROL").is_ok();
    let stream_output = std::env::var("POSTQUAD_STREAM_OUTPUT").is_ok();
    let mut variant = format!(
        "{selected}-bound{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}",
        if closed_depth { "-depth" } else { "" },
        if fused_join { "-fused" } else { "" },
        if sparse { "-sparse" } else { "" },
        if completion_frontiers {
            "-frontiers"
        } else {
            ""
        },
        if independent_sink { "-sink" } else { "" },
        if mixed { "-mixed" } else { "" },
        if raw_proposals { "-raw" } else { "" },
        if closed_bank { "-bank" } else { "" },
        if reuse_temporaries { "-reuse" } else { "" },
        if closed_sink_ingress { "-closed" } else { "" },
        if block_output { "-block" } else { "" },
        if closed_color { "-color" } else { "" },
        if narrow_quantization { "-quant" } else { "" },
        if hard_descriptor { "-descriptor" } else { "" },
        if descriptor_control { "-control" } else { "" },
        if stream_output { "-stream" } else { "" }
    );
    if spatial {
        variant.push_str("-spatial");
    }
    if std::env::var("POSTQUAD_RTL_SOURCE").is_ok() {
        variant.push_str("-external-rtl");
    }
    for (env, suffix) in [
        ("POSTQUAD_CACHE_PIPELINE", "-cachepipe"),
        ("POSTQUAD_JOIN_OVERLAP", "-joinlap"),
        ("POSTQUAD_ROP_OVERLAP", "-roplap"),
    ] {
        if std::env::var(env).is_ok() {
            variant.push_str(suffix);
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/postquad-rom-20261010/{variant}-native-{native}"
    ));
    fs::create_dir_all(&root).unwrap();
    let backend = if native {
        quad_ingress::rtl::Backend::Native
    } else {
        quad_ingress::rtl::Backend::Portable
    };
    let mut source = postquad::verilog_profile(
        &slots(),
        surface(),
        backend,
        postquad::CompositionProfile {
            final_stage: profile,
            closed_depth,
            fused_join,
            completion_frontiers,
            independent_sink,
            raw_proposals,
            closed_bank,
            reuse_temporaries,
            closed_sink_ingress,
            block_output,
            closed_color,
            narrow_quantization,
            hard_descriptor,
            descriptor_control,
            stream_output,
            cache_pipeline: std::env::var("POSTQUAD_CACHE_PIPELINE").is_ok(),
            join_overlap: std::env::var("POSTQUAD_JOIN_OVERLAP").is_ok(),
            rop_overlap: std::env::var("POSTQUAD_ROP_OVERLAP").is_ok(),
            final_reissue: std::env::var("POSTQUAD_FINAL_REISSUE").is_ok(),
            final_two_cycle: std::env::var("POSTQUAD_FINAL_TWO_CYCLE").is_ok(),
            join_pipeline: std::env::var("POSTQUAD_JOIN_PIPELINE").is_ok(),
            rop_prefix: std::env::var("POSTQUAD_ROP_PREFIX").is_ok(),
            final_fraction_bits: std::env::var("POSTQUAD_FINAL_FRACTION_BITS")
                .ok()
                .map(|x| x.parse().unwrap()),
            color_precision: postquad::ColorPrecision::Reference,
            rop_precision: postquad::RopPrecision::Reference,
            color_contract: false,
            color_normalize_511: false,
            framebuffer_write_address: false,
            framebuffer_write_data: std::env::var("POSTQUAD_FRAMEBUFFER_WRITE_DATA").is_ok(),
            framebuffer_held_draw: std::env::var("POSTQUAD_FRAMEBUFFER_HELD_DRAW").is_ok(),
        },
    )
    .unwrap();
    if native {
        source = "`define GPU_V2_GOWIN_DSP\n".to_owned() + &source;
    }
    if let Ok(path) = std::env::var("POSTQUAD_RTL_SOURCE") {
        source = fs::read_to_string(path).unwrap();
    }
    fs::write(root.join("native.v"), &source).unwrap();
    let chosen = std::env::var("POSTQUAD_CASE")
        .ok()
        .map(|x| x.parse::<usize>().unwrap());
    for (number, p) in rom::matrix().into_iter().enumerate() {
        if chosen.is_some_and(|x| x != number) {
            continue;
        }
        let case = root.join(format!("case-{number:02}"));
        fs::create_dir_all(&case).unwrap();
        let mut jobs = Vec::new();
        for (pp, mut qs) in rom::primers(p) {
            if spatial && pp.feature.textured() {
                qs = rom::workloads(pp, rom::QUADS, qs[0].x, 65535);
                rom::spatial_uv(&mut qs);
                for q in &mut qs {
                    q.depth = [65535; 4];
                }
            }
            jobs.extend(qs.into_iter().map(|q| (pp, q)));
        }
        let prime = jobs.len();
        let mut measured = rom::quads(p);
        if spatial {
            rom::spatial_uv(&mut measured);
        }
        if sparse {
            for (i, q) in measured.iter_mut().enumerate() {
                q.mask = (i % 15 + 1) as u8;
            }
        }
        let finals = measured.iter().map(|q| q.mask.count_ones() as usize).sum();
        jobs.extend(measured.into_iter().enumerate().map(|(i, q)| {
            let mut job_profile = p;
            if mixed {
                job_profile.feature = match i % 4 {
                    0 => rom::Feature::Bare,
                    1 => rom::Feature::Texture,
                    2 => rom::Feature::Lighting,
                    _ => rom::Feature::Full,
                };
            }
            (job_profile, q)
        }));
        let mut data = String::new();
        let mut rgbs = String::new();
        let mut samples = String::new();
        let mut lights = String::new();
        for &(pp, q) in &jobs {
            for r in q.rows(pp) {
                let v = ((r.begin as u128) << 104)
                    | ((r.basic as u128) << 72)
                    | ((r.lighting as u128) << 36)
                    | r.uv as u128;
                writeln!(data, "{v:032x}").unwrap();
            }
            let gh = pp.light_output();
            writeln!(lights, "{:05x}", (u32::from(gh[1]) << 9) | u32::from(gh[0])).unwrap();
            for lane in 0..4 {
                let rgb = rom::final_rgb(&q, lane, pp);
                let sample = rom::sample_rgb(&q, lane, pp);
                writeln!(
                    rgbs,
                    "{:06x}",
                    u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 0])
                )
                .unwrap();
                writeln!(
                    samples,
                    "{:06x}",
                    u32::from_le_bytes([sample[0], sample[1], sample[2], 0])
                )
                .unwrap();
            }
        }
        let (initial, gold) = image(p, &jobs);
        for (name, text) in [
            ("rom.mem", data),
            ("rgb.mem", rgbs),
            ("sample.mem", samples),
            ("light.mem", lights),
            ("initial.mem", hex_bytes(&initial)),
            ("gold.mem", hex_bytes(&gold)),
        ] {
            fs::write(case.join(name), text).unwrap();
        }
        let stall = std::env::var("POSTQUAD_STALL").is_ok();
        let mut tb = bench(p, prime, jobs.len(), finals, stall);
        if completion_frontiers {
            tb=tb.replace("integer slot_job[0:31];",r#"reg[31:0]shadow_light=0,shadow_sample=0;
 always @(posedge clk)if(!reset&&dut.advance)begin
  if(published)begin shadow_light[published_slot]<=dut.published_begin[5];shadow_sample[published_slot]<=dut.published_begin[6];end
  if(light_result&&dut.ll)shadow_light[dut.lk[6:2]]<=1;
  if(sample_result&&dut.sl)shadow_sample[dut.sample_address[6:2]]<=1;
  if(quad_accepted)begin shadow_light[dut.retire_slot]<=0;shadow_sample[dut.retire_slot]<=0;end
 end
 integer slot_job[0:31];"#);
            tb=tb.replace("if(fault)$fatal",r#"if(dut.published_count!=0)begin
  if(shadow_light[dut.retire_slot] !== (dut.head_descriptor[5]||dut.light_complete))$fatal(1,"light frontier at wall%0d slot%0d",wall,dut.retire_slot);
  if(shadow_sample[dut.retire_slot] !== (dut.head_descriptor[6]||dut.sample_complete))$fatal(1,"sample frontier at wall%0d slot%0d",wall,dut.retire_slot);
 end
 if(fault)$fatal"#);
            if descriptor_control {
                tb = tb.replace(
                    "if(dut.published_count!=0)begin",
                    "if(dut.published_count!=0&&dut.head_loaded)begin",
                );
            } else if hard_descriptor {
                tb=tb.replace("if(dut.published_count!=0)begin","if(dut.published_count!=0&&(dut.head_loaded||dut.descriptor_return_valid))begin");
            }
        }
        fs::write(case.join("tb.v"), tb).unwrap();
        let mut compile =
            Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()));
        compile
            .current_dir(&case)
            .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
        if native {
            compile.arg(
                PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        let result = compile
            .arg(root.join("native.v"))
            .arg("tb.v")
            .output()
            .unwrap();
        fs::write(case.join("compile.err"), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let result = Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()))
            .current_dir(&case)
            .arg("sim.vvp")
            .output()
            .unwrap();
        fs::write(case.join("run.out"), &result.stdout).unwrap();
        assert!(
            result.status.success(),
            "case{number} {p:?}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        println!(
            "CASE {number} {p:?}: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}
