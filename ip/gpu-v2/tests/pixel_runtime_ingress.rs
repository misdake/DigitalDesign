//! Joint actual Quad/Lighting/row Runtime front half. Parent-owned public credit
//! and ordered ROP completion are bounded scoreboards, not a full Runtime join.
use gpu_v2::{
    lighting::{ports::*, sim::counted, LightingProfile, LightingQuantization},
    system::pixel::{
        pixel_runtime_ingress,
        quad_ingress::{self, Begin},
    },
    texture::{
        emu::{
            compact,
            context_store::{self, Raw},
            coordinate::{self, CoordinateEmu},
            coordinate_store::Owner,
            derivative::{self, DerivativeEmu},
            lod::LodEmu,
        },
        sim::staged::bound::serial,
    },
};
use std::fmt::Write;
const QUADS: usize = 48;
const MAX_EDGES: usize = 10000;

fn context(bank: bool) -> LightingContext {
    LightingContext {
        epoch: if bank { 202 } else { 101 },
        material: Material {
            shininess_code: if bank { 16 } else { 3 },
            ..Material::default()
        },
        light: Light {
            direction: if bank { [0, 16384, 0] } else { [0, 0, 16384] },
            ..Light::default()
        },
        projection: Projection::default(),
    }
}
fn pixel(q: usize, lane: usize) -> CompactPixelInput {
    CompactPixelInput {
        normal: [
            [0, 0, 0],
            [0, 0, 1024],
            [-2048, 2047, 1023],
            [-419, 31, 1001],
        ][(q + lane) % 4],
        ndc: [[-16384, 16384], [0, 0], [16384, -16384], [731, -9001]][lane],
    }
}
pub(crate) fn tb() -> String {
    let (dc, lc, cc) = serial::structural_calendars().unwrap();
    let mut init = String::new();
    let mut live = 0usize;
    let mut expected_light = 0usize;
    let mut expected_c = 0usize;
    for q in 0..QUADS {
        let bank = q >= QUADS / 2;
        let mask = (q % 16) as u8;
        let config = if bank {
            0x0200u32 | (5 << 16) | (6 << 20) | (1 << 24)
        } else {
            213u32 | (3 << 16) | (9 << 20) | (1 << 24) | (2 << 25)
        };
        let begin = Begin {
            mask,
            draw: bank,
            unlit: q % 7 == 4,
            untextured: false,
            force_coarsest: q % 7 == 0,
            origin: (q * 173) as u16,
        };
        let slot = live % 32;
        if mask != 0 {
            live += 1;
        }
        let raw = Raw {
            bias: config as i16,
            force_coarsest: begin.force_coarsest,
            header: derivative::Header {
                quad: (slot % 16) as u8,
                mask,
                slot: ((config >> 16) & 15) as u8,
                max_n: ((config >> 20) & 15) as u8,
                has_mip: true,
                filter: ((config >> 25) & 3) as u8,
            },
        };
        let codes = [
            -131072, -65537, -32769, -1, 0, 1, 32767, 65535, 65536, 131071,
        ];
        let uv = std::array::from_fn(|i| match q % 3 {
            0 => -65537 + (i as i64 / 2) * (13 + q as i64 * 7) + (i as i64 % 2) * 311,
            1 => codes[q % codes.len()],
            _ => codes[(q * 3 + i * 7) % codes.len()],
        });
        writeln!(init,"begins[{q}]=23'h{:x};raws[{q}]=36'h{:x};slots[{q}]={slot};masks[{q}]=4'h{mask:x};unlit[{q}]={};",begin.packed(),raw.pack().unwrap(),usize::from(begin.unlit)).unwrap();
        for row in 0..8 {
            let basic = if row % 2 == 0 {
                ((q * 119731 + row * 713) & 0xffffff) as u32
            } else {
                (55000 - q * 191 - row * 23) as u32
            };
            writeln!(
                init,
                "basics[{}]=32'h{basic:x};lights[{}]=36'h{:x};",
                q * 8 + row,
                q * 8 + row,
                pixel(q, row / 2).rows().unwrap()[row % 2]
            )
            .unwrap();
        }
        for lane in 0..4 {
            let word =
                (uv[lane * 2] as u64 & 0x3ffff) | ((uv[lane * 2 + 1] as u64 & 0x3ffff) << 18);
            writeln!(init, "uvs[{}]=36'h{word:x};", q * 4 + lane).unwrap();
        }
        let mut d = DerivativeEmu::new(dc.clone()).unwrap();
        d.tick(
            true,
            Some(derivative::Input {
                uv,
                bias: raw.bias,
                force_coarsest: raw.force_coarsest,
                header: raw.header,
            }),
        )
        .unwrap();
        for _ in 0..2 {
            d.tick(true, None).unwrap();
        }
        let derived = d.output().unwrap().unwrap();
        let mut l = LodEmu::new(lc.clone().with_issue_offset(0).unwrap()).unwrap();
        l.tick(true, Some(derived.into())).unwrap();
        for _ in 0..3 {
            l.tick(true, None).unwrap();
        }
        let ready = l.output().unwrap().unwrap();
        writeln!(
            init,
            "derived[{q}]=53'h{:x};ready_rows[{q}]=83'h{:x};",
            context_store::pack_derived(derived.into()).unwrap(),
            context_store::pack_ready(ready).unwrap()
        )
        .unwrap();
        for lane in 0..4 {
            let mut c = CoordinateEmu::new(cc.clone()).unwrap();
            c.tick(
                true,
                Some(coordinate::Input {
                    uv: [derived.uv[lane * 2], derived.uv[lane * 2 + 1]],
                    shift: ready.context.shift,
                    nearest: ready.context.nearest,
                    halve: ready.context.halve,
                    side: ready.context.side,
                }),
            )
            .unwrap();
            c.tick(true, None).unwrap();
            let owner = Owner {
                parents: ready.context.parents,
                nearest: ready.context.nearest,
                levels: ready.context.levels,
                slot: ready.slot,
                key: ready.quad * 4 + lane as u8,
                last_fine: ready.context.last_fine,
            };
            let ctx = context(bank);
            let light = counted::evaluate_with_config(
                pixel(q, lane).expanded().unwrap(),
                ctx.material,
                ctx.light,
                ctx.projection,
                2048,
                counted::Config {
                    rsqrt_q13: true,
                    ..counted::Config::lit_queue_resource_profile(
                        LightingProfile::Fast,
                        LightingQuantization::CompensatedFloor,
                    )
                },
            )
            .unwrap()
            .output;
            writeln!(
                init,
                "owners[{}]=38'h{:x};results[{}]=36'h{:x};gs[{}]=9'd{};hs[{}]=9'd{};",
                q * 4 + lane,
                owner.pack().unwrap(),
                q * 4 + lane,
                compact::pack_result(c.output().unwrap().unwrap()),
                q * 4 + lane,
                light.g,
                q * 4 + lane,
                light.h
            )
            .unwrap();
            if mask & (1 << lane) != 0 {
                expected_c += 1;
                if !begin.unlit {
                    expected_light += 1;
                }
            }
        }
    }
    TESTBENCH
        .replace("// GOLDEN", &init)
        .replace("QUADS", &QUADS.to_string())
        .replace("MAX_EDGES", &MAX_EDGES.to_string())
        .replace("EXPECTED_LIVE", &live.to_string())
        .replace("EXPECTED_LIGHT", &expected_light.to_string())
        .replace("EXPECTED_C", &expected_c.to_string())
}

const TESTBENCH: &str = r#"module tb;
reg clk=0,reset=1,ce=0,in_valid=0;
reg[22:0]in_begin=0;reg[31:0]in_basic=0;reg[35:0]in_light=0,in_uv=0;
wire in_ready,advance,preflight_bad,fault;wire[2:0]in_row;
wire published;wire[4:0]published_slot;wire[22:0]published_begin;
wire basic_write;wire[7:0]basic_write_address;wire[23:0]basic_write_data;
reg basic_read=0;reg[7:0]basic_read_address=0;wire basic_return_valid;wire[23:0]basic_return_data;wire basic_collision;
reg rop_row7_accepted=0;reg[4:0]rop_slot=0;wire[4:0]retire_slot;wire[5:0]global_used;wire[1:0]light_used;wire[3:0]sample_used;
reg public_credit=1;
wire[26:0]draw_config;
wire sample_valid,sample_ready,sample_fire;wire[1:0]sample_row;wire[10:0]sample_descriptor;wire[35:0]sample_data,raw_metadata;
assign draw_config=sample_descriptor[9]?27'h1650200:27'h59300d5;
wire raw_write,committed;wire[2:0]context_slot,phase;
wire sample_out_valid;reg sample_out_ready=0;wire[37:0]sample_out_owner;wire[35:0]sample_out_result;
wire[72:0]context_state;wire[4:0]c_occupied,c_completed;
wire d_issue,l_issue,c_issue,d_return,l_return,c_return,uv_write,uv_read;wire[4:0]uv_write_address,uv_read_address;
wire light_bank;reg retire_valid=0,retire_bank=0;
wire[15:0]context_epoch=light_bank?16'd202:16'd101;
wire[1:0]context_mode=3;wire[4:0]context_code=light_bank?5'd16:5'd3;
wire signed[15:0]light_x=0,light_y=light_bank?16'sd16384:16'sd0,light_z=light_bank?16'sd0:16'sd16384;
wire[8:0]ambient=32,directional=224;
wire signed[15:0]ray_x=-8192,ray_y=-8192,ray_k=8192;
wire context_wait,context_accepted,pixel_accepted,out_valid;reg out_ready=0;
wire[6:0]out_key;wire out_quad_last;wire[15:0]out_epoch;wire[8:0]out_g,out_h;
gpu_v2_pixel_runtime_ingress dut(.*);
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
reg[22:0]begins[0:QUADS-1];reg[35:0]raws[0:QUADS-1],lights[0:QUADS*8-1],uvs[0:QUADS*4-1],results[0:QUADS*4-1];
reg[31:0]basics[0:QUADS*8-1];reg[3:0]masks[0:QUADS-1];reg unlit[0:QUADS-1];integer slots[0:QUADS-1];
reg[52:0]derived[0:QUADS-1];reg[82:0]ready_rows[0:QUADS-1];reg[37:0]owners[0:QUADS*4-1];reg[8:0]gs[0:QUADS*4-1],hs[0:QUADS*4-1];
integer global_q[0:31],owner_ctx[0:7],public_q[0:15],public_left[0:15],seen_lane[0:QUADS-1],expected[0:QUADS*4-1];
reg light_done[0:31],sample_done[0:31];
integer source_q=0,source_row=0,wall,i,job,slot,key,head=0,tail=0,allocated=0,retired=0,raw_count=0,admit_count=0;
integer di=0,li=0,dr=0,lr=0,ci=0,cr=0,light_outputs=0,empty_quads=0,high_slots=0,wrapped=0;
integer paid_uv=0,paid_basic=0,parallel=0,ctxwait_returns=0,context_loads=0,held_light=0,held_sample=0;
reg previous_uv=0,previous_basic=0;reg[23:0]expected_basic;
reg[72:0]before_context;reg[2:0]before_phase;reg[4:0]before_count,before_ready;
reg before_advance;reg[37:0]before_owner;reg[35:0]before_result;reg before_valid;
task tick_edge;begin clk=1;#2;clk=0;#2;end endtask
initial begin
// GOLDEN
for(i=0;i<32;i=i+1)begin global_q[i]=-1;light_done[i]=0;sample_done[i]=0;end
for(i=0;i<8;i=i+1)owner_ctx[i]=-1;
for(i=0;i<16;i=i+1)begin public_q[i]=-1;public_left[i]=0;end
for(i=0;i<QUADS;i=i+1)seen_lane[i]=0;
#2;tick_edge;reset=0;
for(wall=0;wall<MAX_EDGES;wall=wall+1)begin
 ce=(wall%7!=3)&&!previous_uv&&!previous_basic;
 in_valid=source_q<QUADS;
 if(source_q<QUADS)begin
  in_begin=begins[source_q];in_basic=basics[source_q*8+source_row];in_light=lights[source_q*8+source_row];in_uv=uvs[source_q*4+source_row/2];
 end
 out_ready=wall>160&&wall%11!=8;sample_out_ready=wall>400&&wall%9!=2&&wall%9!=3;
 public_credit=sample_row!=0||public_left[sample_descriptor[3:0]]==0;
 rop_slot=retire_slot;
 rop_row7_accepted=ce&&global_q[retire_slot]>=0&&light_done[retire_slot]&&sample_done[retire_slot];
 basic_read=global_q[retire_slot]>=0&&wall%19==4&&!rop_row7_accepted;
 basic_read_address={retire_slot,3'd2};
 #2;
 if(fault||preflight_bad||basic_collision)$fatal(1,"joint fault %0d Q=%b S=%b",wall,dut.quad_bad,dut.sampling_bad);
 if(advance!==ce)$fatal(1,"joint healthy advance");
 if(in_row!==source_row[2:0])$fatal(1,"Quad row cursor");
 if(basic_return_valid!==previous_basic)$fatal(1,"paid Basic valid");
 if(previous_basic)begin if(basic_return_data!==expected_basic)$fatal(1,"paid Basic data");if(!ce)paid_basic=paid_basic+1;end
 if(previous_uv&&!ce)paid_uv=paid_uv+1;
 if(sample_out_valid)begin
  if(head>=tail||sample_out_owner!==owners[expected[head]]||sample_out_result!==results[expected[head]])$fatal(1,"joint C result %0d",wall);
  if(!advance||!sample_out_ready)held_sample=held_sample+1;
 end
 if(out_valid)begin
  slot=out_key[6:2];job=global_q[slot];key=out_key[1:0];
  if(job<0||unlit[job]||!masks[job][key]||out_g!==gs[job*4+key]||out_h!==hs[job*4+key]||out_epoch!==(job>=24?16'd202:16'd101)||out_quad_last!==((masks[job]>>(key+1))==0))$fatal(1,"joint Lighting result %0d job %0d key %0d g %d/%d h %d/%d",wall,job,key,out_g,gs[job*4+key],out_h,hs[job*4+key]);
  if(!advance||!out_ready)held_light=held_light+1;
  if(advance&&out_ready)begin light_outputs=light_outputs+1;if(out_quad_last)light_done[slot]=1;end
  if(advance&&out_ready&&context_wait)ctxwait_returns=ctxwait_returns+1;
 end
 if(context_accepted)context_loads=context_loads+1;
 if(advance&&sample_out_valid&&sample_out_ready)begin
  job=expected[head]/4;key=slots[job]%16;public_left[key]=public_left[key]-1;
  if(public_left[key]==0)sample_done[slots[job]]=1;
  head=head+1;
 end
 if(advance&&d_return)begin job=owner_ctx[dut.sampling.d_return_slot];if(phase!=3||dut.sampling.held_derived!==derived[job])$fatal(1,"joint D");dr=dr+1;end
 if(advance&&l_return)begin job=owner_ctx[dut.sampling.l_return_slot];if(phase!=0||dut.sampling.held_derived!==derived[job]||dut.sampling.ready_data!==ready_rows[job])$fatal(1,"joint LOD loan");lr=lr+1;end
 if(d_issue)di=di+1;if(l_issue)li=li+1;if(advance&&c_return)cr=cr+1;
 if(c_issue)begin
  job=owner_ctx[dut.sampling.c_slot];key=dut.sampling.c_lane;
  if(job<0||phase[0]||!masks[job][key]||seen_lane[job][key]||dut.sampling.c_owner!==owners[job*4+key])$fatal(1,"joint C issue");
  seen_lane[job]=seen_lane[job]|(1<<key);expected[tail]=job*4+key;tail=tail+1;ci=ci+1;
 end
 if(dut.sampling.reclaim_fire)begin job=owner_ctx[dut.sampling.reclaim_slot];if(job<0||seen_lane[job]!=masks[job])$fatal(1,"joint early release");owner_ctx[dut.sampling.reclaim_slot]=-1;end
 if(sample_fire)begin
  slot=sample_descriptor[4:0];job=global_q[slot];
  if(job<0||sample_data!==uvs[job*4+sample_row]||raw_metadata!==raws[job])$fatal(1,"actual Quad Sample rows");
  if(raw_write)begin
   if(sample_row!=0||owner_ctx[context_slot]>=0||public_left[slot%16]!=0)$fatal(1,"public/context reservation");
   owner_ctx[context_slot]=job;public_q[slot%16]=job;
   public_left[slot%16]=masks[job][0]+masks[job][1]+masks[job][2]+masks[job][3];raw_count=raw_count+1;
  end else if(sample_row==0)$fatal(1,"missing canonical Raw");
  if(committed)begin if(sample_row!=3)$fatal(1,"row3 admit");admit_count=admit_count+1;end
  if(pixel_accepted)parallel=parallel+1;
 end
 if(rop_row7_accepted)begin global_q[rop_slot]=-1;light_done[rop_slot]=0;sample_done[rop_slot]=0;retired=retired+1;end
 if(basic_write)begin
  if(basic_write_address!=={slots[source_q][4:0],source_row[2:0]}||basic_write_data!==basics[source_q*8+source_row][23:0])$fatal(1,"Basic write");
 end
 if(published)begin
  slot=published_slot;
  if(slot!=slots[source_q]||global_q[slot]>=0||published_begin!==begins[source_q])$fatal(1,"global publication");
  global_q[slot]=source_q;light_done[slot]=unlit[source_q];sample_done[slot]=0;allocated=allocated+1;
  if(slot>15)high_slots=high_slots+1;if(source_q>32&&slot<8)wrapped=wrapped+1;
 end
 previous_basic=advance&&basic_read;
 if(previous_basic)expected_basic=basics[global_q[basic_read_address[7:3]]*8+2][23:0];
 previous_uv=uv_read;
 if(in_valid&&in_ready)begin
  if(source_row==7)begin if(masks[source_q]==0)empty_quads=empty_quads+1;source_q=source_q+1;source_row=0;end else source_row=source_row+1;
 end
 before_context=context_state;before_phase=phase;before_count=c_occupied;before_ready=c_completed;before_advance=advance;
 before_valid=sample_out_valid;before_owner=sample_out_owner;before_result=sample_out_result;
 tick_edge;
 if(!before_advance)begin
  if(context_state!==before_context||phase!==before_phase||c_occupied!==before_count||c_completed!==before_ready)$fatal(1,"joint CE0 controls");
  if(before_valid&&(sample_out_owner!==before_owner||sample_out_result!==before_result))$fatal(1,"joint held C");
 end
 if(source_q==QUADS&&allocated==EXPECTED_LIVE&&retired==allocated&&head==tail&&global_used==0&&context_state[9:6]==0&&c_occupied==0)begin
  if(di!=EXPECTED_LIVE||li!=di||dr!=di||lr!=di||raw_count!=di||admit_count!=di||ci!=EXPECTED_C||cr!=ci||head!=ci||light_outputs!=EXPECTED_LIGHT||empty_quads!=3||paid_uv==0||paid_basic==0||parallel==0||high_slots==0||wrapped==0||context_loads!=2||held_light==0||held_sample==0||ctxwait_returns==0)$fatal(1,"joint coverage D%0d C%0d L%0d ctx%0d parallel%0d reuse%0d",di,ci,light_outputs,context_loads,parallel,wrapped);
  $display("PASS joint wall=%0d live=%0d D/LOD=%0d C=%0d Lighting=%0d UVpaidCE0=%0d BasicpaidCE0=%0d parallel=%0d switchDrain=%0d highSlots=%0d wrapped=%0d heldLight=%0d heldC=%0d",wall+1,allocated,di,ci,light_outputs,paid_uv,paid_basic,parallel,ctxwait_returns,high_slots,wrapped,held_light,held_sample);
  $finish;
 end
end
$fatal(1,"joint wall watchdog");
end
initial begin #200000;$fatal(1,"joint time watchdog");end
endmodule
"#;

#[test]
#[ignore = "requires actual Lighting and D/LOD/C RTL plus Gowin primitives"]
fn actual_single_quad_light_and_runtime_rows_qualify() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-runtime-ingress-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tb.v"), tb()).unwrap();
    for native in [false, true] {
        let backend = if native {
            quad_ingress::rtl::Backend::Native
        } else {
            quad_ingress::rtl::Backend::Portable
        };
        let source = pixel_runtime_ingress::verilog(backend).unwrap();
        let source = if native {
            "`define GPU_V2_GOWIN_DSP\n".to_owned() + &source
        } else {
            source
        };
        let name = format!("native-{native}.v");
        std::fs::write(dir.join(&name), source).unwrap();
        let mut command = std::process::Command::new(
            std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()),
        );
        command
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
        if native {
            command.arg(
                std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        let compiled = command.args([&name, "tb.v"]).output().unwrap();
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
            "native={native}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        println!(
            "native={native}: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}
