"""Bounded real shared-SDRAM RTL -> V/T/R/I -> backend -> physical FB replay.

The numerical checker is imported from the independent Stage2 qualification.
This script owns only its fixture/clock/memory/lifecycle test construction.
"""
from pathlib import Path
import argparse, copy, difflib, hashlib, importlib.util, json, os, re, shutil, subprocess, sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'ip/gpu-v2/examples/vliw_integration'
OUT = ROOT / 'target/gpu-vliw-integration-20261018/stage3'
SHA = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()
sys.path.insert(0, str(ROOT/'hardware/vendor/gowin/tests'))
from stream_contract import load_contract, stage_peer


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    sys.modules[name] = m
    spec.loader.exec_module(m)
    return m


def execute(cmd, cwd, log, timeout=300, env=None):
    p = subprocess.run(list(map(str, cmd)), cwd=cwd, env=env, capture_output=True, timeout=timeout)
    Path(log).write_bytes(p.stdout + p.stderr)
    assert p.returncode == 0, (p.stdout + p.stderr).decode(errors='replace')[-9000:]
    return p.stdout.decode(errors='replace')


def wiring_origins(dut):
    """Origins of this emitted RTL, never a whole-repository/docs hash sweep.

    The emitted dependency-closed DUT is authoritative. Origin copies identify
    the input files and active factories; future unrelated Stage4 code is not
    made a dependency merely by sharing the repository.
    """
    src = ROOT / 'ip/gpu-v2/src'
    names = set(re.findall(r'\bmodule\s+(\w+)', dut.read_text()))
    selected = {src / p for p in [
        'bin/gpu-v2-vliw-export.rs', 'frontend/vliw.rs', 'frontend/vliw_dma.rs', 'frontend/vliw_dma.v',
        'system/vliw.rs', 'system/vliw.v', 'system/vliw_sdram.rs', 'system/vliw_sdram.v',
        'system/vliw_memory.rs', 'system/vliw_memory.v', 'system/pixel/vliw_backend.rs',
        'system/pixel/direct_lod.rs', 'system/pixel/postquad_ii4.rs', 'system/pixel/postquad.rs',
        'lighting/ports.rs', 'texture/ports.rs', 'framebuffer/ports.rs']}
    selected.update(p for p in (src / 'frontend/vliw').rglob('*') if p.suffix in ('.v', '.vh'))
    # Include raw source files contributing an actual emitted module, then the
    # Rust include_str factories that consume those files.
    for p in src.rglob('*.v'):
        if names.intersection(re.findall(r'\bmodule\s+(\w+)', p.read_text())):
            selected.add(p)
    for p in src.rglob('*.rs'):
        for name in re.findall(r'include_str!\(\s*"([^"]+)"\s*\)', p.read_text()):
            if (p.parent / name).resolve() in selected:
                selected.add(p)
    return {str(p.relative_to(ROOT)).replace('\\', '/'): SHA(p) for p in sorted(selected)}


def snapshot_origins(out, origins):
    for rel, h in origins.items():
        p = ROOT / rel; assert SHA(p) == h, ('wiring changed during export', rel)
        d = out / 'wiring-source' / rel; d.parent.mkdir(parents=True, exist_ok=True); shutil.copy2(p, d)
    (out / 'wiring-source-manifest.json').write_text(json.dumps(origins, indent=2))


def changed_origins(out, origins):
    changes = {}
    for rel, h in origins.items():
        p = ROOT / rel
        if SHA(p) != h:
            old = (out / 'wiring-source' / rel).read_text().splitlines(True)
            new = p.read_text().splitlines(True)
            diff = ''.join(difflib.unified_diff(old, new, fromfile='tested/' + rel, tofile='live/' + rel))
            changes[rel] = dict(tested_sha256=h, live_sha256=SHA(p), diff=diff)
    return changes


MC = r'''
wire logic_clk=clk,sdram_clk=phy_clk;
wire O_sdram_clk,O_sdram_cke,O_sdram_cs_n,O_sdram_cas_n,O_sdram_ras_n,O_sdram_wen_n;
wire[3:0]O_sdram_dqm;wire[10:0]O_sdram_addr;wire[1:0]O_sdram_ba;wire[31:0]IO_sdram_dq;
`include "ports.vh"
GowinSdramCombination mc(.*);
integer refreshes;
LabPinModel #(.PERIOD_NS(9.259259),.CAS_LATENCY(3),.RCD_MIN(3),.RP_MIN(3),.RETURN_MIN(1),.RETURN_MAX(4)) pin(
 .sclk(O_sdram_clk),.reset(reset),.cke(O_sdram_cke),.cs(O_sdram_cs_n),
 .ras(O_sdram_ras_n),.cas(O_sdram_cas_n),.we(O_sdram_wen_n),.dqm(O_sdram_dqm),
 .a(O_sdram_addr),.ba(O_sdram_ba),.dq(IO_sdram_dq),.cycle(),.refreshes(refreshes));
function[20:0]native_word;input integer bytes;integer w;
begin w=bytes/4;native_word=((w>>5)&3)*524288+(w&31)+((w>>7)<<5);end endfunction
reg descriptor_valid=0,descriptor_page=0,completion_ready=0;
wire descriptor_ready,completion_valid,completion_publish,completion_page,start_ready;
reg[7:0]descriptor_tag=0;wire[7:0]completion_tag;wire[3:0]completion_error;
reg[21:0]config_source=0,metadata_source=0,payload_source=0;
reg[10:0]config_bytes=0,payload_bytes=0;
reg[127:0]jobs[0:4];reg[63:0]expected_dma[0:2559];reg expected_valid[0:2559],dma_seen[0:2559];
integer observed_dma[0:4];integer active_job_id=-1,job_admissions=0,job_completions=0,host_completions=0;
integer dma_ce0=0,inactive_writes=0,descriptor_conflict=0,completion_starts=0;
integer config_blocked_backend=0;
integer requests=0,ro_requests=0,fb_reads=0,fb_writes=0,ro_beats=0,fb_r_beats=0,fb_w_beats=0;
integer paid_ce0=0,raw_seed_addr,seed_handle,seed_rc,seed_count=0;
reg[31:0]seed_value,dump_value;reg[4:0]completion_seen=0;
reg descriptor_taken=0,force_pause=0,forbid_start=0,flush_config_probe=0;
wire dma_we=dut.dma_we,dma_ready=dut.dma_ready;wire[9:0]dma_addr=dut.dma_addr;wire[63:0]dma_data=dut.dma_data;
always @(posedge clk)if(!reset)begin
 if(descriptor_valid&&descriptor_ready)begin
  descriptor_taken=1;active_job_id=descriptor_tag;job_admissions=job_admissions+1;
  if(forbid_start)descriptor_conflict=descriptor_conflict+1;
 end
 if(dut.start_fire&&forbid_start)$fatal(1,"start accepted with same-page descriptor refresh");
 if(dut.start_fire&&completion_valid&&!completion_ready)completion_starts=completion_starts+1;
 if(descriptor_valid&&config_bytes!=0)begin
  if(!dut.config_drained&&descriptor_ready)$fatal(1,"configuration accepted before strict GPU drain");
  if(flush_config_probe&&!flush_complete&&descriptor_ready)$fatal(1,"configuration accepted before full flush completion");
  if(frontend_idle&&!dut.compute_idle)config_blocked_backend=config_blocked_backend+1;
 end
 if(dma_we)begin
  if(!dma_ready||active_job_id<0||dma_addr>=512)$fatal(1,"DMA fixed sink violation");
  if(!expected_valid[active_job_id*512+dma_addr]||dma_seen[active_job_id*512+dma_addr]||
      dma_data!==expected_dma[active_job_id*512+dma_addr])$fatal(1,"DMA raw source/destination job%0d row%0d got%h",active_job_id,dma_addr,dma_data);
  dma_seen[active_job_id*512+dma_addr]=1;observed_dma[active_job_id]=observed_dma[active_job_id]+1;
  if(!ce)dma_ce0=dma_ce0+1;
  if(!frontend_idle&&dut.job_page!=dut.active_page)inactive_writes=inactive_writes+1;
 end
 if(completion_valid&&!completion_seen[completion_tag])begin
  if(completion_tag!=active_job_id||completion_page!==jobs[completion_tag][88]||
      observed_dma[completion_tag]!=jobs[completion_tag][98:89])$fatal(1,"completion identity/count");
  if(completion_tag==1||completion_tag==4)begin
   if(completion_publish||completion_error!=1)$fatal(1,"invalid refresh published");
  end else if(!completion_publish||completion_error)$fatal(1,"valid DMA failed");
  completion_seen[completion_tag]=1;job_completions=job_completions+1;
 end
 if(completion_valid&&completion_ready)host_completions=host_completions+1;
 if(ro_request_valid&&ro_request_ready)begin requests=requests+1;ro_requests=ro_requests+1;end
 if(fb_r_request_valid&&fb_r_request_ready)begin requests=requests+1;fb_reads=fb_reads+1;end
 if(fb_w_request_valid&&fb_w_request_ready)begin requests=requests+1;fb_writes=fb_writes+1;end
 if(ro_response_valid)begin ro_beats=ro_beats+1;if(!ce)paid_ce0=paid_ce0+1;end
 if(fb_r_response_valid)begin fb_r_beats=fb_r_beats+1;if(!ce)paid_ce0=paid_ce0+1;end
 if(fb_w_data_ready)begin fb_w_beats=fb_w_beats+1;if(!ce)paid_ce0=paid_ce0+1;end
 if((ro_response_valid&&ro_response_error)||(fb_r_response_valid&&fb_r_response_error)||
    (fb_w_response_valid&&fb_w_response_error))$fatal(1,"actual shared MC error");
end
'''

PORTS = r'''
wire ro_request_valid,ro_request_ready,ro_response_valid,ro_response_last,ro_response_error;
wire[21:0]ro_address;wire[1:0]ro_count;wire[63:0]ro_data;
wire fb_r_request_valid,fb_r_request_ready,fb_r_response_valid,fb_r_response_last,fb_r_response_error;
wire[21:0]fb_r_address;wire[1:0]fb_r_count;wire[63:0]fb_r_data;
wire fb_w_request_valid,fb_w_request_ready,fb_w_response_valid,fb_w_response_last,fb_w_response_error,fb_w_data_ready;
wire[21:0]fb_w_address;wire[1:0]fb_w_count;wire[63:0]fb_w_data;
'''

TICK = r'''
task tick;begin
 @(negedge clk);rng=rng*1664525+1013904223;
 ce=!force_pause&&(!STRESS||rng[3:0]!=0);output_ready=!STRESS||rng[6:4]!=0;
 #1;frozen={dut.core.phase,dut.core.front.interpolator.compute.valid_pipe,dut.core.front.raster.w_pc,dut.core.back.front.quad.allocated,dut.core.back.front.quad.retired};
 @(posedge clk);#2;
 if(!reset&&!ce)begin
  if(frozen!=={dut.core.phase,dut.core.front.interpolator.compute.valid_pipe,dut.core.front.raster.w_pc,dut.core.back.front.quad.allocated,dut.core.back.front.quad.retired})$fatal(1,"CE0 architectural hold");
  ce0_checks=ce0_checks+1;
 end
end endtask
task present_job;input integer n;input conflicting_start;begin
 descriptor_tag=n;descriptor_page=jobs[n][88];config_source=jobs[n][21:0];metadata_source=jobs[n][43:22];payload_source=jobs[n][65:44];
 config_bytes=jobs[n][76:66];payload_bytes=jobs[n][87:77];
 slot=descriptor_page;start=conflicting_start;forbid_start=conflicting_start;
 descriptor_taken=0;descriptor_valid=1;while(!descriptor_taken)tick();descriptor_valid=0;start=0;forbid_start=0;
end endtask
task wait_completion;input integer n;begin
 while(!completion_valid||completion_tag!=n)tick();
 repeat(3)tick();
end endtask
task acknowledge;begin
 completion_ready=1;tick();while(completion_valid)tick();completion_ready=0;
end endtask
task prepare_task;input integer taskno,vn,tn,pn;input use_slot;
reg[1023:0]path;begin
 while(!meshlet_ready)tick();task_id=taskno;nv=vn;nt=tn;np=pn;slot=use_slot;vi=0;ti=0;pi=0;saw_done=0;
 $sformat(path,"task%0d/vertices.hex",taskno);$readmemh(path,vertices,0,nv*5-1);
 $sformat(path,"task%0d/admissions.hex",taskno);$readmemh(path,admissions,0,nt-1);
 $sformat(path,"task%0d/descriptors.hex",taskno);$readmemh(path,descriptors,0,np-1);
 $sformat(path,"task%0d/words.hex",taskno);$readmemh(path,words,0,np*16-1);
 expect_task(taskno,np);
end endtask
task launch_task;begin
 start=1;start_taken=0;while(!start_taken)tick();start=0;
end endtask
task finish_task;begin
 while(!saw_done)tick();
 if(vi!=nv*5||ti!=nt||published_vertices!=nv)$fatal(1,"task computation counts");
end endtask
'''

INITIAL = r'''
initial begin
 fd=$fopen("front.txt","w");bd=$fopen("back.txt","w");
 $readmemh("jobs.hex",jobs);$readmemh("expected-dma.hex",expected_dma);$readmemh("expected-valid.hex",expected_valid);
 for(j=0;j<2560;j=j+1)dma_seen[j]=0;
 for(j=0;j<5;j=j+1)observed_dma[j]=0;
 seed_handle=$fopen("seed.txt","r");if(!seed_handle)$fatal(1,"seed image missing");
 while(!$feof(seed_handle))begin seed_rc=$fscanf(seed_handle,"%h %h",raw_seed_addr,seed_value);
  if(seed_rc==2)begin pin.seed_word(native_word(raw_seed_addr),seed_value);seed_count=seed_count+1;end
 end $fclose(seed_handle);
 tick();tick();reset=0;
 // A successful page is later invalidated by a failed refresh. Present a
 // same-page start on the refresh acceptance edge: it must never race through.
 present_job(0,0);wait_completion(0);acknowledge();
 present_job(1,1);wait_completion(1);slot=1;start=1;
 repeat(12)begin tick();if(start_ready||start_taken||dut.page_valid[1])$fatal(1,"failed page became executable");end
 start=0;acknowledge();
 // All initial configuration/payload returns are paid while math CE is zero.
 force_pause=1;present_job(2,0);wait_completion(2);force_pause=0;
 @PREP0@
 launch_task();repeat(16)tick();
 if(!completion_valid||completion_ready)$fatal(1,"success page did not start before host completion ACK");
 acknowledge();
 // The other page prefetch runs concurrently with active page0 computation.
 present_job(3,0);wait_completion(3);finish_task();
 @PREP1@
 launch_task();repeat(16)tick();acknowledge();
 // Manufacture a backend-only maintenance window rather than assuming the
 // last triangle necessarily outlives its backend. Configuration must wait
 // through the complete flush, including gaps between physical writebacks.
 finish_task();while(!idle)tick();flush_request=1;
 while(!dut.core.back.sink.cache.mt_active&&!flush_complete)tick();
 flush_request=0;flush_config_probe=1;
 // A config request waits across backend drain even after the frontend ends.
 // Its oversized segment must fail before any physical read or local write,
 // and conservatively invalidate both pages without touching the cached FB.
 present_job(4,0);if(!flush_complete)$fatal(1,"configuration bypassed flush sequencer");flush_config_probe=0;wait_completion(4);
 repeat(3)begin tick();if(dut.page_valid!==2'b00)$fatal(1,"failed config kept an executable page");end
 start=1;slot=0;repeat(4)begin tick();if(start_ready||start_taken)$fatal(1,"page0 starts after failed config");end
 slot=1;repeat(4)begin tick();if(start_ready||start_taken)$fatal(1,"page1 starts after failed config");end
 start=0;acknowledge();finish_task();
 inputs_done=1;while(!idle)tick();flush_request=1;while(!flush_complete)tick();
 ff=$fopen("memory.mem","w");
 for(j=0;j<BYTES;j=j+4)begin
  if(j<TEX)raw_seed_addr=32'h1000+j;else if(j<TEX+FB)raw_seed_addr=32'h100000+j-TEX;else raw_seed_addr=32'h140000+j-TEX-FB;
  dump_value=pin.physical_word(native_word(raw_seed_addr));
  $fdisplay(ff,"%h\n%h\n%h\n%h",dump_value[7:0],dump_value[15:8],dump_value[23:16],dump_value[31:24]);
 end $fclose(ff);
 if(job_admissions!=5||job_completions!=5||host_completions!=5||dma_ce0==0||inactive_writes==0||
    descriptor_conflict!=1||completion_starts!=2||ro_requests==0||fb_reads==0||fb_writes==0||refreshes==0||(@REQUIREWAIT@&&config_blocked_backend==0))
  $fatal(1,"shared integration coverage jobs%0d/%0d/%0d CE0dma%0d inactive%0d conflict%0d starts%0d physical%0d/%0d/%0d refresh%0d",job_admissions,job_completions,host_completions,dma_ce0,inactive_writes,descriptor_conflict,completion_starts,ro_requests,fb_reads,fb_writes,refreshes);
 $display("PASS SDRAM FULL cycles=%0d CE=%0d Q=%0d published=%0d empty=%0d light=%0d sample=%0d final=%0d commits=%0d RO=%0d FBread=%0d FBwrite=%0d CE0returns=%0d CE0dma=%0d inactiveDMA=%0d startsBeforeACK=%0d configBackendWait=%0d refresh=%0d seed=%0d",cycle,en,sequence_id,qpub,skips,nlight,nsample,nfinal,commits,ro_requests,fb_reads,fb_writes,paid_ce0,dma_ce0,inactive_writes,completion_starts,config_blocked_backend,refreshes,seed_count);
 $fclose(fd);$fclose(bd);$finish;
end
initial begin #@MAXTIME@;$fatal(1,"HDL bounded timeout");end
endmodule
'''


def bench(V, metas, stress, max_cycles, require_dirty_flush_wait=True):
    base = (HERE / 'stage2_tb.v').read_text()
    declarations = base[:base.index('gpu_v2_vliw dut(.*);')]
    declarations = declarations.replace('reg clk=0,reset=1', 'reg clk=0,reset=1')
    declarations = declarations.replace(',dma_we=0', '')
    declarations = declarations.replace('reg[9:0]dma_addr=0;reg[63:0]dma_data=0;wire dma_ready;', '')
    declarations = declarations[:declarations.index('wire mem_req_valid')]
    declarations += '''reg controller_clk=0,phy_clk=0;
always #4.629629 controller_clk=~controller_clk;
always @(posedge controller_clk)clk=~clk;
initial begin #12.430556;forever begin phy_clk=~phy_clk;#4.629629;end end
'''
    declarations += PORTS + '\n' + MC + '\ngpu_v2_vliw_sdram dut(.*);\n'
    common = base[base.index('localparam STRESS='):base.index('function integer mi;')]
    # Avoid duplicate names now owned by the physical bus/protocol checker.
    common = common.replace(',requests=0,paid_ce0=0', '')
    declarations += common
    monitor = base[base.index('always @(posedge clk)begin'):base.index('task tick;')]
    monitor = monitor.replace('dut.', 'dut.core.')
    monitor = monitor.replace('start_taken=ce&&!reset&&start&&meshlet_ready;', 'start_taken=ce&&!reset&&start&&start_ready;')
    result = declarations + monitor + TICK + INITIAL
    replacements = dict(STRESS=int(stress), MAXC=max_cycles, BYTES=len(V.initial_memory()), MAXTIME=int(max_cycles * 21 + 200000), REQUIREWAIT=int(require_dirty_flush_wait),
        PREP0=f'prepare_task(0,{metas[0]["vertices"]},{metas[0]["triangles"]},{metas[0]["packets"]},0);',
        PREP1=f'prepare_task(1,{metas[1]["vertices"]},{metas[1]["triangles"]},{metas[1]["packets"]},1);')
    for k, v in replacements.items(): result = result.replace('@' + k + '@', str(v))
    assert not re.search('@[A-Z0-9]+@', result)
    return result


def dma_fixture(out, V):
    maps = [{int(a): int(v, 16) for a, v in (l.split() for l in (out / f'task{i}/dma.txt').read_text().splitlines())} for i in range(2)]
    assert {a: v for a, v in maps[0].items() if a >= 320} == {a: v for a, v in maps[1].items() if a >= 320}
    seed = {}
    b = V.initial_memory()
    for off in range(0, len(b), 4):
        a = 0x1000 + off if off < V.TEX else (0x100000 + off - V.TEX if off < V.TEX + V.FB else 0x140000 + off - V.TEX - V.FB)
        seed[a] = int.from_bytes(b[off:off + 4], 'little')
    expected, valid, jobs = [0] * 2560, [0] * 2560, []
    for job, task, config, bad in [(0, 1, False, False), (1, 1, False, True), (2, 0, True, False), (3, 1, False, False), (4, 1, True, True)]:
        m = maps[task];page = task;payload = sorted(a for a in m if page * 128 <= a < page * 128 + 128)
        length = (max(payload) - page * 128 + 1) * 8
        base = 0x40000 + job * 0x1000; cfg, meta, pay = base, base + 0x80, base + 0x100
        words = {}
        if config:
            for a in range(320, 336): words[a] = m[a]
        for a in range(256 + 4 * page, 260 + 4 * page): words[a] = m[a]
        for a in payload: words[a] = m[a]
        for a, value in words.items():
            source = cfg + (a - 320) * 8 if a >= 320 else meta + (a - 256 - 4 * page) * 8 if a >= 256 else pay + (a - page * 128) * 8
            seed[source], seed[source + 4] = value & 0xffffffff, value >> 32
            if not bad: expected[job * 512 + a], valid[job * 512 + a] = value, 1
        # Native tail padding is input memory, never a fabricated response beat.
        for a in range(pay, pay + ((length + 31) // 32) * 32, 4): seed.setdefault(a, 0)
        cfg_bytes = 520 if job == 4 else 128 if config else 0
        pay_bytes = 0 if job == 4 else 1025 if bad else length
        jobs.append((cfg // 2) | ((meta // 2) << 22) | ((pay // 2) << 44) | (cfg_bytes << 66) | (pay_bytes << 77) | (page << 88) | ((0 if bad else len(words)) << 89))
    (out / 'seed.txt').write_text(''.join(f'{a:08x} {v:08x}\n' for a, v in sorted(seed.items())))
    (out / 'expected-dma.hex').write_text(''.join(f'{v:016x}\n' for v in expected))
    (out / 'expected-valid.hex').write_text(''.join(f'{v:x}\n' for v in valid))
    (out / 'jobs.hex').write_text(''.join(f'{v:032x}\n' for v in jobs))


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--tag', required=True)
    ap.add_argument('--mode', choices=['portable', 'native', 'both'], default='both')
    ap.add_argument('--stress', action='store_true')
    ap.add_argument('--native-rabbit', action='store_true', help='Native profile uses two complete rabbit meshlets; portable retains rabbit plus box.')
    ap.add_argument('--max-cycles', type=int, default=1200000)
    a = ap.parse_args(); assert re.fullmatch('[A-Za-z0-9_.-]+', a.tag)
    out = OUT / a.tag; out.mkdir(parents=True, exist_ok=False)
    V = load('stage3_reuse_stage2_gold', HERE / 'verify_stage2.py')
    O = load('stage3_independent_front_math', HERE / 'reference/scene/oracle.py')
    O = load('current_transport', HERE / 'transport.py').adapt_oracle(O)
    fixtures = json.loads((HERE / 'reference/uv-inputs.json').read_text())
    instances = [copy.deepcopy(next(x for x in fixtures['scenes'][0]['meshlets'] if x['name'] == name)) for name in ['bunny_patch_1', 'box_x_minus']]
    packets, metas = [], []
    for i, v in enumerate(instances):
        v.update(slot=i, cfg_tex_w_log2=6, cfg_tex_h_log2=6, cfg_max_mip=6)
        meta, ps = O.prepare(v, out / f'task{i}'); packets += ps; metas.append(meta)
    (out / 'input-contract.json').write_text(json.dumps(dict(instances=instances, note='Original real V96/refs/normals; I texture logs6 match actual64x64 mip fixture.'), ensure_ascii=False, indent=2))
    dma_fixture(out, V)
    env = os.environ.copy();env['CARGO_TARGET_DIR'] = str(OUT / 'cargo-stage3-validation')
    cargo = Path.home() / '.cargo/bin/cargo.exe'
    execute([cargo, 'build', '--release', '-p', 'gpu-v2', '--bin', 'gpu-v2-vliw-export', '--example', 'vliw_integration'], ROOT, out / 'build.log', env=env)
    execute([cargo, 'test', '-p', 'gpu-v2', '--test', 'host', 'vliw_sdram::export_real_shared_sdram_peer', '--', '--nocapture'], ROOT, out / 'mc-export.log', env=env)
    gold_exe = Path(env['CARGO_TARGET_DIR']) / 'release/examples/vliw_integration.exe'
    exporter = Path(env['CARGO_TARGET_DIR']) / 'release/gpu-v2-vliw-export.exe'
    snapshots = [Path(__file__), ROOT / 'ip/gpu-v2/tests/host/vliw_sdram.rs', HERE / 'verify_stage2.py', HERE / 'transport.py', HERE / 'stage2_tb.v', HERE / 'main.rs']
    snapshots += [p for p in (HERE / 'reference').rglob('*') if p.is_file() and p.suffix in ('.py', '.json', '.rs', '.v')]
    sources = {str(p.relative_to(ROOT)).replace('\\', '/'): SHA(p) for p in snapshots}
    reports = []; origins = None
    for mode in (['portable', 'native'] if a.mode == 'both' else [a.mode]):
        run = out / mode; run.mkdir()
        profile_instances = instances
        profile_packets, profile_metas = packets, metas
        if mode == 'native' and a.native_rabbit:
            profile_instances = [copy.deepcopy(instances[0]), copy.deepcopy(instances[0])]
            profile_packets, profile_metas = [], []
            for i, v in enumerate(profile_instances):
                v['slot'] = i
                meta, ps = O.prepare(v, run / f'task{i}'); profile_packets += ps; profile_metas.append(meta)
            dma_fixture(run, V)
        else:
            for i in range(2): shutil.copytree(out / f'task{i}', run / f'task{i}')
            for name in ['seed.txt', 'expected-dma.hex', 'expected-valid.hex', 'jobs.hex']: shutil.copy2(out / name, run / name)
        (run / 'input-contract.json').write_text(json.dumps(dict(instances=profile_instances), indent=2))
        stage_peer(OUT/'mc-peer',run)
        execute([exporter, run / 'dut.v', 'sdram'] + (['portable'] if mode == 'portable' else []), ROOT, run / 'export.log', env=env)
        if origins is None:
            origins = wiring_origins(run / 'dut.v'); snapshot_origins(out, origins)
        else:
            assert not changed_origins(out, origins), 'wiring source changed between portable/native exports'
        # Repeating the exact rabbit rejects the second draw by LESS and may
        # leave every cache line clean. The portable box profile must pay a
        # dirty flush; both profiles retain the strict acceptance assertion.
        require_dirty_flush_wait = not (mode == 'native' and a.native_rabbit)
        (run / 'tb.v').write_text(bench(V, profile_metas, a.stress, a.max_cycles, require_dirty_flush_wait))
        cmd = [env['IVERILOG_EXE'], '-g2012', '-D__ICARUS__', '-I', run, '-s', 'tb', '-o', run / 'sim.vvp']
        if mode == 'native': cmd += [Path(env['GOWIN_HOME']) / 'IDE/simlib/gw2a/prim_sim.v']
        else: cmd += ['-DSTREAM_STATIC_INPUT_DELAY']
        cmd += sorted(run.glob('*.v'))
        execute(cmd, run, run / 'compile.log', 180, env=env)
        log = execute([env['VVP_EXE'], run / 'sim.vvp'], run, run / 'run.log', 600, env=env)
        assert 'PASS SDRAM FULL' in log
        counts = V.audit(run, profile_packets, O, gold_exe)
        report = dict(status='pass', mode=mode, stress=a.stress, counts=counts, log=log,
            source_sha256=SHA(run / 'dut.v'), front_sha256=SHA(run / 'front.txt'), back_sha256=SHA(run / 'back.txt'), memory_sha256=SHA(run / 'memory.mem'),
            rtl_sources={p.name: SHA(p) for p in run.iterdir() if p.suffix in ('.v','.vh')},
            fixtures={p.name: SHA(p) for p in run.iterdir() if p.name in ('jobs.hex','seed.txt','expected-dma.hex','expected-valid.hex')},
            primitive_sha256=SHA(Path(env['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v') if mode=='native' else None,
            wiring_source_manifest_sha256=SHA(out/'wiring-source-manifest.json'),
            max_cycles=a.max_cycles, scenes=[v['name'] for v in profile_instances], require_dirty_flush_wait=require_dirty_flush_wait)
        (run / 'receipt.json').write_text(json.dumps(report, indent=2)); reports.append(report); print(json.dumps(report), flush=True)
    if len(reports) == 2 and not a.native_rabbit:
        # Memory timing is identical across portable/native primitive models.
        for key in ['counts', 'front_sha256', 'back_sha256', 'memory_sha256']: assert reports[0][key] == reports[1][key], key
    for p, h in sources.items(): assert SHA(ROOT / p) == h, ('checker changed during run', p)
    changes = changed_origins(out, origins)
    if changes: (out/'live-source-changes.json').write_text(json.dumps(changes, indent=2))
    receipt = dict(status='pass', runs=reports, checker_sha256=sources,
        wiring_source_manifest_sha256=SHA(out/'wiring-source-manifest.json'), live_origin_changes=changes,
        boundary='Full real input pages through actual shared SDRAM arbiter/bridge/controller RTL and independent SDRAM pin model; V/T/R/I + producer pool + Lighting/Sampling/Final/ROP; no board/PnR claim')
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
