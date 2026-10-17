"""实际VLIW最终码→完整采样/Final/ROP有界回放；独立整数逐tap与DRAM金值。"""
from pathlib import Path
import json,hashlib,os,subprocess,argparse,importlib.util,sys,shutil
R=Path(__file__).resolve().parents[3];O=R/'target/gpu-vliw-integration-20261018/stage1'
SHA=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
MAX_N=5;FB=400*240*2;TEX=8192

def pattern(n,x,y):return ((x*3+y*7+n*5)%32<<11)|((y*5+x*11+n*13)%64<<5)|((x*13+y*3+n*17)%32)
def expand(w):return [(w>>11<<3)|(w>>13),(((w>>5)&63)<<2)|((w>>9)&3),((w&31)<<3)|((w>>2)&7)]
def sample(uv,lod):
    d=min(lod//8,MAX_N);n=MAX_N-d;f=lod%8 if d<MAX_N else 0;p1=(511*f+4)//8
    sums=[0,0,0]
    fine=[(v*(1<<max(1,n))//256)-128 for v in uv]
    for layer,parent in [(n,511-p1),(max(0,n-1),p1)]:
        if not parent:continue
        xy=fine if layer==n else [(v-128)//2 if n>1 else v for v in fine]
        frac=[v%256 for v in xy];base=[v//256 for v in xy]
        bot=parent*frac[1]//256;top=parent-bot;tr=top*frac[0]//256;br=bot*frac[0]//256
        weights=[top-tr,tr,bot-br,br]
        for t,w in enumerate(weights):
            side=1<<layer;x=(base[0]+t%2)%side;y=(base[1]+t//2)%side
            col=expand(pattern(layer,x,y))
            for c in range(3):sums[c]+=(w*col[c]+256)//512
    return sums

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--tag',default='v2');ap.add_argument('--steady',action='store_true');ap.add_argument('--single-texture-tile',action='store_true');ap.add_argument('--paced',action='store_true');ap.add_argument('--count',type=int,default=64);ap.add_argument('--mode',choices=['portable','native','both'],default='both');opts=ap.parse_args()
    jobs=[];reference=R/'ip/gpu-v2/examples/vliw_integration/reference';sources={}
    for path in reference.rglob('*'):
        if path.is_file() and path.suffix in ('.py','.json'):sources[str(path.relative_to(R))]=SHA(path)
    spec=importlib.util.spec_from_file_location('stage1_complete_oracle',reference/'scene/oracle.py')
    oracle=importlib.util.module_from_spec(spec);sys.modules[spec.name]=oracle;spec.loader.exec_module(oracle)
    fixtures=json.loads((reference/'uv-inputs.json').read_text(encoding='utf8'))
    for instance in fixtures['scenes'][0]['meshlets']:
        name=instance['name'];_,packets=oracle.prepare(instance,O/f'complete-{opts.tag}-oracle'/name)
        selected=[]
        for packet in packets:
            for qi,mask in sorted(packet['masks'].items()):
                x=(qi%200)*2;y=(qi//200)*2
                h00=tuple(c+2*nx*x+2*ny*y for nx,ny,c in packet['edges'])
                v=oracle.M.quad(packet['triangle'],h00,mask)
                if not v['passed']:continue
                lanes=[]
                for lane in range(4):
                    p=v['pixels'].get(lane)
                    lanes.append(dict(natural_lane=lane,covered=bool(mask>>lane&1),passed=bool(v['passed']>>lane&1),
                        normal_s12f10=list(p['normal']) if p else [0,0,0],uv_s18f16=list(p['uv']) if p else [0,0],depth_u16q8_8=p['Z16'] if p else 0))
                selected.append(dict(qx=x//2,qy=y//2,geometry_mask=mask,numerical_mask=v['passed'],lod_u5f3=v['lod']['LOD_q3'],lanes=lanes,
                    textured=True,lit=False,tint=[[180,210,255]]*4,source=name,triangle_id=packet['tid']))
                if len(selected)==2:break
            if len(selected)==2:break
        assert len(selected)==2
        jobs.extend(selected)
    # Empty at start, sparse lane0 absent, high LOD clamp, RGB still lit, and both branches.
    empty=json.loads(json.dumps(jobs[0]));empty['numerical_mask']=0;empty['source']='directed_empty'
    jobs.insert(0,empty)
    for textured,lit,lod in [(True,False,255),(False,True,0),(True,True,4),(False,False,0)]:
        q=json.loads(json.dumps(jobs[-1]));q.update(textured=textured,lit=lit,lod_u5f3=lod,source='directed_mode');q['numerical_mask']=15
        for l in q['lanes']:l['normal_s12f10']=[0,0,1024]
        jobs.append(q)
    if opts.steady:
        q=next(q for q in jobs if q['textured'] and q['lit'])
        assert 64<=opts.count<=128
        jobs=[json.loads(json.dumps(q)) for _ in range(opts.count)]
        if opts.single_texture_tile:
            for q in jobs:
                q['source']='directed_same_texture_tile'
                for l in q['lanes']:l['uv_s18f16']=[4352,4864]
    init=bytearray(TEX+2*FB);init[TEX+FB:]=b'\xff'*FB
    offset=0
    for n in range(MAX_N+1):
        logical=1<<n
        for ty in range((logical+7)//8):
            for tx in range((logical+7)//8):
                for y in range(8):
                    for x in range(8):
                        w=pattern(n,(tx*8+x)%logical,(ty*8+y)%logical);init[offset:offset+2]=w.to_bytes(2,'little');offset+=2
    gold=bytearray(init);rom=[];sg=[];fg=[];nonempty=0;pixels=0
    for q in jobs:
        mask=q['numerical_mask'];nonempty+=bool(mask);pixels+=mask.bit_count();begin=mask|((not q['lit'])<<5)|((not q['textured'])<<6)|(q['qx']<<8)|(q['qy']<<16)
        for lane,l in enumerate(q['lanes']):
            uv=[v&65535 for v in l['uv_s18f16']];normal=sum((v&4095)<<(12*c) for c,v in enumerate(l['normal_s12f10']))
            tex=sample(uv,q['lod_u5f3']) if q['textured'] else [255]*3;g=224 if q['lit'] else 256
            rgb=[]
            for tint,col in zip(q['tint'][lane],tex):
                value=tint*col+128;base=(value+(value>>8))>>8;rgb.append(min(255,(base*g)//256))
            sg.append(sum(v<<(8*c) for c,v in enumerate(tex)));fg.append(sum(v<<(8*c) for c,v in enumerate(rgb)))
            for odd in (0,1):
                basic=l['depth_u16q8_8'] if odd else sum(v<<(8*c) for c,v in enumerate(q['tint'][lane]))
                fields=[(begin,23),(basic,32),(normal if not odd else 0,36),((uv[0]|uv[1]<<16) if odd else 0,32),(q['lod_u5f3'] if lane==0 and not odd else 0xa5,8)]
                w=0
                for v,width in fields:w=(w<<width)|v
                rom.append(w)
            if mask>>lane&1:
                x=q['qx']*2+lane%2;y=q['qy']*2+lane//2;off=((y//16)*25+x//16)*512+(y%16)*32+(x%16)*2
                # Selected REPLACE quantizer, RNE rgb8 to 565.
                def rnd(v,d):return v//d+int(v%d*2>d or v%d*2==d and (v//d)&1)
                c=(rnd(rgb[0]*31,255)<<11)|(rnd(rgb[1]*63,255)<<5)|rnd(rgb[2]*31,255)
                gold[TEX+off:TEX+off+2]=c.to_bytes(2,'little');gold[TEX+FB+off:TEX+FB+off+2]=l['depth_u16q8_8'].to_bytes(2,'little')
    data=(O/f'complete-{opts.tag}-inputs.json');data.write_text(json.dumps({'source_sha256':sources,'jobs':jobs,'nonempty':nonempty,'pixels':pixels},ensure_ascii=False,indent=2),encoding='utf8')
    tb=TB.replace('@TOTAL@',str(len(jobs))).replace('@LIVE@',str(nonempty)).replace('@PIXELS@',str(pixels)).replace('@BYTES@',str(len(init))).replace('@TEX@',str(TEX)).replace('@FB@',str(FB)).replace('@STEADY@',str(int(opts.steady))).replace('@EXPECT16@',str(int(opts.single_texture_tile))).replace('@PACED@',str(int(opts.paced))).replace('@WA@',str(max(0,len(jobs)-32))).replace('@WB@',str(len(jobs)-1)).replace('@FLOW@',str(min(24,len(jobs)-1)))
    reports=[]
    for mode in (('portable','native') if opts.mode=='both' else (opts.mode,)):
        d=O/f'complete-{opts.tag}-{mode}';d.mkdir(exist_ok=False)
        shutil.copy2(O/f'test-export/postquad-{mode}.v',d/'dut.v')
        for name,values,width in [('rom.mem',rom,33),('sample.mem',sg,6),('final.mem',fg,6),('initial.mem',init,2),('gold.mem',gold,2)]:
            (d/name).write_text(''.join(f'{v:0{width}x}\n' for v in values))
        (d/'tb.v').write_text(tb)
        cmd=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',str(d/'test.vvp')]
        if mode=='native':cmd+=[str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')]
        cmd +=[str(d/'dut.v'),str(d/'tb.v')]
        for step,args in [('compile',cmd),('run',[os.environ['VVP_EXE'],str(d/'test.vvp')])]:
            cp=subprocess.run(args,cwd=d,capture_output=True,text=True,timeout=120);(d/f'{step}.log').write_text(cp.stdout+cp.stderr);assert cp.returncode==0,cp.stdout+cp.stderr
        reports.append({'mode':mode,'summary':cp.stdout,'trace_sha256':SHA(d/'trace.txt'),'source_sha256':SHA(d/'dut.v')});print(mode,cp.stdout,flush=True)
    if len(reports)==2:assert reports[0]['trace_sha256']==reports[1]['trace_sha256']
    (O/f'complete-{opts.tag}-receipt.json').write_text(json.dumps({'status':'pass','inputs_sha256':SHA(data),'reports':reports,'boundary':'actual VLIW UV/LOD/Z codes on four quads; directed empty/RGB-lit/full cases; selected integer tap/Final/ROP and whole memory oracle; not whole GPU'},ensure_ascii=False,indent=2)+'\n',encoding='utf8')
TB=r'''`timescale 1ns/1ps
module tb;
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0,reset=1,ce=0,abort=0,in_valid=0,draw_rebind=0,flush_request=0;
reg[22:0]in_begin=0;reg[31:0]in_basic=0,in_uv=0;reg[35:0]in_light=0;reg[7:0]in_lod=0;
reg[26:0]draw_config=(2<<25)|(1<<24)|(5<<20);reg[23:0]specular=0;reg[7:0]alpha=255;
reg[2:0]depth_func=7;reg depth_write=1,blend=0;reg[15:0]context_epoch=1;reg[1:0]context_mode=3;reg[4:0]context_code=8;
reg signed[15:0]light_x=0,light_y=0,light_z=16384,ray_x=-8192,ray_y=-8192,ray_k=8192;reg[8:0]ambient=32,directional=192;
wire in_ready,flush_complete,pipeline_idle,idle,fault,published,light_result,sample_result,final_issue,final_result,quad_accepted,commit_valid;
wire[2:0]in_row;wire[4:0]published_slot,work_reserved,work_count;wire[5:0]global_used;wire[8:0]commit_x;wire[7:0]commit_y;wire[3:0]commit_mask;
wire mem_req_valid,mem_req_write,mem_write_valid;wire[31:0]mem_req_addr;wire[63:0]mem_write_data;
reg mem_req_ready=0,mem_read_valid=0,mem_write_ready=0,mem_complete_valid=0,mem_complete_ok=1;reg[63:0]mem_read_data=0;
gpu_v2_postquad_direct_lod dut(.*);
reg[130:0]rom[0:@TOTAL@*8-1];reg[23:0]samples[0:@TOTAL@*4-1],finals[0:@TOTAL@*4-1];reg[7:0]memory[0:@BYTES@-1],gold[0:@BYTES@-1];integer slot_job[0:31];
integer wall=0,cursor=0,pending=0,address=0,beat=0,delay_count=0,pubs=0,rets=0,commits=0,nfinal=0,nsample=0,nlight=0,requests=0,paid_ce0=0,fd,i,k,job,lane;
integer nc[0:@TOTAL@-1],nm[0:@TOTAL@-1],ng[0:@TOTAL@-1],ncol[0:@TOTAL@-1],ctime[0:@TOTAL@-1],input_ce[0:@TOTAL@-1],core_c_ce[0:@TOTAL@-1];integer external_ce=0,core_ce=0;
reg finished=0;integer enabled=0,commit_wall[0:@TOTAL@-1],commit_ce[0:@TOTAL@-1],delta,ii_min=100000,ii_max=0;
function integer mi;input[31:0]a;begin
 if(a>=32'h200000)mi=@TEX@+@FB@+a-32'h200000;else if(a>=32'h100000)mi=@TEX@+a-32'h100000;else mi=a;
 if(mi<0||mi>=@BYTES@)$fatal(1,"memory range %h",a);
end endfunction
initial begin
 fd=$fopen("trace.txt","w");$readmemh("rom.mem",rom);$readmemh("sample.mem",samples);$readmemh("final.mem",finals);$readmemh("initial.mem",memory);$readmemh("gold.mem",gold);
 for(i=0;i<32;i=i+1)slot_job[i]=-1;for(i=0;i<@TOTAL@;i=i+1)begin nc[i]=0;nm[i]=0;ng[i]=0;ncol[i]=0;end
 #1;clk=1;#1;clk=0;#1;reset=0;
 for(wall=1;wall<30000;wall=wall+1)begin
  ce=wall%29!=3&&wall%29!=4;in_valid=cursor<@TOTAL@*8&&(!@PACED@||external_ce>=16*(cursor/8));
  if(in_valid){in_begin,in_basic,in_light,in_uv,in_lod}=rom[cursor];
  mem_req_ready=pending==0&&wall%7!=1;mem_read_valid=pending==1&&delay_count==0&&wall%5!=2;
  mem_write_ready=pending==3&&delay_count==0&&wall%7!=2;mem_complete_valid=(pending==2||pending==4)&&delay_count==0;
  mem_read_data=0;if(mem_read_valid)for(k=0;k<8;k=k+1)mem_read_data[k*8+:8]=memory[mi(address+beat*8+k)];
  #2;
  if(ce)external_ce=external_ce+1;if(dut.front.sampling.direct.advance)core_ce=core_ce+1;
  if(dut.advance)enabled=enabled+1;
  if(fault)$fatal(1,"top fault wall%0d cursor%0d front%0d sink%0d final%0d sampling%0d",wall,cursor,dut.front_fault,dut.sink_fault,dut.final_fault,dut.front.sampling_fault);
  if(published)begin slot_job[published_slot]=cursor/8;pubs=pubs+1;end
  if(dut.front.sampling.c_issue)begin job=slot_job[dut.front.sampling.direct.in_quad];if(job>=0)begin nc[job]=nc[job]+1;if(dut.front.sampling.direct.in_lane==0)begin ctime[job]=external_ce;core_c_ce[job]=core_ce;end end end
  if(dut.front.backend.member_issue)begin job=slot_job[dut.front.backend.coefficient_meta[35:32]];if(job>=0)nm[job]=nm[job]+1;end
  if(dut.front.demand.read_issue)begin job=slot_job[dut.front.demand.head_payload[33:30]];if(job>=0)ng[job]=ng[job]+1;end
  if(dut.front.color.input_fire)begin job=slot_job[dut.front.color.in_payload[69:66]];if(job>=0)ncol[job]=ncol[job]+1;end
  if(light_result)begin if({dut.lh,dut.lg}!=={9'd192,9'd224})$fatal(1,"light endpoint %0d %0d",dut.lh,dut.lg);nlight=nlight+1;end
  if(sample_result)begin job=slot_job[dut.sample_address[6:2]];lane=dut.sk[1:0];if(job<0||dut.sRGB!==samples[job*4+lane])$fatal(1,"sample job%0d lane%0d got%h exp%h",job,lane,dut.sRGB,samples[job*4+lane]);nsample=nsample+1;$fdisplay(fd,"S %0d %0d %h",job,lane,dut.sRGB);end
  if(final_result)begin job=slot_job[dut.retire_slot];lane=dut.out_lane;if(job<0||dut.final_RGB!==finals[job*4+lane])$fatal(1,"final job%0d lane%0d got%h exp%h",job,lane,dut.final_RGB,finals[job*4+lane]);nfinal=nfinal+1;$fdisplay(fd,"F %0d %0d %h",job,lane,dut.final_RGB);end
  if(quad_accepted)rets=rets+1;if(commit_valid)begin commit_wall[commits]=wall;commit_ce[commits]=enabled;commits=commits+1;$fdisplay(fd,"C %0d %0d %h",commit_x,commit_y,commit_mask);end
  if(in_valid&&in_ready)begin if(cursor%8==0)input_ce[cursor/8]=external_ce;cursor=cursor+1;end
  if(mem_req_valid&&mem_req_ready)begin address=mem_req_addr;beat=0;delay_count=3;pending=mem_req_write?3:1;requests=requests+1;end else if(delay_count>0)delay_count=delay_count-1;
  if(mem_read_valid)begin if(!ce)paid_ce0=paid_ce0+1;beat=beat+1;if(beat==16)begin pending=2;delay_count=2;end end
  if(mem_write_valid&&mem_write_ready)begin for(k=0;k<8;k=k+1)memory[mi(address+beat*8+k)]=mem_write_data[k*8+:8];beat=beat+1;if(beat==16)begin pending=4;delay_count=2;end end
  if(mem_complete_valid)pending=0;
  if(cursor==@TOTAL@*8&&idle)flush_request=1;
  if(flush_complete)begin
   if(pubs!=@LIVE@||rets!=@LIVE@||commits!=@LIVE@||nfinal!=@PIXELS@||nlight<1)$fatal(1,"completion counts %0d %0d %0d %0d light%0d",pubs,rets,commits,nfinal,nlight);
   if(@STEADY@)begin
    for(i=@WA@+1;i<=@WB@;i=i+1)begin
     delta=commit_ce[i]-commit_ce[i-1];if(delta<ii_min)ii_min=delta;if(delta>ii_max)ii_max=delta;

    end
    if(@EXPECT16@&&@PACED@&&@TOTAL@>=96&&(commit_ce[@WB@]-commit_ce[@WA@]!=496||ctime[@WB@]-ctime[@WA@]!=496||input_ce[@WB@]-input_ce[@WA@]!=496))$fatal(1,"sustainable II16 not met");
    $display("DOMAINS externalCE=%0d advanceCE=%0d coreCE=%0d inputCE_window=%0d CissueCE_window=%0d CcoreCE_window=%0d input_to_commit_start=%0d end=%0d",external_ce,enabled,core_ce,input_ce[@WB@]-input_ce[@WA@],ctime[@WB@]-ctime[@WA@],core_c_ce[@WB@]-core_c_ce[@WA@],commit_ce[@WA@]-input_ce[@WA@],commit_ce[@WB@]-input_ce[@WB@]);
    $display("FLOW job@FLOW@ C=%0d member=%0d cacheRead=%0d ColorGroup=%0d Lighting=4 Final=4 ROPpixel=4",nc[@FLOW@],nm[@FLOW@],ng[@FLOW@],ncol[@FLOW@]);
    for(i=@WA@;i<=@WB@;i=i+1)$fdisplay(fd,"T %0d %0d %0d",i,commit_wall[i],commit_ce[i]);
    $display("STEADY interval_count=31 CE_interval_min=%0d max=%0d CE_window=%0d wall_window=%0d CE_total=%0d",ii_min,ii_max,commit_ce[@WB@]-commit_ce[@WA@],commit_wall[@WB@]-commit_wall[@WA@],enabled);
   end
   for(i=0;i<@BYTES@;i=i+1)if(memory[i]!==gold[i])$fatal(1,"DRAM byte%0d got%h exp%h",i,memory[i],gold[i]);
   $display("PASS complete sourceQ=@TOTAL@ liveQ=%0d pixels=%0d sample=%0d light=%0d requests=%0d CE0paid=%0d wall=%0d",pubs,nfinal,nsample,nlight,requests,paid_ce0,wall);$fclose(fd);$finish;
  end
  clk=1;#2;clk=0;#1;
 end
 $fatal(1,"bounded timeout cursor%0d global%0d",cursor,global_used);
end
initial begin #300000;$fatal(1,"wall watchdog");end
endmodule
'''
if __name__=='__main__':main()
