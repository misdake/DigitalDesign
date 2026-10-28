"""Bounded real-pin CPU scalar/LAST holds and GPU successor compatibility."""
from pathlib import Path
import re,os,subprocess,json,hashlib,sys
sys.dont_write_bytecode=True
from cl3_pins import with_cl3_pins, cl3_bridge_parameters
import argparse
ap=argparse.ArgumentParser(__doc__);ap.add_argument('--peer',type=Path,required=True);ap.add_argument('--output',type=Path,required=True);ap.add_argument('--bursts',type=int,default=8)
ap.add_argument('--late-bank',action='store_true',help='Vary read arrival, bank and row to cover preparation at the READ tail')
ap.add_argument('--display',action='store_true',help='Insert one strict-priority Display burst during GPU writes')
ap.add_argument('--cl3',action='store_true',help='Use board CL3/108 MHz and registered logic-clock read return')
ap.add_argument('--logic-skew-ns',type=float,default=0,help='Inject related logic-clock skew relative to controller (CL3 only)')
ap.add_argument('--pair-cq-ns',type=float,default=0,help='Inject read-pair/write-head clock-to-Q delay in the simulation fixture')
args=ap.parse_args()
assert 2 <= args.bursts <= (64 if args.late_bank else 32)
assert -0.35 <= args.logic_skew_ns <= 0.35
assert 0 <= args.pair_cq_ns <= 0.201
assert args.cl3 or (args.logic_skew_ns == 0 and args.pair_cq_ns == 0)
peer=args.peer.resolve();out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
ports=[]
for d,w,n in re.findall(r'\b(input|output|inout)\s+(?:wire\s+)?(\[[^]]+\]\s*)?(\w+)',(peer/'combination.v').read_text().split(');',1)[0]):
 ports.append(('reg ' if d=='input' else 'wire ')+w+n+('=0;' if d=='input' else ';'))
tb=r"""`timescale 1ns/1ps
module tb;
localparam BURSTS=@BURSTS@,LATE_BANK=@LATE_BANK@;
@PORTS@
always #5.051 controller_clk=~controller_clk;
always @(posedge controller_clk)logic_clk=~logic_clk;
initial begin #13.259;forever begin sdram_clk=~sdram_clk;#5.051;end end
GowinSdramCombination dut(.*);
LabPinModel #(.PERIOD_NS(10.102),.RETURN_MIN(1),.RETURN_MAX(4)) pin(
 .sclk(O_sdram_clk),.reset(reset),.cke(O_sdram_cke),.cs(O_sdram_cs_n),.ras(O_sdram_ras_n),
 .cas(O_sdram_cas_n),.we(O_sdram_wen_n),.dqm(O_sdram_dqm),.a(O_sdram_addr),.ba(O_sdram_ba),.dq(IO_sdram_dq),.cycle(),.refreshes());
integer cycle=0,i,n,hold_count,received,addr,gpu_words=0,gpu_requests=0;reg gpu_pending=0,write_test=0,read_test=0;integer read_offer_cycle=0,read_requests=0,read_terms=0,read_words=0,read_start=0,read_first=0,read_last=0;integer write_requests=0,write_words=0,write_terms=0,write_start=0,first_commit=0,last_commit=0;reg[63:0]kept;
function[20:0]nw;input integer bytes;integer w;
begin w=bytes/4;nw=((w>>5)&3)*524288+(w&31)+((w>>7)<<5);end endfunction
function[31:0]seed32;input integer address;begin seed32=32'h10000000|(address/4);end endfunction
function[31:0]write32;input integer index;begin write32=32'hb0000000|index;end endfunction
function integer read_addr;input integer index;begin
 if(LATE_BANK)read_addr='h18000+(((index*73)^(index>>2)^(index*index))%128)*128;
 else read_addr='h4000+index*128;
end endfunction
always @(negedge logic_clk)if(write_test)begin
 gpu_fb_w_request_valid=write_requests<BURSTS;gpu_fb_w_write=1;
 gpu_fb_w_address='h1200+write_requests*64;gpu_fb_w_line_count_minus_1=3;
 gpu_fb_w_write_data={write32(write_words*2+1),write32(write_words*2)};
end
always @(posedge logic_clk)if(!reset&&write_test)begin
 if(gpu_fb_w_request_valid&&gpu_fb_w_request_ready)write_requests=write_requests+1;
 if(gpu_fb_w_write_data_ready)write_words=write_words+1;
 if(gpu_fb_w_response_valid)begin
  if(!gpu_fb_w_response_last||gpu_fb_w_error||pin.mem[nw('h2400+(write_terms*32+31)*4)]!==write32(write_terms*32+31))$fatal(1,"write completion before pin commit");
  if(write_terms==0)first_commit=cycle;last_commit=cycle;
  write_terms=write_terms+1;
 end
end
always @(negedge logic_clk)if(read_test)begin
 gpu_ro_request_valid=read_requests<BURSTS&&cycle>=read_offer_cycle;gpu_ro_address=read_addr(read_requests)/2;
 case(read_requests%3)0:gpu_ro_line_count_minus_1=0;1:gpu_ro_line_count_minus_1=1;default:gpu_ro_line_count_minus_1=3;endcase
end
always @(posedge logic_clk)if(!reset)begin
 if(read_test)begin
  if(gpu_ro_request_valid&&gpu_ro_request_ready)begin
   if(LATE_BANK)read_offer_cycle=cycle+(read_requests*13)%21;
   read_requests=read_requests+1;
  end
  if(gpu_ro_response_valid)begin
   if(gpu_ro_error||gpu_ro_read_data!=={seed32(read_addr(read_terms)+read_words*8+4),seed32(read_addr(read_terms)+read_words*8)})$fatal(1,"mixed read data");
   if(gpu_ro_response_last!==(read_words==((4<<(read_terms%3))-1)))$fatal(1,"mixed read framing");
   if(gpu_ro_response_last)begin
    if(read_terms==0)read_first=cycle;read_last=cycle;
    read_words=0;read_terms=read_terms+1;
   end else read_words=read_words+1;
  end
 end else begin
 if(gpu_ro_request_valid&&gpu_ro_request_ready)begin gpu_pending=1;gpu_requests=gpu_requests+1;gpu_ro_request_valid<=0;end
 if(gpu_ro_response_valid)begin
  if(gpu_ro_error||!gpu_pending||received<4||gpu_ro_read_data!=={seed32('h2000+gpu_words*8+4),seed32('h2000+gpu_words*8)})$fatal(1,"GPU after held CPU LAST got=%h expected=%h words=%0d received=%0d error=%0d",gpu_ro_read_data,{seed32('h2000+gpu_words*8+4),seed32('h2000+gpu_words*8)},gpu_words,received,gpu_ro_error);
  if(gpu_ro_response_last!==(gpu_words==15))$fatal(1,"GPU response framing");
  gpu_words=gpu_words+1;
 end
 end
end
always @(posedge logic_clk)begin cycle=cycle+1;if(cycle>40000)$fatal(1,"memory watchdog CPU words=%0d GPU reads=%0d write requests=%0d write words=%0d completions=%0d",received,gpu_words,write_requests,write_words,write_terms);end
task scalar;input writing;input integer address,value;begin
 @(negedge logic_clk);data_line=0;data_write=writing;data_address=address/2;data_write_data=value;data_request_valid=1;data_response_ready=0;
 @(posedge logic_clk);while(!data_request_ready)@(posedge logic_clk);#1;data_request_valid=0;
 while(!data_response_valid)@(negedge logic_clk);
 if(data_error||(!writing&&data_read_data[15:0]!==value[15:0]))$fatal(1,"scalar addr%h got%h want%h",address,data_read_data,value);
 kept=data_read_data;
 repeat(3)begin @(posedge logic_clk);#1;if(!data_response_valid||data_read_data!==kept)$fatal(1,"scalar hold");end
 @(negedge logic_clk);data_response_ready=1;@(posedge logic_clk);#1;data_response_ready=0;
end endtask
task illegal_line;begin
 @(negedge logic_clk);data_line=1;data_write=0;data_address='h801;data_request_valid=1;data_response_ready=0;
 @(posedge logic_clk);while(!data_request_ready)@(posedge logic_clk);#1;data_request_valid=0;
 while(!data_response_valid)@(negedge logic_clk);
 if(!data_error)$fatal(1,"invalid CPU line was not rejected");
 repeat(3)begin @(posedge logic_clk);#1;if(!data_response_valid||!data_error)$fatal(1,"error hold");end
 @(negedge logic_clk);data_response_ready=1;@(posedge logic_clk);#1;data_response_ready=0;
end endtask
task line;input integer address;begin
 @(negedge logic_clk);data_line=1;data_write=0;data_address=address/2;data_request_valid=1;data_response_ready=1;
 @(posedge logic_clk);while(!data_request_ready)@(posedge logic_clk);#1;data_request_valid=0;received=0;hold_count=0;
 while(received<4)begin
  @(negedge logic_clk);data_response_ready=received!=3||hold_count==3;
  if(address=='h1080&&received>=2&&gpu_requests==0)begin gpu_ro_request_valid=1;gpu_ro_address='h1000;gpu_ro_line_count_minus_1=3;end
  else gpu_ro_request_valid=0;
  @(posedge logic_clk);
  if(data_response_valid)begin
   if(data_error||data_read_data!=={seed32(address+received*8+4),seed32(address+received*8)})$fatal(1,"CPU line word%0d data%h",received,data_read_data);
   if(data_response_ready)received=received+1;else hold_count=hold_count+1;
  end
 end
 #1;data_response_ready=0;
end endtask
initial begin
 for(i=0;i<(LATE_BANK?131072:32768);i=i+4)pin.seed_word(nw(i),32'h10000000|(i/4));
 reset=1;repeat(4)@(negedge logic_clk);reset=0;
 scalar(1,16'h1000,16'habc1);scalar(1,16'h1002,16'hdef2);
 scalar(0,16'h1000,16'habc1);scalar(0,16'h1002,16'hdef2);
 illegal_line;scalar(0,16'h1000,16'habc1);
 if(pin.mem[nw('h1000)]!==32'hdef2abc1)$fatal(1,"scalar write masks");
 line('h1080);while(gpu_words!=16)@(negedge logic_clk);gpu_ro_request_valid=0;line('h1800);line('h2000);
 scalar(1,16'h1010,16'h7654);scalar(0,16'h1010,16'h7654);
 @(negedge logic_clk);write_start=cycle;write_test=1;
 while(write_terms<BURSTS)@(negedge logic_clk);
 write_test=0;gpu_fb_w_request_valid=0;
 if(write_requests!=BURSTS||write_words!=BURSTS*16)$fatal(1,"write stream conservation");
 for(i=0;i<BURSTS*32;i=i+1)if(pin.mem[nw('h2400+i*4)]!==write32(i))$fatal(1,"write carry data%0d",i);
 $display("BURST_TIMING direction=write count=%0d work=%0d commit_span=%0d",BURSTS,cycle-write_start,last_commit-first_commit);
 @(negedge logic_clk);read_start=cycle;read_test=1;
 while(read_terms<BURSTS)@(negedge logic_clk);
 read_test=0;gpu_ro_request_valid=0;
 if(read_requests!=BURSTS)$fatal(1,"read stream conservation");
 $display("BURST_TIMING direction=read count=%0d work=%0d commit_span=%0d",BURSTS,cycle-read_start,read_last-read_first);
 $display("PASS CPU compatibility cycles=%0d: scalar mask, held scalar/LAST, line/row/direction, %0d varying GPU writes committed",cycle,BURSTS);$finish;
end
endmodule
""".replace('@PORTS@','\n'.join(ports)).replace('@BURSTS@',str(args.bursts)).replace('@LATE_BANK@',str(int(args.late_bank)))
if args.display:
 tb=tb.replace('initial begin\n for(i=0;', r"""reg display_armed=0;integer display_grants=0,display_words=0;
always @(negedge logic_clk)if(!reset&&write_test&&write_requests>=2&&!display_armed)begin
 display_armed=1;display_request_valid=1;display_address='hc00;display_response_ready=1;
end
always @(posedge logic_clk)if(!reset)begin
 if(display_request_valid&&gpu_fb_w_request_valid&&gpu_fb_w_request_ready)$fatal(1,"Display priority lost");
 if(display_request_valid&&display_request_ready)begin display_request_valid<=0;display_grants=display_grants+1;end
 if(display_response_valid)begin
  if(display_error||display_read_data!=={seed32('h1800+display_words*8+4),seed32('h1800+display_words*8)}||display_response_last!==(display_words==3))$fatal(1,"Display data/framing");
  display_words=display_words+1;
 end
end
initial begin
 for(i=0;""",1)
 tb=tb.replace(' $display("PASS CPU compatibility', ' if(display_grants!=1||display_words!=4)$fatal(1,"Display coverage");\n $display("DISPLAY_CONTENTION grants=%0d words=%0d",display_grants,display_words);\n $display("PASS CPU compatibility',1)
if args.cl3:
 tb=with_cl3_pins(tb,'dut.bridge.u_controller')
 tb=tb.replace('integer cycle=0,', 'integer write_chains=0,read_chains=0;\nalways @(posedge controller_clk)if(!reset)begin if(dut.bridge.chain_accept)write_chains=write_chains+1;if(dut.bridge.read_chain_accept)read_chains=read_chains+1;end\ninteger cycle=0,',1)
 tb=tb.replace(' $display("PASS CPU compatibility', ' if(write_chains==0||read_chains==0)$fatal(1,"missing physical continuation write=%0d read=%0d",write_chains,read_chains);\n $display("PHYSICAL_CHAIN write=%0d read=%0d",write_chains,read_chains);\n $display("PASS CPU compatibility',1)
 if args.logic_skew_ns != 0:
  # Use the common PLL source so negative relative skew never needs a negative delay.
  tb=tb.replace('always @(posedge controller_clk)logic_clk=~logic_clk;',
                f'always @(posedge source_clk)logic_clk<=#{2.271 + args.logic_skew_ns:.3f} ~logic_clk;')
(out/'tb.v').write_text(tb)
files=[]
for name in ['bridge.v','combination.v','controller.v','cpu_v3_memory_arbiter.v','pins.v','shared_port.v']:
 p=out/name;source=(peer/name).read_text()
 if args.cl3 and name=='combination.v':
  source=cl3_bridge_parameters(source)
 if name=='bridge.v' and args.pair_cq_ns:
  # Delay only simulation-visible producer Q, leaving every receiving register
  # and valid event intact. This is an injected witness, not measured board skew.
  for signal in ('pair_head','read_pair'):
   declaration=f'reg[63:0]{signal}=0'
   assert source.count(declaration)==1
   source=source.replace(declaration,f'reg[63:0]{signal}_q=0',1)
   source=re.sub(rf'\b{signal}\s*<=',f'{signal}_q<=',source)
   declaration_start=source.index(f'reg[63:0]{signal}_q=0')
   declaration_end=source.index(';',declaration_start)+1
   source=source[:declaration_end]+f'wire[63:0]{signal};assign #({args.pair_cq_ns:.3f}) {signal}={signal}_q;'+source[declaration_end:]
  source='`timescale 1ns/1ps\n'+source
 p.write_text(source);files.append(p)
for name,cmd in [('compile',[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',out/'sim.vvp',*files,out/'tb.v']),('run',[os.environ['VVP_EXE'],out/'sim.vvp'])]:
 r=subprocess.run(list(map(str,cmd)),cwd=out,capture_output=True,timeout=90)
 (out/(name+'.log')).write_bytes(r.stdout+r.stderr)
 if r.returncode:raise RuntimeError((r.stdout+r.stderr).decode()[-1800:])
result=dict(status='pass',cl3=args.cl3,logic_skew_ns=args.logic_skew_ns,pair_cq_ns=args.pair_cq_ns,display_contention=[s for s in (out/'run.log').read_text().splitlines() if s.startswith('DISPLAY_CONTENTION')],physical_chain=[s for s in (out/'run.log').read_text().splitlines() if s.startswith('PHYSICAL_CHAIN')],case=next(s for s in (out/'run.log').read_text().splitlines() if s.startswith('PASS')),burst_timing=[s for s in (out/'run.log').read_text().splitlines() if s.startswith('BURST_TIMING')], source_sha256={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in files},test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
(out/'receipt.json').write_text(json.dumps(result,indent=2));print(json.dumps({k:v for k,v in result.items() if k != 'source_sha256'}))
