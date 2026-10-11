//! Real Quad/Lighting/preparation ports; independent counted Group72 goldens.
#[path = "pixel_runtime_ingress.rs"]
mod reference;
use gpu_v2::{
    system::pixel::{pixel_preparation_ingress, quad_ingress},
    texture::{
        ports::{Filter, QuadInput, Slot},
        sim::counted,
    },
};
use std::fmt::Write;

pub(crate) fn preparation_tb() -> String {
    let slots: Vec<_> = (0..16)
        .map(|i| Slot {
            base_address: i << 20,
            has_full_mip: true,
            max_size_log2: if i == 5 { 6 } else { 9 },
            valid: true,
        })
        .collect();
    let mut init = String::new();
    let mut group_count = 0usize;
    let mut live = 0usize;
    for q in 0..48 {
        let mask = (q % 16) as u8;
        let slot = live % 32;
        if mask == 0 {
            continue;
        }
        live += 1;
        let codes = [
            -131072, -65537, -32769, -1, 0, 1, 32767, 65535, 65536, 131071,
        ];
        let uv: [i64; 8] = std::array::from_fn(|i| match q % 3 {
            0 => -65537 + (i as i64 / 2) * (13 + q as i64 * 7) + (i as i64 % 2) * 311,
            1 => codes[q % codes.len()],
            _ => codes[(q * 3 + i * 7) % codes.len()],
        });
        let input = QuadInput {
            quad_id: (slot % 16) as u8,
            mask,
            uv: std::array::from_fn(|lane| {
                std::array::from_fn(|axis| uv[lane * 2 + axis] as f64 / 65536.0)
            }),
            slot: if q >= 24 { 5 } else { 3 },
            material_size_log2: if q >= 24 { 6 } else { 9 },
            filter: if q >= 24 {
                Filter::Nearest
            } else {
                Filter::Trilinear
            },
            lod_bias: if q >= 24 { 2.0 } else { 213.0 / 256.0 },
            force_coarsest: q % 7 == 0,
        };
        let prepared = counted::prepare(&input, &slots).unwrap();
        assert!(!prepared.groups.is_empty());
        for group in prepared.groups {
            writeln!(
                init,
                "group_gold[{group_count}]=72'h{:x};",
                group.pack72().unwrap()
            )
            .unwrap();
            group_count += 1;
        }
    }
    let mut tb = reference::tb()
        .replace("gpu_v2_pixel_runtime_ingress dut(.*);", "gpu_v2_pixel_preparation_ingress dut(.*);")
        .replace("reg sample_out_ready=0;", "wire sample_out_ready;")
        .replace("gpu_v2_pixel_preparation_ingress dut(.*);", &format!("reg packet_enable=0,packet_out_ready=0;wire packet_out_valid,coefficient_accept;wire[71:0]packet_out_payload;wire[4:0]work_reserved,work_count,producer_count,unwritten_count;wire[5:0]group_count,row_count;reg[71:0]group_gold[0:{}];integer group_read=0,held_group=0,peak_reserved=0,peak_work=0;\ngpu_v2_pixel_preparation_ingress dut(.*);", group_count-1))
        .replace("sample_out_ready=wall>400&&wall%9!=2&&wall%9!=3;", "packet_enable=wall>240&&wall%13!=5;packet_out_ready=wall>400&&wall%9!=2&&wall%9!=3;")
        .replace("job=expected[head]/4;key=slots[job]%16;public_left[key]=public_left[key]-1;\n  if(public_left[key]==0)sample_done[slots[job]]=1;", "if(!coefficient_accept)$fatal(1,\"C/Coeff joined transfer\");")
        .replace("&&c_occupied==0)begin", &format!("&&c_occupied==0&&group_read=={group_count}&&work_reserved==0&&work_count==0&&producer_count==0&&group_count==0)begin"))
        .replace("$display(\"PASS joint", "$display(\"PASS preparation groups=%0d peakReserved=%0d peakWork=%0d heldGroups=%0d\",group_read,peak_reserved,peak_work,held_group);\n  $display(\"PASS joint");
    tb = tb.replacen(
        "for(i=0;i<32;i=i+1)begin",
        &(init + "for(i=0;i<32;i=i+1)begin"),
        1,
    );
    let group_check = format!(
        r#"
 if(work_reserved>peak_reserved)peak_reserved=work_reserved;
 if(work_count>peak_work)peak_work=work_count;
 if(packet_out_valid)begin
  if(group_read>={group_count}||packet_out_payload!==group_gold[group_read])$fatal(1,"Group72 numerical/order %0d got%h want%h",group_read,packet_out_payload,group_gold[group_read]);
  if(!advance||!packet_out_ready)held_group=held_group+1;
  if(advance&&packet_out_ready)begin
   if(packet_out_payload[65])begin
    key=packet_out_payload[69:66];job=public_q[key];
    if(job<0||public_left[key]==0)$fatal(1,"Group public lifetime");
    public_left[key]=public_left[key]-1;
    if(public_left[key]==0)sample_done[slots[job]]=1;
   end
   group_read=group_read+1;
  end
 end
 if(coefficient_accept!==(advance&&sample_out_valid&&sample_out_ready))$fatal(1,"C acceptance identity");
"#
    );
    tb.replacen(
        "if(context_accepted)context_loads=context_loads+1;",
        &(group_check + "\n if(context_accepted)context_loads=context_loads+1;"),
        1,
    )
}

#[test]
#[ignore = "requires actual RTL and Gowin primitives"]
fn actual_quad_lighting_preparation_qualify() {
    qualify(false);
}

#[test]
#[ignore = "requires actual RTL and Gowin primitives"]
fn actual_canonical_cost_preparation_qualify() {
    qualify(true);
}

fn qualify(canonical_cost: bool) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(if canonical_cost {
        "../../target/pixel-preparation-canonical-cost-20261010"
    } else {
        "../../target/pixel-preparation-ingress-20261010"
    });
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tb.v"), preparation_tb()).unwrap();
    for native in [false, true] {
        let backend = if native {
            quad_ingress::rtl::Backend::Native
        } else {
            quad_ingress::rtl::Backend::Portable
        };
        let source = if canonical_cost {
            pixel_preparation_ingress::verilog_canonical(backend)
        } else {
            pixel_preparation_ingress::verilog(backend)
        }
        .unwrap();
        let source = if native {
            "`define GPU_V2_GOWIN_DSP\n".to_owned() + &source
        } else {
            source
        };
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
