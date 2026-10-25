"""Bounded complete Stanford bunny replay through SDRAM, GPU and framebuffer."""
from pathlib import Path
from collections import Counter, defaultdict
import argparse, copy, hashlib, importlib.util, json, math, os, re, shutil, subprocess, sys, time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SHA = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    sys.modules[name] = m
    spec.loader.exec_module(m)
    return m


def execute(cmd, cwd, log, timeout=1800, env=None):
    t = time.monotonic()
    p = subprocess.run(list(map(str, cmd)), cwd=cwd, env=env, capture_output=True, timeout=timeout)
    Path(log).write_bytes(p.stdout + p.stderr)
    assert p.returncode == 0, (p.stdout + p.stderr).decode(errors='replace')[-7000:]
    return p.stdout.decode(errors='replace'), time.monotonic() - t


def smooth_normals(world, faces):
    """Area-weighted normals with a deterministic cancellation fallback.

    Zipper meshes can contain opposite copies of a face and vertices incident
    only to that pair. Never normalize floating-point cancellation residue.
    Preserve every source triangle; repair only undefined vertex normals using
    an incident face oriented toward its reliable neighboring vertex normals.
    """
    normals = [[0.0]*3 for _ in world]
    area = [0.0]*len(world)
    incident = [[] for _ in world]
    for triangle in faces:
        a, b, c = triangle
        u = [world[b][k]-world[a][k] for k in range(3)]
        w = [world[c][k]-world[a][k] for k in range(3)]
        n = (u[1]*w[2]-u[2]*w[1], u[2]*w[0]-u[0]*w[2], u[0]*w[1]-u[1]*w[0])
        length = math.sqrt(sum(x*x for x in n))
        for i in triangle:
            area[i] += length
            incident[i].append((triangle, n, length))
            for k in range(3):
                normals[i][k] += n[k]
    reliable = [math.sqrt(sum(x*x for x in n)) > area[i]*1e-12 for i, n in enumerate(normals)]
    repaired = []
    for i, n in enumerate(normals):
        if not reliable[i] and incident[i]:
            triangle, face, length = max(incident[i], key=lambda v: v[2])
            assert length > 0, ('isolated degenerate vertex', i)
            adjacent = [j for j in triangle if reliable[j]]
            assert adjacent, ('undefined surface orientation', i)
            score = sum(sum(face[k]*normals[j][k] for k in range(3)) for j in adjacent)
            assert abs(score) > 0, ('ambiguous surface orientation', i)
            normals[i] = [v*(1 if score > 0 else -1) for v in face]
            repaired.append(i)
    for n in normals:
        length = math.sqrt(sum(x*x for x in n)) or 1
        for k in range(3):
            n[k] /= length
    return normals, repaired


def mesh(path):
    lines = path.read_text().splitlines()
    assert lines[0] == 'ply' and lines[1] == 'format ascii 1.0'
    nv = next(int(l.split()[-1]) for l in lines if l.startswith('element vertex'))
    nf = next(int(l.split()[-1]) for l in lines if l.startswith('element face'))
    h = lines.index('end_header') + 1
    v = [tuple(map(float, l.split()[:3])) for l in lines[h:h+nv]]
    f = [tuple(map(int, l.split()[1:])) for l in lines[h+nv:h+nv+nf]]
    assert len(v) == nv and len(f) == nf and all(len(t) == 3 for t in f)
    center = [(min(p[k] for p in v) + max(p[k] for p in v))/2 for k in range(3)]
    scale = 1.5/max(max(p[k] for p in v)-min(p[k] for p in v) for k in range(3))
    world = [tuple((p[k]-center[k])*scale for k in range(3)) for p in v]
    normals, repaired = smooth_normals(world, f)
    # One model-wide decompression grid keeps duplicated meshlet vertices bit-identical.
    raw = [tuple(round(x*65536) for x in p) for p in world]
    origin = [min(p[k] for p in raw) for k in range(3)]
    shift = max(0, max(max(p[k] for p in raw)-origin[k] for k in range(3)).bit_length()-10)
    while any(round((p[k]-origin[k])/(1<<shift)) > 1023 for p in raw for k in range(3)): shift += 1
    packed = []
    for i,p in enumerate(raw):
        pos = sum(round((p[k]-origin[k])/(1<<shift)) << (10*k) for k in range(3))
        normal = sum((max(-511,min(511,round(normals[i][k]*511))) & 1023) << (10*k) for k in range(3))
        uv = [max(0,min(65535,round((world[i][k]/1.5+0.5)*65535))) for k in (0,1)]
        packed.append([pos, normal, uv[0] | (uv[1]<<16)])
    adjacent = defaultdict(set)
    for i,t in enumerate(f):
        for x in t: adjacent[x].add(i)
    remaining = set(range(nf)); batches = []
    while remaining:
        used = set(); faces = []; candidates = {min(remaining)}
        while len(faces)<96:
            legal = [i for i in candidates if i in remaining and len(used | set(f[i]))<=64]
            if not legal: break
            i = min(legal, key=lambda i:(len(set(f[i])-used), i))
            faces.append(i); remaining.remove(i); candidates.discard(i)
            used.update(f[i])
            for x in f[i]: candidates.update(adjacent[x] & remaining)
        assert faces
        ids = sorted(used); remap = {x:i for i,x in enumerate(ids)}
        batches.append(dict(source_vertex_ids=ids, source_face_ids=faces,
            vertices=[packed[i] for i in ids], triangles=[[remap[x] for x in f[i]] for i in faces]))
    assert sorted(i for b in batches for i in b['source_face_ids']) == list(range(nf))
    return batches, dict(vertices=nv,triangles=nf,used_vertices=len({x for t in f for x in t}),
        meshlets=len(batches),submitted_vertices=sum(len(b['vertices']) for b in batches),
        config=[*origin,shift|256],source_sha256=SHA(path),world_center=center,world_scale=scale,
        normal_fallback_vertices=repaired,
        note='Every source face exactly once; no further decimation; shared model-wide position grid and normals; generated UVs')


def camera_for(name):
    if name == 'front':
        return copy.deepcopy(json.loads((HERE/'reference/uv-inputs.json').read_text())['scenes'][0]['camera'])
    eye = [2.8,0.1,0.0] if name == 'side' else [0.0,0.1,-2.8]
    def unit(v):
        n=math.sqrt(sum(x*x for x in v));return [x/n for x in v]
    def cross(a,b):return [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]
    forward=unit([-x for x in eye]);right=unit(cross([0,1,0],forward));up=cross(forward,right)
    rotation=[right,[-x for x in up],forward]
    h=[[x*(180/256 if i<2 else 1) for x in row] for i,row in enumerate(rotation)]
    h=[row+[-sum(row[k]*eye[k] for k in range(3))] for row in h]
    uniform=[round(v*65536) for row in h for v in row]+[round(v*65536) for row in rotation for v in row]+[65536]+[0]*10
    return dict(name=name,eye=eye,target=[0,0,0],focal_pixels=180,uniform=uniform,world_to_h_float=h,normal_matrix_float=rotation)


MC_MONITOR = r'''
reg descriptor_valid=0,descriptor_page=0,completion_ready=0;
wire descriptor_ready,completion_valid,completion_publish,completion_page,start_ready;
reg[7:0]descriptor_tag=0;wire[7:0]completion_tag;wire[3:0]completion_error;
reg[21:0]config_source=0,metadata_source=0,payload_source=0;
reg[10:0]config_bytes=0,payload_bytes=0;
localparam NJ=@NJ@;
reg[127:0]jobs[0:NJ-1];reg[63:0]expected_dma[0:NJ*512-1];reg expected_valid[0:NJ*512-1];
integer active_job=-1,job_words=0,job_completions=0,dma_overlap=0;
integer ro_requests=0,fb_reads=0,fb_writes=0,ro_beats=0,fb_r_beats=0,fb_w_beats=0;
integer raw_seed_addr,seed_handle,seed_rc;reg[31:0]seed_value,dump_value;
reg force_pause=0,descriptor_taken=0;
integer frame_begin,render_begin,render_end,frame_end,mesh_start,mesh_front_end;
integer vw=0,vwait=0,tw=0,twait=0,prep=0,ib=0,rb=0,nodes=0,walk=0,leaf=0,row=0,rprep=0,rcredit=0;
integer pixel_launch=0,covered_launch=0,empty_scan=0,numeric_reject=0,host_start_pending=0;
integer wall[0:7],wc;
wire dma_we=dut.dma_we,dma_ready=dut.dma_ready;wire[9:0]dma_addr=dut.dma_addr;wire[63:0]dma_data=dut.dma_data;
always @(posedge clk)if(!reset)begin
 if(descriptor_valid&&descriptor_ready)begin descriptor_taken=1;job_words=0;end
 if(dma_we)begin
  if(!dma_ready||active_job<0||dma_addr>=512)$fatal(1,"DMA fixed sink");
  if(!expected_valid[active_job*512+dma_addr]||dma_data!==expected_dma[active_job*512+dma_addr])$fatal(1,"DMA source/destination");
  job_words=job_words+1;if(!frontend_idle)dma_overlap=dma_overlap+1;
 end
 if(ro_request_valid&&ro_request_ready)ro_requests=ro_requests+1;
 if(fb_r_request_valid&&fb_r_request_ready)fb_reads=fb_reads+1;
 if(fb_w_request_valid&&fb_w_request_ready)fb_writes=fb_writes+1;
 if(ro_response_valid)ro_beats=ro_beats+1;
 if(fb_r_response_valid)fb_r_beats=fb_r_beats+1;
 if(fb_w_data_ready)fb_w_beats=fb_w_beats+1;
 if((ro_response_valid&&ro_response_error)||(fb_r_response_valid&&fb_r_response_error)||(fb_w_response_valid&&fb_w_response_error))$fatal(1,"actual MC error");
 if(ce&&task_id>=0)begin
  if(dut.core.front.meshlet.core.busy)begin
   if(!dut.core.front.meshlet.core.triangle_mode)begin if(dut.core.front.meshlet.core.step)vw=vw+1;else vwait=vwait+1;end
   else begin if(dut.core.front.meshlet.core.step)tw=tw+1;else twait=twait+1;end
  end
  if(dut.core.front.s_busy)prep=prep+1;
  if(dut.core.front.interpolator.p_busy)ib=ib+1;
  if(dut.core.front.raster_busy)rb=rb+1;
  if(dut.core.front.raster.evt_node)nodes=nodes+1;
  if(dut.core.front.raster.evt_traverse)walk=walk+1;
  if(dut.core.front.raster.evt_leaf)leaf=leaf+1;
  if(dut.core.front.raster.evt_row)row=row+1;
  if(dut.core.front.raster.evt_prep)rprep=rprep+1;
  if(dut.core.front.raster.evt_credit_wait)rcredit=rcredit+1;
  if(dut.core.front.raster.evt_empty_quad)empty_scan=empty_scan+1;
  if(dut.core.front.interpolator.compute.launch)begin pixel_launch=pixel_launch+1;end
  if(dut.core.front.raster.scan_cover_valid&&dut.core.front.sample_cover)covered_launch=covered_launch+1;
  if(dut.core.depth_valid&&!dut.core.depth_pass)numeric_reject=numeric_reject+1;
  if(start_taken&&global_used)host_start_pending=host_start_pending+1;
 end
 if(frame_begin>0)begin
  if(dut.core.front.meshlet.core.busy&&!dut.core.front.meshlet.core.triangle_mode)wc=0;
  else if(dut.core.front.s_busy)wc=1;
  else if(dut.core.front.raster_busy)wc=2;
  else if(dut.core.front.interpolator.p_busy)wc=3;
  else if(dut.core.front.meshlet.core.busy)wc=4;
  else if(!dut.compute_idle)wc=5;
  else if(dut.memory_busy)wc=6;else wc=7;
  wall[wc]=wall[wc]+1;
 end
end
'''

TASKS = r'''
task present_job;input integer n;begin
 active_job=n;descriptor_tag=n%256;descriptor_page=jobs[n][88];config_source=jobs[n][21:0];metadata_source=jobs[n][43:22];payload_source=jobs[n][65:44];
 config_bytes=jobs[n][76:66];payload_bytes=jobs[n][87:77];
 descriptor_taken=0;descriptor_valid=1;while(!descriptor_taken)tick();descriptor_valid=0;
 while(!completion_valid)tick();
 if(!completion_publish||completion_error||completion_tag!==n%256||job_words!=jobs[n][98:89])$fatal(1,"loader completion");
 completion_ready=1;tick();while(completion_valid)tick();completion_ready=0;job_completions=job_completions+1;
end endtask
task prepare_task;input integer taskno,vn,tn,pn;input use_slot;
reg[1023:0]path;begin
 while(!meshlet_ready)tick();task_id=taskno;nv=vn;nt=tn;np=pn;slot=use_slot;vi=0;ti=0;pi=0;saw_done=0;
 $sformat(path,"task%0d/vertices.hex",taskno);$readmemh(path,vertices,0,nv*5-1);
 $sformat(path,"task%0d/admissions.hex",taskno);$readmemh(path,admissions,0,nt-1);
 if(np>0)begin
  $sformat(path,"task%0d/descriptors.hex",taskno);$readmemh(path,descriptors,0,np-1);
  $sformat(path,"task%0d/words.hex",taskno);$readmemh(path,words,0,np*16-1);
 end
 expect_task(taskno,np);
 start=1;start_taken=0;while(!start_taken)tick();start=0;mesh_start=cycle;
 $display("START task=%0d cycle=%0d",task_id,cycle);
end endtask
task finish_task;begin
 while(!saw_done)tick();mesh_front_end=cycle;
 if(vi!=nv*5||ti!=nt||published_vertices!=nv)$fatal(1,"task computation counts");
 $display("MESH task=%0d cycles=%0d end=%0d V=%0d T=%0d P=%0d exported=%0d",task_id,cycle-mesh_start,cycle,vi/5,ti,np,observed_packets[task_id]);
end endtask
'''

INITIAL = r'''
initial begin
 frame_begin=0;for(j=0;j<8;j=j+1)wall[j]=0;
 fd=$fopen("front.txt","w");bd=$fopen("back.txt","w");
 $readmemh("jobs.hex",jobs);$readmemh("expected-dma.hex",expected_dma);$readmemh("expected-valid.hex",expected_valid);
 seed_handle=$fopen("seed.txt","r");if(!seed_handle)$fatal(1,"seed missing");
 while(!$feof(seed_handle))begin seed_rc=$fscanf(seed_handle,"%h %h",raw_seed_addr,seed_value);
  if(seed_rc==2)pin.seed_word(native_word(raw_seed_addr),seed_value);
 end $fclose(seed_handle);
 tick();tick();reset=0;
 frame_begin=cycle;present_job(0);render_begin=cycle;
 @SEQUENCE@
 render_end=cycle;inputs_done=1;while(!idle)tick();flush_request=1;while(!flush_complete)tick();frame_end=cycle;
 ff=$fopen("memory.mem","w");
 for(j=0;j<BYTES;j=j+4)begin
  if(j<TEX)raw_seed_addr=32'h1000+j;else if(j<TEX+FB)raw_seed_addr=32'h100000+j-TEX;else raw_seed_addr=32'h140000+j-TEX-FB;
  dump_value=pin.physical_word(native_word(raw_seed_addr));
  $fdisplay(ff,"%h\n%h\n%h\n%h",dump_value[7:0],dump_value[15:8],dump_value[23:16],dump_value[31:24]);
 end $fclose(ff);
 if(job_completions!=NJ)$fatal(1,"missing jobs");
 $display("PASS BUNNY total=%0d preload=%0d frontend=%0d tail=%0d Q=%0d empty=%0d peak=%0d fullCE=%0d packets=%0d light=%0d sample=%0d final=%0d commits=%0d RO=%0d FBread=%0d FBwrite=%0d RObeats=%0d FBreadbeats=%0d FBwritebeats=%0d overlapDMA=%0d pendingStarts=%0d",frame_end-frame_begin,render_begin-frame_begin,render_end-render_begin,frame_end-render_end,sequence_id,skips,peak,full_ce,all_pi,nlight,nsample,nfinal,commits,ro_requests,fb_reads,fb_writes,ro_beats,fb_r_beats,fb_w_beats,dma_overlap,host_start_pending);
 $display("ACTIVITY Vwork=%0d Vwait=%0d Twork=%0d Twait=%0d prepare=%0d Rbusy=%0d Ibusy=%0d nodes=%0d traversal=%0d leaf=%0d row=%0d Rprep=%0d Rcredit=%0d pixels=%0d covered=%0d emptyScan=%0d invalidLanes=%0d",vw,vwait,tw,twait,prep,rb,ib,nodes,walk,leaf,row,rprep,rcredit,pixel_launch,covered_launch,empty_scan,numeric_reject);
 $write("WALL ");for(j=0;j<8;j=j+1)$write("%0d ",wall[j]);$display("");
 $fclose(fd);$fclose(bd);$finish;
end
initial begin #@MAXTIME@;$fatal(1,"HDL bounded timeout");end
endmodule
'''


def bench(S,V,metas,max_cycles):
    base = (HERE/'stage2_tb.v').read_text()
    declarations = base[:base.index('gpu_v2_vliw dut(.*);')]
    declarations = declarations.replace(',dma_we=0','').replace('reg[9:0]dma_addr=0;reg[63:0]dma_data=0;wire dma_ready;','')
    declarations = declarations[:declarations.index('wire mem_req_valid')]
    declarations += '''reg controller_clk=0,phy_clk=0;
always #5.051 controller_clk=~controller_clk;
always @(posedge controller_clk)clk=~clk;
initial begin #13.259;forever begin phy_clk=~phy_clk;#5.051;end end
'''
    declarations += S.PORTS + S.MC[:S.MC.index('reg descriptor_valid')]
    common = base[base.index('localparam STRESS='):base.index('function integer mi;')].replace(',requests=0,paid_ce0=0','')
    monitor = base[base.index('always @(posedge clk)begin'):base.index('task tick;')].replace('dut.','dut.core.')
    monitor = monitor.replace('start_taken=ce&&!reset&&start&&meshlet_ready;', 'start_taken=ce&&!reset&&start&&start_ready;')
    sequence = []
    for i,m in enumerate(metas):
        sequence.append(f'prepare_task({i},{m["vertices"]},{m["triangles"]},{m["packets"]},{i%2});')
        if i+1<len(metas): sequence.append(f'present_job({i+1});')
        sequence.append('finish_task();')
    result = declarations + common + MC_MONITOR + '\ngpu_v2_vliw_sdram dut(.*);\n' + monitor + S.TICK[:S.TICK.index('task present_job;')] + TASKS + INITIAL
    fields = dict(STRESS=0, MAXC=max_cycles, BYTES=len(V.initial_memory()), MAXTIME=int(max_cycles*21+200000), NJ=len(metas), SEQUENCE='\n'.join(sequence))
    for k,v in fields.items(): result=result.replace('@'+k+'@',str(v))
    assert not re.search('@[A-Z0-9]+@',result)
    return result


def dma_fixture(out,V,metas,initial=None):
    seed={};b=V.initial_memory() if initial is None else initial;jobs=[];expected=[0]*(512*len(metas));valid=[0]*len(expected)
    for off in range(0,len(b),4):
        a=0x1000+off if off<V.TEX else 0x100000+off-V.TEX if off<V.TEX+V.FB else 0x140000+off-V.TEX-V.FB
        seed[a]=int.from_bytes(b[off:off+4],'little')
    for i,m in enumerate(metas):
        image={int(a):int(v,16) for a,v in (l.split() for l in (out/f'task{i}/dma.txt').read_text().splitlines())}
        page=i%2;pay_words=[a for a in image if page*128<=a<(page+1)*128]
        length=(max(pay_words)-page*128+1)*8
        cfg=0x30000;meta=0x200000+i*0x500;pay=meta+128
        assert pay+1024<=0x800000
        count=0
        for a,value in image.items():
            if a>=320:
                if i:continue
                source=cfg+(a-320)*8
            elif a>=256:source=meta+(a-256-4*page)*8
            else:source=pay+(a-page*128)*8
            assert source not in seed
            seed[source],seed[source+4]=value&0xffffffff,value>>32
            expected[i*512+a]=value;valid[i*512+a]=1;count+=1
        for a in range(pay,pay+((length+31)//32)*32,4):seed.setdefault(a,0)
        jobs.append((cfg//2)|((meta//2)<<22)|((pay//2)<<44)|((128 if i==0 else 0)<<66)|(length<<77)|(page<<88)|(count<<89))
    (out/'seed.txt').write_text(''.join(f'{a:08x} {v:08x}\n' for a,v in sorted(seed.items())))
    (out/'jobs.hex').write_text(''.join(f'{v:032x}\n' for v in jobs))
    (out/'expected-dma.hex').write_text(''.join(f'{v:016x}\n' for v in expected))
    (out/'expected-valid.hex').write_text(''.join(f'{v:x}\n' for v in valid))
    return dict(payload_bytes=sum(((x>>77)&2047) for x in jobs),metadata_bytes=32*len(jobs),config_bytes=128,
        geometry_address_end=max(a for a in seed if a>=0x200000)+4)


def main():
    ap=argparse.ArgumentParser(__doc__)
    ap.add_argument('--ply',type=Path,required=True);ap.add_argument('--tag',required=True)
    ap.add_argument('--camera',choices=['front','side','back'],default='front')
    ap.add_argument('--max-cycles',type=int,default=15000000)
    ap.add_argument('--sim-timeout',type=int,default=1800)
    a=ap.parse_args();assert re.fullmatch('[A-Za-z0-9_.-]+',a.tag)
    out=ROOT/'target/gpu-full-bunny-20261018'/a.tag;out.mkdir(parents=True,exist_ok=False)
    runner_sha=SHA(__file__)
    shutil.copy2(__file__,out/'runner-used.py')
    S=load('full_bunny_sdram_helpers',ROOT/'ip/gpu-v2/tests/vliw_sdram.py')
    V=load('full_bunny_integer_audit',HERE/'verify_stage2.py')
    O=load('full_bunny_front_oracle',HERE/'reference/scene/oracle.py')
    O = load('current_transport', HERE / 'transport.py').adapt_oracle(O)
    batches,info=mesh(a.ply);assert 0<len(batches)<=2048
    camera=camera_for(a.camera)
    uniform=camera['uniform'];packets=[];metas=[];instances=[]
    for i,b in enumerate(batches):
        instance=dict(name=f'bunny_full_{i}',group='bunny',slot=i%2,uniform=uniform,config=info['config'],
            vertices=b['vertices'],triangles=b['triangles'],color_mode=0,cull_negative=1,seq_base=0,generation=i%16,
            cfg_tex_w_log2=6,cfg_tex_h_log2=6,cfg_max_mip=6)
        meta,ps=O.prepare(instance,out/f'task{i}');packets+=ps;metas.append(meta);instances.append(instance)
    classes=Counter()
    for m in metas:classes.update(m['classes'])
    info.update(camera=camera,classes=dict(classes),packets=len(packets),rtl_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip())
    (out/'input.json').write_text(json.dumps(dict(info=info,instances=instances,face_ids=[b['source_face_ids'] for b in batches]),indent=2))
    info['memory']=dma_fixture(out,V,metas)
    env=os.environ.copy();env['CARGO_TARGET_DIR']=str(ROOT/'target/gpu-vliw-integration-20261018/cargo-root')
    assert env.get('IVERILOG_EXE') and env.get('VVP_EXE'), 'Set IVERILOG_EXE and VVP_EXE'
    execute([Path.home()/'.cargo/bin/cargo.exe','build','--release','-p','gpu-v2','--bin','gpu-v2-vliw-export','--example','vliw_integration'],ROOT,out/'build.log',env=env)
    exporter=Path(env['CARGO_TARGET_DIR'])/'release/gpu-v2-vliw-export.exe'
    gold=Path(env['CARGO_TARGET_DIR'])/'release/examples/vliw_integration.exe'
    execute([exporter,out/'dut.v','sdram','portable'],ROOT,out/'export.log',env=env)
    origins=S.wiring_origins(out/'dut.v')
    execute([Path.home()/'.cargo/bin/cargo.exe','test','--release','-p','gpu-v2','--test','host','vliw_sdram::export_real_shared_sdram_peer','--','--nocapture'],ROOT,out/'mc-export.log',env=env)
    for p in (ROOT/'target/gpu-vliw-integration-20261018/stage3/mc-peer').iterdir():shutil.copy2(p,out/p.name)
    (out/'tb.v').write_text(bench(S,V,metas,a.max_cycles))
    execute([env['IVERILOG_EXE'],'-g2012','-D__ICARUS__','-I',out,'-s','tb','-o',out/'sim.vvp',*sorted(out.glob('*.v'))],out,out/'compile.log',env=env)
    print(json.dumps(dict(status='prepared',info=info),ensure_ascii=False),flush=True)
    log,seconds=execute([env['VVP_EXE'],out/'sim.vvp'],out,out/'run.log',a.sim_timeout,env)
    print(log[-4000:],flush=True)
    assert 'PASS BUNNY' in log
    counts=V.audit(out,packets,O,gold)
    def parse(prefix):return {k:int(v) for k,v in re.findall(r'(\w+)=(\d+)',next(l for l in log.splitlines() if l.startswith(prefix)))}
    timing=parse('PASS BUNNY');activity=parse('ACTIVITY')
    assert activity['covered']==sum(m['coverage_pixels'] for m in metas)
    assert activity['pixels']==4*counts['quads']
    wall_keys=('vertex','prepare','raster','I_tail','T_only','backend_tail','memory_only','other')
    wall=dict(zip(wall_keys,map(int,next(l for l in log.splitlines() if l.startswith('WALL ')).split()[1:])))
    assert sum(wall.values())==timing['total'] and len(wall)==8
    assert not S.changed_origins(out,origins)
    assert SHA(__file__)==runner_sha
    receipt=dict(status='pass',info=info,counts=counts,timing=timing,activity=activity,exclusive_wall=wall,simulation_seconds=seconds,
        at_54MHz=dict(frame_ms=timing['total']/54000,fps=54000000/timing['total']),
        boundary='All source faces; current GPU RTL with actual main arbiter/bridge/controller and independent pin model; no CPU/display contention; preclear/ROM setup excluded; first load and final flush included. Functional clocks 49.5/99 MHz; 54 MHz is cycle scaling only.',
        source_sha256=origins,runner_sha256=runner_sha,
        runner_support_sha256={p.name:SHA(p) for p in [HERE/'transport.py',HERE/'stage2_tb.v',HERE/'verify_stage2.py',ROOT/'ip/gpu-v2/tests/vliw_sdram.py']},rtl_sources={p.name:SHA(p) for p in out.glob('*.v')},
        dut_sha256=SHA(out/'dut.v'),front_sha256=SHA(out/'front.txt'),back_sha256=SHA(out/'back.txt'),memory_sha256=SHA(out/'memory.mem'))
    (out/'receipt.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2))
    print(json.dumps({k:v for k,v in receipt.items() if k not in ['source_sha256']},ensure_ascii=False,indent=2),flush=True)


if __name__=='__main__':main()
