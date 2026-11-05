/* Same offline viewer, full-system source. Raw stored time is integer quantum ticks. */
function renderSystemProfile(D) {
 const S=D.system,$=id=>document.getElementById(id),ports=['Display','I-cache','D-cache','DMA (disabled)','GPU RO','GPU FB read','GPU FB write'];
 const F=S.frame_view,boundLo=F?.start_tick??S.origin_tick,boundHi=F?.end_tick??S.end_tick,axisOrigin=F?boundLo:S.axis_origin_tick;
 const submissions=F?S.submissions.filter(s=>F.owned_submission_ids.includes(s.id)):S.submissions;
 const presents=F?S.presents.filter(p=>(p[1]==='present_request'?p[2]:p[2][0])===F.presentation):S.presents;
 const scanouts=F?(F.scanout_index===null?[]:[S.scanouts[F.scanout_index]]):S.scanouts;
 const commands=F?D.commands.filter(c=>F.owned_submission_ids.includes(c.submission)):D.commands;
 const visibleOps=new Set(commands.map(c=>c.opcode).filter(op=>![1,2,4].includes(op)));
 let selected=null;
 const commandTick=cycle=>S.axis_origin_tick+cycle*148;
 let lo=boundLo,hi=boundHi,unit='ms',bins=[],selection=0;
 const stateNames=['Startup hold','System hold','Cache sweep','CPU halted','CPU fault','GPU busy','GPU producer busy','GPU backend busy'];
 const phaseNames={0:'Idle',1:'Fetch',2:'Execute',3:'DRAW wait',4:'Apply read',5:'Apply write',7:'WAIT',8:'FLUSH',9:'END',10:'Failed',11:'Latch',12:'Clear request',13:'Clear wait',14:'Clear done',15:'Color read',16:'Color write',17:'DRAW start',18:'UI wait',19:'Copy address',20:'Copy request',21:'Copy wait',22:'Copy done'};
 const channels=[...(S.capabilities.client_payload64?ports.filter((_,i)=>i!==3).map((name,i)=>({name,port:i<3?i:i+1})):[]),...(S.capabilities.native_model32?[{name:'Native model32',native:true}]:[])];
 const color=['#bec9d7','#91a4cf','#e6b45b','#a3adba','#f28e60','#56c69b','#ae86e7','#5faae8'];
 document.querySelector('h1').textContent=F?'Application frame '+F.presentation+' · system profiler':S.domains?.cpu===false?'GPU capture · optional system context':'System emu profiler';
 {const link=document.createElement('a');link.href='?view=gpu';link.textContent='GPU profiler · instructions / pipeline / images';document.querySelector('header').append(link);}
 $('identity').textContent=F?'Presentation '+F.presentation+' · observed time window + GPU-owned submissions':S.domains?'Observed: '+Object.keys(S.domains).filter(k=>S.domains[k]).join(', '):'CPU + GPU + Display + shared SDRAM service';
 $('status').textContent=D.partial?'PARTIAL — incomplete coverage':'Cycle model · synthetic preload · no board qualification';
 document.querySelectorAll('.panehead h2')[0].textContent='CPU / GPU activity · submit / present';
 document.querySelectorAll('.panehead h2')[1].textContent='Shared SDRAM service · client64 / native model32';
 $('filters').hidden=!commands.length;$('focus').hidden=!commands.length;$('mouse-help').textContent='Wheel: zoom · overview drag: select · click: inspect command · Fit: reset';
 $('legend-section').hidden=true;
 document.querySelector('#inspect-section summary').textContent='System capture';
 $('instruction').parentElement.hidden=!commands.length;
 const note=document.createElement('p');note.className='note';note.textContent=S.scope+' '+S.client_boundary+'. '+S.native_boundary+'. '+(S.capabilities.gpu_signal_journal?'Detailed asynchronous GPU ownership is in the GPU pipeline view.':'Precise command / meshlet ownership: unavailable.');$('detail').append(note);
 const cb=document.createElement('p');cb.className='note';cb.setAttribute('id','cb-capability');cb.textContent=S.capabilities.command_control_lineage?'CB details: actual committed submit descriptor and RAM instruction words. First enabled Exec, accepted issue, CP release and END drain are distinct.':'CB instruction details: not captured by this legacy system source. CP phase is controller state, not a command PC/opcode sequence.';$('detail').append(cb);
 const inspector=document.createElement('p');inspector.className='note';inspector.setAttribute('id','cb-inspector');$('detail').append(inspector);
 function inspect(){const c=commands.find(c=>c.id===selected);inspector.textContent=c?`CB${c.submission} · PC +0x${c.pc.toString(16)} · physical 0x${c.address_bytes.toString(16)} · ${c.name} 0x${c.word}\nActual latch tick ${c.latch_tick}; first enabled Exec ${c.execute_tick??'not observed'}; accepted issue ${c.issue_tick??'open'}; CP release ${c.release_tick??'open'}; residency end ${c.end===null?'OPEN':commandTick(c.end)}. CP release does not establish asynchronous fragment/ROP completion.`:'Select a captured instruction to inspect its actual word and boundaries.';}
 function visibleCommands(){return commands.filter(c=>visibleOps.has(c.opcode));}
 function rebuildCommands(){
  if(!visibleCommands().some(c=>c.id===selected))selected=null;
  const options=[{id:'',text:'Session overview'},...visibleCommands().map(c=>({id:c.id,text:`CB${c.submission} · ${c.pc.toString(16)} ${c.name}`}))];
  $('instruction').replaceChildren(...options.map(v=>{const n=document.createElement('option');n.value=String(v.id);n.textContent=v.text;return n;}));$('instruction').value=selected===null?'':String(selected);
  $('filter-count').textContent=`(${visibleCommands().length}/${commands.length})`;$('command-empty').hidden=!commands.length||visibleCommands().length>0;inspect();
 }
 const types=[...new Map(commands.map(c=>[c.opcode,c.name]))];
 for(const [op,name]of types){const label=document.createElement('label'),input=document.createElement('input');input.type='checkbox';input.checked=visibleOps.has(op);input.setAttribute('id','system-filter-'+op);input.addEventListener('change',()=>{input.checked?visibleOps.add(op):visibleOps.delete(op);rebuildCommands();draw();});label.append(input,document.createTextNode(`${name} (${commands.filter(c=>c.opcode===op).length})`));$('filter-options').append(label);}
 for(const mode of ['all','none','default'])$('filter-'+mode).addEventListener('click',()=>{visibleOps.clear();for(const [op]of types){if(mode==='all'||mode==='default'&&![1,2,4].includes(op))visibleOps.add(op);$('system-filter-'+op).checked=visibleOps.has(op);}rebuildCommands();draw();});
 $('instruction').addEventListener('change',()=>{selected=Number($('instruction').value)||null;inspect();draw();});
 $('focus').addEventListener('click',()=>{const c=commands.find(c=>c.id===selected);if(c)fit(commandTick(c.start),c.end===null?S.coverage_end_tick:commandTick(c.end));});
 rebuildCommands();
 if(F){
  const program=D.provenance.report_inputs?.files?.['program.bin']??'unavailable',app=D.provenance.application_identity?.source??'unavailable';
  const info=document.createElement('p');info.className='note';info.setAttribute('id','frame-scope');
  info.textContent=`Frame ${F.presentation} · ticks [${boundLo}, ${boundHi}). App source SHA: ${app}. Program SHA256: ${program}. ${F.window_scope} ${F.ownership_scope} CPU frame owner and exact HUD wait-return ticks: unavailable.`;$('detail').append(info);
  const counts=document.createElement('p');counts.className='note';counts.setAttribute('id','frame-statistics');
  const c=F.summary.cpu_counter;counts.textContent=`Selected GPU jobs: ${submissions.length}; window accepted ${F.summary.accepted_in_window}, terminal events ${F.summary.terminal_events_in_window}, carry-in ${F.summary.carry_in}, carry-out ${F.summary.carry_out}. CPU counter delta ${c?c.retired_words+' words from saved baseline tick '+c.baseline_tick:'unavailable'}; concurrent window activity, frame ownership unavailable.`;$('detail').append(counts);
  if(F.session_report){const link=document.createElement('a');link.href=F.session_report;link.textContent='Open separate full-session report';$('detail').append(link);}
 }
 const hostSeconds=v=>Number.isFinite(v)?v.toFixed(3)+'s':'unavailable';
 const host=document.createElement('p');host.className='note';host.textContent=`Host: step ${hostSeconds(D.provenance.host_step_seconds)}, export ${hostSeconds(D.provenance.host_export_seconds)}. Host elapsed is separate from simulated time.`;$('detail').append(host);
 document.querySelector('#memory-section summary').textContent='Shared service memory';
 $('frame-label').textContent='Registered scanout';
 const choose=document.createElement('select');choose.setAttribute('aria-label','Registered scanout frame');
 scanouts.forEach((f,i)=>{const opt=document.createElement('option');opt.value=i;opt.textContent=`Raster ${f.frame??'number unavailable'} · ${f.effective_pixels} source pixels`;choose.append(opt);});
 $('frame-preview').append(choose);
 function frame(i){const f=scanouts[i];$('frame-wrap').hidden=!f;if(!f){$('frame-caption').textContent='No complete registered raster saved.';return;}
  $('frame-image').src=f.src;$('frame-large').src=f.src;$('frame-caption').textContent=F?`Presentation ${F.presentation}, stable raster ${f.frame??'number unavailable'}, scanout ticks ${f.start_tick}–${f.end_tick}. Scanout interval is separate from the plotted applied-to-applied window.`:`Ticks ${f.start_tick}–${f.end_tick}. Present association unavailable; ${f.applied_present_count} applied by end. No cache flush or second color conversion.`;}
 const initial=scanouts.reduce((best,f,i)=>f.effective_pixels>0?i:best,0);choose.value=String(initial);
 choose.addEventListener('change',()=>frame(Number(choose.value)));frame(initial);
 const axis=t=>unit==='cycles'?`${((t-axisOrigin)/148).toFixed(1)} cyc`:`${((t-axisOrigin)/S.quantum_hz*(unit==='us'?1e6:1e3)).toFixed(3)} ${unit}`;
 function setup(id,height){const c=$(id),ratio=window.devicePixelRatio||1,w=Math.max(360,c.clientWidth||800);c.width=w*ratio;c.height=height*ratio;c.style.height=height+'px';const g=c.getContext('2d');g.scale(ratio,ratio);g.font='11px system-ui';return {g,w,x:t=>220+(t-lo)/(hi-lo)*(w-232)};}
 function grid(o,height){o.g.strokeStyle='#2a3039';o.g.fillStyle='#a3adba';for(let i=0;i<=5;i++){const t=lo+(hi-lo)*i/5,x=o.x(t);o.g.beginPath();o.g.moveTo(x,0);o.g.lineTo(x,height);o.g.stroke();}}
 function stateAt(t){let left=0,right=S.states.length;while(left<right){const mid=(left+right)>>1;if(S.states[mid][0]<=t)left=mid+1;else right=mid;}return left?S.states[left-1][1]:null;}
 function makeBins(n){const step=(hi-lo)/n,rows=Array.from({length:n},(_,i)=>({start:lo+i*step,end:i===n-1?hi:lo+(i+1)*step,clients:Array.from({length:7},()=>[0,0]),native:[0,0,0],outstanding:Array(7).fill(0),request_wait:Array(7).fill(0),write_terminal_wait:Array(7).fill(0),accepted:Array(7).fill(0),requested:Array(7).fill(0)}));
  for(const [t,p,r,w]of S.client_payload)if(t>=lo&&t<hi){const k=Math.min(n-1,Math.floor((t-lo)/step));rows[k].clients[p][0]+=r;rows[k].clients[p][1]+=w;}
  for(const [t,r,w,u]of S.native_model32)if(t>=lo&&t<hi){const k=Math.min(n-1,Math.floor((t-lo)/step));rows[k].native[0]+=r;rows[k].native[1]+=w;rows[k].native[2]+=u;}
  function spans(a,b,visit){if(b<=lo||a>=hi)return;const first=Math.max(0,Math.floor((a-lo)/step)),last=Math.min(n-1,Math.floor((b-lo)/step));for(let k=first;k<=last;k++){const duration=Math.max(0,Math.min(b,rows[k].end)-Math.max(a,rows[k].start));if(duration)visit(rows[k],duration);}}
  for(const t of S.transactions){const end=Math.min(t.end_tick??S.coverage_end_tick,S.coverage_end_tick);spans(t.start_tick,end,(b,d)=>b.outstanding[t.port]+=d/(b.end-b.start));if(t.writing&&t.last_data_tick!==null)spans(t.last_data_tick+148,end,(b,d)=>b.write_terminal_wait[t.port]+=d/148);if(t.start_tick>=lo&&t.start_tick<hi){const b=rows[Math.min(n-1,Math.floor((t.start_tick-lo)/step))];b.accepted[t.port]++;if(t.requested_beats!==null)b.requested[t.port]+=t.requested_beats*8;}}
  for(let i=0;i<S.states.length;i++){const [a,v]=S.states[i],end=S.states[i+1]?.[0]??S.coverage_end_tick;spans(a,end,(b,d)=>{for(let p=0;p<7;p++)if(v[9]&(1<<p))b.request_wait[p]+=d/148;});}
  return rows;
 }
 function ownedSpans(a,b){return F?submissions.map(s=>[Math.max(a,s.start_tick),Math.min(b,s.end_tick??S.coverage_end_tick)]).filter(p=>p[0]<p[1]):[[a,b]];}
 function draw(){
  $('range').textContent=axis(lo)+' – '+axis(hi);$('command-summary').textContent=(F?`${submissions.length} frame-owned submits · 1 request / 1 applied · window-only statistics`:`${submissions.length} submits · ${presents.filter(p=>p[1]==='present_request').length} request / ${presents.filter(p=>p[1]==='present_applied').length} applied`)+` · ${visibleCommands().length}/${commands.length} commands`;
  const o=setup('commands',(13+visibleCommands().length)*30);grid(o,(13+visibleCommands().length)*30);
  for(let row=0;row<13;row++){const y=row*30;o.g.fillStyle='#a3adba';o.g.fillText(row<8?stateNames[row]:row===8?'CP phase':row===9?'Submit / drained':row===10?'Present request / applied':row===11?(F?'CPU concurrent · owner unavailable':'CPU retire events'):'CPU + GPU + Display pending',8,y+18);
   if(row<8){for(let i=0;i<S.states.length;i++){const [a,v]=S.states[i],b=S.states[i+1]?.[0]??S.coverage_end_tick;if(!v[row]||b<=lo||a>=hi)continue;o.g.fillStyle=color[row];for(const [start,end]of(F&&row>=5?ownedSpans(a,b):[[a,b]])){if(end<=lo||start>=hi)continue;o.g.fillRect(o.x(Math.max(start,lo)),y+7,Math.max(1,o.x(Math.min(end,hi))-o.x(Math.max(start,lo))),12);}}}
   else if(row===8){for(let i=0;i<S.phases.length;i++){const [a,v]=S.phases[i],b=S.phases[i+1]?.[0]??S.coverage_end_tick;if(b<=lo||a>=hi)continue;o.g.fillStyle=color[v%8];for(const [start,end]of ownedSpans(a,b)){if(end<=lo||start>=hi)continue;o.g.fillRect(o.x(Math.max(start,lo)),y+7,Math.max(1,o.x(Math.min(end,hi))-o.x(Math.max(start,lo))),12);}}}
   else if(row===9){for(const s of submissions){const b=s.end_tick??S.coverage_end_tick;if(b<lo||s.start_tick>=hi)continue;o.g.fillStyle='#56c69b';o.g.fillRect(o.x(Math.max(s.start_tick,lo)),y+7,Math.max(2,o.x(Math.min(b,hi))-o.x(Math.max(s.start_tick,lo))),12);}}
   else if(row===10){for(const [t,k]of presents)if(t>=lo&&t<hi){o.g.fillStyle=k==='present_request'?'#e6b45b':'#5faae8';o.g.fillRect(o.x(t),y+5,2,16);}}
   else if(row===11){o.g.fillStyle='#56c69b';for(const [t]of S.retires)if(t>=lo&&t<hi)o.g.fillRect(o.x(t),y+7,1,12);}
   else{for(let i=0;i<S.states.length;i++){const [a,v]=S.states[i],b=S.states[i+1]?.[0]??S.coverage_end_tick;if(!(v[8]&1)||!(v[8]&6)||!(v[8]&112)||b<=lo||a>=hi)continue;o.g.fillStyle='#e6b45b';o.g.fillRect(o.x(Math.max(a,lo)),y+7,Math.max(1,o.x(Math.min(b,hi))-o.x(Math.max(a,lo))),12);}}
  }
  for(const [i,c]of visibleCommands().entries()){const y=(13+i)*30,a=commandTick(c.start),b=c.end===null?S.coverage_end_tick:commandTick(c.end);o.g.fillStyle='#a3adba';o.g.fillText(`CB${c.submission} +${c.pc.toString(16)} ${c.name}${c.end===null?' · OPEN':''}`,8,y+18);if(b<=lo||a>=hi)continue;o.g.fillStyle=c.id===selected?'#df8ac1':'#7d9aa2';o.g.fillRect(o.x(Math.max(a,lo)),y+7,Math.max(1,o.x(Math.min(b,hi))-o.x(Math.max(a,lo))),12);if(c.issue_tick!==null&&c.issue_tick>=lo&&c.issue_tick<hi){o.g.fillStyle='#56c69b';o.g.fillRect(o.x(c.issue_tick),y+4,2,18);}}
  for(const id of ['cmd-axis','load-axis']){const a=setup(id,27);a.g.fillStyle='#a3adba';for(let i=0;i<5;i++)a.g.fillText(axis(lo+(hi-lo)*i/5),a.x(lo+(hi-lo)*i/5),18);}
  const p=setup('load',channels.length*72+7*30);grid(p,channels.length*72+7*30);bins=makeBins(Math.max(2,Math.floor((p.w-232)/5)));
  channels.forEach((ch,row)=>{const y=row*72,values=bins.map(b=>ch.native?b.native:b.clients[ch.port]),peak=Math.max(1,...values.map(v=>v[0]+v[1]));p.g.fillStyle='#a3adba';p.g.fillText(ch.name+' · bytes/bin',8,y+18);p.g.fillText(ch.native?'4B slots / DQM separate':'8B payload · not pin bandwidth',8,y+34);
   bins.forEach((b,i)=>{if(b.start>=S.coverage_end_tick)return;const v=values[i],x=p.x(b.start),w=Math.max(.5,p.x(b.end)-x-1),r=v[0]/peak*48,wr=v[1]/peak*48;p.g.fillStyle='#5faae8';p.g.fillRect(x,y+62-r,w,r);p.g.fillStyle='#f28e60';p.g.fillRect(x,y+62-r-wr,w,wr);});
  });
  ports.forEach((name,port)=>{const y=channels.length*72+port*30;p.g.fillStyle='#a3adba';p.g.fillText(name+' request backpressure',8,y+18);for(let i=0;i<S.states.length;i++){const [a,v]=S.states[i],b=S.states[i+1]?.[0]??S.coverage_end_tick;if(!(v[9]&(1<<port))||b<=lo||a>=hi)continue;p.g.fillStyle='#e6b45b';p.g.fillRect(p.x(Math.max(a,lo)),y+7,Math.max(.5,p.x(Math.min(b,hi))-p.x(Math.max(a,lo))),12);}});
  if(S.coverage_end_tick<hi){p.g.fillStyle='#bec9d755';p.g.fillRect(p.x(Math.max(lo,S.coverage_end_tick)),0,p.w-p.x(Math.max(lo,S.coverage_end_tick)),channels.length*72+210);}
  $('resolution').textContent=`${bins.length} shared bins · exact bytes · blue read / orange write · shaded unknown`;
  const totals=channels.map(ch=>{const sum=bins.reduce((a,b)=>{const v=ch.native?b.native:b.clients[ch.port];return [a[0]+v[0],a[1]+v[1],a[2]+(v[2]||0)];},[0,0,0]);return `${ch.name}: R ${sum[0]}B, W slots ${sum[1]}B${ch.native?', unmasked '+sum[2]+'B':''}`;});$('gpu-memory-summary').textContent=totals.length?totals.join(' · '):'Shared service / native memory observations unavailable; GPU master accounting is in the GPU pipeline view.';
  const ov=setup('overview',60),range=boundHi-boundLo;ov.g.fillStyle='#353d49';ov.g.fillRect(220,14,ov.w-232,30);ov.g.fillStyle='#5faae8';ov.g.fillRect(220+(lo-boundLo)/range*(ov.w-232),14,(hi-lo)/range*(ov.w-232),30);
 }
 $('commands').addEventListener('click',e=>{const c=visibleCommands()[Math.floor(e.offsetY/30)-13];if(c){selected=c.id;$('instruction').value=String(c.id);inspect();draw();}});
 function fit(a,b){lo=Math.min(boundHi-1,Math.max(boundLo,a));hi=Math.min(boundHi,Math.max(lo+1,b));draw();}
 for(const id of ['commands','load']){
  $(id).addEventListener('wheel',e=>{e.preventDefault();const x=e.offsetX,w=$(id).clientWidth,at=lo+(x-220)/(w-232)*(hi-lo),factor=e.deltaY>0?1.25:.8;fit(at-(at-lo)*factor,at+(hi-at)*factor);},{passive:false});
  $(id).addEventListener('mousemove',e=>{const t=lo+(e.offsetX-220)/($(id).clientWidth-232)*(hi-lo),b=bins.find(b=>b.start<=t&&t<b.end),s=stateAt(t),phase=S.phases.filter(p=>p[0]<=t).at(-1)?.[1];if(!b)return;if(!S.capabilities.client_payload64&&!S.capabilities.native_model32){$('cursor').textContent=`${axis(t)} · Shared service/native measurements unavailable. GPU master measurements are in the GPU pipeline view.`;return;}$('cursor').textContent=`${axis(t)} · tick ${Math.floor(t)} · CP ${phaseNames[phase]??phase??'unknown'}\nBin [${Math.ceil(b.start)}, ${Math.ceil(b.end)}) ticks · ${b.end>S.coverage_end_tick?'PARTIAL/UNKNOWN':'covered'}\nClient R/W ${b.clients.map((v,i)=>ports[i]+':'+v.join('/')).join(' ')}\nNative32 R/W slot/unmasked ${b.native.join('/')} B\nAccepted counts ${b.accepted.join('/')} · requested bytes ${b.requested.join('/')} (missing descriptor sizes remain unavailable)\nOutstanding means ${b.outstanding.map(x=>x.toFixed(2)).join('/')}\nRequest wait logic-cycle equivalents ${b.request_wait.map(x=>x.toFixed(2)).join('/')}\nWrite terminal wait logic-cycle equivalents ${b.write_terminal_wait.map(x=>x.toFixed(2)).join('/')}\nRequest valid/wait masks ${s?s[8]+'/'+s[9]:'unavailable'}`;});
 }
 $('overview').addEventListener('pointerdown',e=>{selection=e.offsetX;$('overview').setPointerCapture(e.pointerId);});
 $('overview').addEventListener('pointerup',e=>{const w=$('overview').clientWidth-232,convert=x=>boundLo+(x-220)/w*(boundHi-boundLo);if(Math.abs(e.offsetX-selection)>3)fit(convert(Math.min(selection,e.offsetX)),convert(Math.max(selection,e.offsetX)));});
 $('fit').textContent=F?'Fit frame':'Fit session';$('fit').addEventListener('click',()=>fit(boundLo,boundHi));$('unit').addEventListener('change',()=>{unit=$('unit').value;draw();});
 new ResizeObserver(draw).observe(document.querySelector('.main'));draw();
 window.systemProfiler={data:D,bins:()=>bins,commands:()=>visibleCommands(),select:id=>{selected=id;inspect();draw();},fit:(a=boundLo,b=boundHi)=>fit(a,b)};
}

/* The common GPU renderer consumes the same normalized model as RTL captures.
   This is a read-only view projection; canonical lifetimes stay in profile.json. */
function systemGpuView(source){
 const S=source.system,F=S.frame_view,tick=t=>(t-S.axis_origin_tick)/148;
 const start=tick(F?.start_tick??S.origin_tick),end=tick(F?.end_tick??S.end_tick),coverage=tick(S.coverage_end_tick);
 const own=sid=>!F||F.owned_submission_ids.includes(sid);
 const observedSubmissions=source.submissions.length?source.submissions:S.submissions.map(s=>({
  id:s.id,start:tick(s.start_tick),end:s.end_tick===null?null:tick(s.end_tick),base:null,bytes:null,
  frame_id:F?.presentation??null,name:'Observed GPU submission; CB descriptor not captured'}));
 const submissions=observedSubmissions.filter(s=>own(s.id)).map(s=>({...s,
  frame_id:F?.presentation??s.frame_id??null,name:s.name??'Observed GPU submission #'+s.id}));
 const spans=submissions.map(s=>[Math.max(start,s.start),Math.min(end,s.end??coverage)]).filter(([a,b])=>a<b);
 const ownedTime=t=>spans.some(([a,b])=>a<=t&&t<b);
 function mask(points,kind){
  if(!F)return points;
  if(kind==='rate'||kind==='bytes')return points.filter(p=>ownedTime(p[0]));
  const result=[];
  function put(t,v){if(result.at(-1)?.[0]===t)result[result.length-1]=[t,v];else if(result.at(-1)?.[1]!==v)result.push([t,v]);}
  for(const [a,b]of spans){let value=0;for(const [t,v]of points){if(t>a)break;value=v;}put(a,value);for(const [t,v]of points)if(a<t&&t<b)put(t,v);put(b,0);}
  return result;
 }
 const tracks={},metric_defs=source.metric_defs.map(d=>{
  const points=mask(source.tracks[d.key],d.kind);tracks[d.key]=points;let integral=0,peak=0;
  for(let i=0;i<points.length;i++){const [t,v]=points[i];if(d.kind==='rate'){if(start<=t&&t<Math.min(end,coverage)){integral+=v;peak=Math.max(peak,v);}}
   else{const a=Math.max(start,t),b=Math.min(end,coverage,points[i+1]?.[0]??coverage);if(a<b){integral+=(b-a)*v;peak=Math.max(peak,v);}}}
  return {...d,integral,peak};
 });
 let memory=source.memory;
 if(F&&memory?.available){const timeline=memory.timeline;memory={...memory,transactions:memory.transactions.filter(t=>ownedTime(t.start)),
  timeline:{...timeline,data:timeline.data.filter(p=>ownedTime(p[0])),
   states:Object.fromEntries(Object.entries(timeline.states).map(([k,p])=>[k,mask(p,'queue')])),
   backpressure:{...timeline.backpressure,track:mask(timeline.backpressure.track??[],'queue')}}};}
 const commands=source.commands.filter(c=>own(c.submission)).map(c=>({...c,
  observed_open:c.end===null,asynchronous_open:c.complete_cycle===null,actual_end:c.end,
  execute:c.execute??Math.min(coverage,tick(c.latch_tick)+1),end:c.end??coverage}));
 let images=source.images;
 if(images&&source.viewer_image_sources){const hydrate=im=>({...im,src:source.viewer_image_sources[im.src_ref]});
  images={...images,commands:Object.fromEntries(Object.entries(images.commands).map(([k,v])=>[k,hydrate(v)])),final:images.final?hydrate(images.final):null};}
 if(F&&images){const ids=new Set(commands.map(c=>String(c.id))),chosen=Object.fromEntries(Object.entries(images.commands).filter(([id])=>ids.has(id)));
  images={...images,commands:chosen,available:Object.keys(chosen).length>0,final:Object.keys(chosen).length?{...Object.values(chosen).sort((a,b)=>a.cycle-b.cycle||a.command-b.command).at(-1),command:null}:null};}
 return {...source,start,end,view_axis_origin:F?start:0,submissions,commands,
  instruction_trace_available:!!S.capabilities.command_control_lineage,pipeline_trace_available:!!S.capabilities.gpu_signal_journal,
  meshlets:source.meshlets.filter(m=>own(m.submission)),jobs:source.jobs.filter(j=>own(j.submission)),
  dma_transactions:source.dma_transactions.filter(t=>own(t.submission)),tracks,metric_defs,memory,images,
  sample_start:Math.max(start,tick(S.origin_tick)),sample_end:Math.min(end,coverage),
  submit:submissions[0]?.start??start,gpu_end:Math.min(end,Math.max(start,...submissions.map(s=>s.end??coverage)))};
}
