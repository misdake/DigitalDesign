//! Actual cache/memory/Color joined to Quad, Lighting and preparation.
#[path = "pixel_preparation_ingress.rs"]
mod reference;
use gpu_v2::{
    system::pixel::{pixel_sampling_ingress, quad_ingress},
    texture::ports::{Group4, Slot},
};
use std::fmt::Write;

fn slots() -> Vec<Slot> {
    (0..16)
        .map(|i| Slot {
            base_address: i << 20,
            has_full_mip: true,
            max_size_log2: if i == 5 { 6 } else { 9 },
            valid: true,
        })
        .collect()
}
fn texel(address: u32) -> u16 {
    ((address >> 1) ^ (address >> 9).wrapping_mul(37) ^ 0x5a39) as u16
}
fn tb() -> String {
    let mut source = reference::preparation_tb();
    let mut init = String::new();
    let mut rgb_count = 0;
    let mut accum = [0u32; 3];
    let mut weights = 0;
    for line in source.lines().filter(|s| s.starts_with("group_gold[")) {
        let hex = line.split("72'h").nth(1).unwrap().trim_end_matches(';');
        let g = Group4::unpack72(u128::from_str_radix(hex, 16).unwrap() as i128).unwrap();
        if g.first {
            accum = [0; 3];
            weights = 0;
        }
        let n = u32::from(g.key.n);
        let prefix: u32 = (0..n).map(|k| 1 << (2 * k.saturating_sub(3))).sum();
        let side = 1 << n.saturating_sub(3);
        let base = (u32::from(g.key.slot) << 20)
            + 128 * (prefix + u32::from(g.key.y) * side + u32::from(g.key.x));
        for tap in 0..4 {
            let x = (u32::from(g.top_left_local[0]) + (tap as u32 & 1)) & 7;
            let y = (u32::from(g.top_left_local[1]) + (tap as u32 >> 1)) & 7;
            let t = u32::from(texel(base + 2 * (8 * y + x)));
            let channels = [
                ((t >> 11) & 31) * 8 | ((t >> 13) & 7),
                ((t >> 5) & 63) * 4 | ((t >> 9) & 3),
                (t & 31) * 8 | ((t >> 2) & 7),
            ];
            weights += g.coefficients[tap];
            for axis in 0..3 {
                accum[axis] += channels[axis] * g.coefficients[tap];
            }
        }
        if g.last {
            assert_eq!(weights, 511);
            let rgb = ((accum[0] + 255) / 511)
                | (((accum[1] + 255) / 511) << 8)
                | (((accum[2] + 255) / 511) << 16);
            writeln!(
                init,
                "rgb_gold[{rgb_count}]=24'h{rgb:x};rgb_keys[{rgb_count}]=6'd{};",
                g.quad_id * 4 + g.lane
            )
            .unwrap();
            rgb_count += 1;
        }
    }
    assert_eq!(rgb_count, 96);
    let extra = format!(
        r#"
reg abort=0,result_ready=0;wire result_valid,result_quad_last,result_fire,public_publish;
wire[5:0]result_key;wire[23:0]result_rgb;wire[15:0]public_live;
wire mem_req_valid,cache_drained;wire[31:0]mem_req_addr;
reg mem_req_ready=0,mem_resp_valid=0,mem_resp_complete=0,mem_resp_ok=1;
reg[3:0]mem_resp_index=0;reg[63:0]mem_resp_data=0;
integer pending=0,beat=0,requests=0,rgb_read=0,held_rgb=0,paid_memory=0;
integer public_now=0,peak_public=0;
reg borrowed=0;reg[71:0]borrowed_group=0;
reg[31:0]memory_address=0;reg[23:0]rgb_gold[0:{last}];reg[5:0]rgb_keys[0:{last}];
function[15:0]memory_texel;input[31:0]address;begin memory_texel=(address>>1)^((address>>9)*37)^16'h5a39;end endfunction
function[63:0]memory_beat;input[31:0]address;begin memory_beat={{memory_texel(address+6),memory_texel(address+4),memory_texel(address+2),memory_texel(address)}};end endfunction
gpu_v2_pixel_sampling_ingress dut(.*);
"#,
        last = rgb_count - 1
    );
    source = source.replace("gpu_v2_pixel_preparation_ingress dut(.*);", &extra)
        .replace("reg packet_enable=0,packet_out_ready=0;", "reg packet_enable=0;wire packet_out_ready;")
        .replace("reg public_credit=1;", "wire public_credit;")
        .replace("public_credit=sample_row!=0||public_left[sample_descriptor[3:0]]==0;", "")
        .replace("packet_out_ready=wall>400&&wall%9!=2&&wall%9!=3;", "")
        .replace("if(packet_out_payload[65])begin\n    key=packet_out_payload[69:66];job=public_q[key];\n    if(job<0||public_left[key]==0)$fatal(1,\"Group public lifetime\");\n    public_left[key]=public_left[key]-1;\n    if(public_left[key]==0)sample_done[slots[job]]=1;\n   end", "")
        .replace("wall<10000", "wall<20000")
        .replace("#200000", "#400000")
        .replace("&&producer_count==0&&group_count==0)begin", "&&producer_count==0&&group_count==0&&rgb_read==96&&public_live==0&&cache_drained&&pending==0)begin");
    assert!(!source.contains("Group public lifetime"));
    source = source.replacen(
        "for(i=0;i<32;i=i+1)begin",
        &(init + "for(i=0;i<32;i=i+1)begin"),
        1,
    );
    source = source.replacen(" #2;\n if(fault", r#"
 result_ready=wall>1200&&wall%11!=2&&wall%11!=3;
 mem_req_ready=pending==0&&wall%7!=1;
 mem_resp_valid=pending==1&&beat<16&&wall%5!=2;
 mem_resp_index=beat;mem_resp_data=memory_beat(memory_address+8*beat);
 mem_resp_complete=pending==2&&wall%3!=0;
 #2;
 public_now=0;for(i=0;i<16;i=i+1)if(public_live[i])public_now=public_now+1;
 if(public_now>peak_public)peak_public=public_now;
 if(sample_valid&&sample_row==3&&!dut.color.publish_ready)$fatal(1,"row0 reservation lost before row3");
 if(borrowed&&(!packet_out_valid||packet_out_payload!==borrowed_group))$fatal(1,"actual Pool changed borrowed Group");
 borrowed=dut.demand.desc_valid;borrowed_group=packet_out_payload;
 if(result_valid)begin
  if(rgb_read>=96||result_key!==rgb_keys[rgb_read]||result_rgb!==rgb_gold[rgb_read])$fatal(1,"RGB numerical/order wall%0d index%0d key%0d/%0d rgb%h/%h",wall,rgb_read,result_key,rgb_keys[rgb_read],result_rgb,rgb_gold[rgb_read]);
  key=result_key[5:2];job=public_q[key];
  if(job<0||public_left[key]==0||result_quad_last!==(public_left[key]==1))$fatal(1,"RGB public lifetime");
  if(!result_fire)held_rgb=held_rgb+1;
  if(result_fire)begin public_left[key]=public_left[key]-1;if(public_left[key]==0)sample_done[slots[job]]=1;rgb_read=rgb_read+1;end
 end
 if(public_publish!==(sample_fire&&sample_row==3))$fatal(1,"row3 Color publication");
 if(mem_req_valid&&mem_req_ready)begin
  if(pending!=0)$fatal(1,"overlapping memory requests");
  pending=1;memory_address=mem_req_addr;beat=0;requests=requests+1;
 end
 if(mem_resp_valid)begin
  if(!ce)paid_memory=paid_memory+1;
  beat=beat+1;if(beat==16)pending=2;
 end
 if(mem_resp_complete)pending=0;
 if(fault"#, 1);
    source.replace("$display(\"PASS preparation", "$display(\"PASS Sampling RGB=%0d requests=%0d paidMemoryCE0=%0d heldRGB=%0d publicPeak=%0d\",rgb_read,requests,paid_memory,held_rgb,peak_public);\n  $display(\"PASS preparation")
}

#[test]
#[ignore = "requires actual RTL and Gowin primitives"]
fn actual_quad_lighting_cache_color_qualify() {
    qualify(0);
}

#[test]
#[ignore = "requires actual RTL and Gowin primitives"]
fn actual_pool_closed_cache_color_qualify() {
    qualify(1);
}

#[test]
#[ignore = "requires actual RTL and Gowin primitives"]
fn actual_reserved_publication_qualify() {
    qualify(2);
}

#[test]
#[ignore = "requires actual RTL and Gowin primitives"]
fn actual_factored_publication_qualify() {
    qualify(3);
}

#[test]
#[ignore = "requires actual RTL and Gowin primitives"]
fn actual_factored_checked_qualify() {
    qualify(4);
}

#[test]
#[ignore = "requires actual RTL and Gowin primitives"]
fn actual_selected_sampling_full_qualify() {
    qualify(5);
}

#[test]
#[ignore = "requires actual RTL and Gowin primitives"]
fn actual_work_direct_sampling_qualify() {
    qualify(6);
}

fn qualify(profile: u8) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(match profile {
        6 => "../../target/pixel-sampling-work-direct-20261010",
        5 => "../../target/pixel-sampling-work-selected-20261010",
        4 => "../../target/pixel-sampling-factored-checked-20261010",
        3 => "../../target/pixel-sampling-factored-20261010",
        2 => "../../target/pixel-sampling-reserved-20261010",
        1 => "../../target/pixel-sampling-pool-closed-20261010",
        _ => "../../target/pixel-sampling-ingress-20261010",
    });
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tb.v"), tb()).unwrap();
    for native in [false, true] {
        let backend = if native {
            quad_ingress::rtl::Backend::Native
        } else {
            quad_ingress::rtl::Backend::Portable
        };
        let mut source = match profile {
            6 => pixel_sampling_ingress::verilog_work_direct(&slots(), backend),
            5 => pixel_sampling_ingress::verilog(&slots(), backend),
            4 => pixel_sampling_ingress::verilog_factored_checked(&slots(), backend),
            3 => pixel_sampling_ingress::verilog_factored(&slots(), backend),
            2 => pixel_sampling_ingress::verilog_reserved(&slots(), backend),
            1 => pixel_sampling_ingress::verilog_pool_closed(&slots(), backend),
            _ => pixel_sampling_ingress::verilog_checked(&slots(), backend),
        }
        .unwrap();
        if profile == 5 {
            assert_eq!(
                source,
                pixel_sampling_ingress::verilog_work_direct(&slots(), backend).unwrap()
            );
        }
        if native {
            source = "`define GPU_V2_GOWIN_DSP\n".to_owned() + &source;
        }
        let file = format!("native-{native}.v");
        std::fs::write(dir.join(&file), source).unwrap();
        let mut compile = std::process::Command::new(
            std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()),
        );
        compile
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
        if native {
            compile.arg(
                std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        let compiled = compile.args([&file, "tb.v"]).output().unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let result =
            std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()))
                .current_dir(&dir)
                .arg("sim.vvp")
                .output()
                .unwrap();
        std::fs::write(dir.join(format!("native-{native}-run.out")), &result.stdout).unwrap();
        assert!(
            result.status.success(),
            "{native}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        println!(
            "native={native}: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}
