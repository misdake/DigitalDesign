/* Same offline viewer, full-system source. Raw stored time is integer quantum ticks. */
function renderSystemProfile(D) {
 const S=D.system,$=id=>document.getElementById(id),ports=['Display','I-cache','D-cache','DMA (disabled)','GPU RO','GPU FB read','GPU FB write'];
 let lo=S.origin_tick,hi=S.end_tick,unit='ms',bins=[],selection=0;
 const stateNames=['Startup hold','System hold','Cache sweep','CPU halted','CPU fault','GPU busy','GPU producer busy','GPU backend busy'];
 const phaseNames={0:'Idle',1:'Fetch',2:'Execute',3:'DRAW wait',4:'Apply read',5:'Apply write',7:'WAIT',8:'FLUSH',9:'END',10:'Failed',11:'Latch',12:'Clear request',13:'Clear wait',14:'Clear done',15:'Color read',16:'Color write',17:'DRAW start'};
 const channels=[...ports.filter((_,i)=>i!==3).map((name,i)=>({name,port:i<3?i:i+1})),{name:'Native model32',native:true}];
 const color=['#bec9d7','#91a4cf','#e6b45b','#a3adba','#f28e60','#56c69b','#ae86e7','#5faae8'];
 document.querySelector('h1').textContent='System emu profiler';
 $('identity').textContent='CPU + GPU + Display + shared SDRAM service';
 $('status').textContent=D.partial?'PARTIAL — incomplete coverage':'Cycle model · synthetic preload · no board qualification';
 document.querySelectorAll('.panehead h2')[0].textContent='CPU / GPU activity · submit / present';
 document.querySelectorAll('.panehead h2')[1].textContent='Shared SDRAM service · client64 / native model32';
 $('filters').hidden=true;$('focus').hidden=true;$('mouse-help').textContent='Wheel: zoom · overview drag: select · Fit: reset';
 $('legend-section').hidden=true;
 document.querySelector('#inspect-section summary').textContent='System capture';
 $('instruction').parentElement.hidden=true;
 const note=document.createElement('p');note.className='note';note.textContent=S.scope+' '+S.client_boundary+'. '+S.native_boundary+'. Precise command / meshlet ownership: unavailable.';$('detail').append(note);
 const hostSeconds=v=>Number.isFinite(v)?v.toFixed(3)+'s':'unavailable';
 const host=document.createElement('p');host.className='note';host.textContent=`Host: step ${hostSeconds(D.provenance.host_step_seconds)}, export ${hostSeconds(D.provenance.host_export_seconds)}. Host elapsed is separate from simulated time.`;$('detail').append(host);
 document.querySelector('#memory-section summary').textContent='Shared service memory';
 $('frame-label').textContent='Registered scanout';
 const choose=document.createElement('select');choose.setAttribute('aria-label','Registered scanout frame');
 S.scanouts.forEach((f,i)=>{const opt=document.createElement('option');opt.value=i;opt.textContent=`Raster ${f.frame} · ${f.effective_pixels} source pixels`;choose.append(opt);});
 $('frame-preview').append(choose);
 function frame(i){const f=S.scanouts[i];$('frame-wrap').hidden=!f;if(!f){$('frame-caption').textContent='No complete registered raster saved.';return;}
  $('frame-image').src=f.src;$('frame-large').src=f.src;$('frame-caption').textContent=`Ticks ${f.start_tick}–${f.end_tick}. Present association unavailable; ${f.applied_present_count} applied by end. No cache flush or second color conversion.`;}
 const initial=S.scanouts.reduce((best,f,i)=>f.effective_pixels>0?i:best,0);choose.value=String(initial);
 choose.addEventListener('change',()=>frame(Number(choose.value)));frame(initial);
 const axis=t=>unit==='cycles'?`${((t-S.axis_origin_tick)/148).toFixed(1)} cyc`:`${((t-S.axis_origin_tick)/S.quantum_hz*(unit==='us'?1e6:1e3)).toFixed(3)} ${unit}`;
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
 function draw(){
  $('range').textContent=axis(lo)+' – '+axis(hi);$('command-summary').textContent=`${S.submissions.length} submits · ${S.presents.length} present events · exact ticks`;
  const o=setup('commands',13*30);grid(o,390);
  for(let row=0;row<13;row++){const y=row*30;o.g.fillStyle='#a3adba';o.g.fillText(row<8?stateNames[row]:row===8?'CP phase':row===9?'Submit / drained':row===10?'Present request / applied':row===11?'CPU retire events':'CPU + GPU + Display pending',8,y+18);
   if(row<8){for(let i=0;i<S.states.length;i++){const [a,v]=S.states[i],b=S.states[i+1]?.[0]??S.coverage_end_tick;if(!v[row]||b<=lo||a>=hi)continue;o.g.fillStyle=color[row];o.g.fillRect(o.x(Math.max(a,lo)),y+7,Math.max(1,o.x(Math.min(b,hi))-o.x(Math.max(a,lo))),12);}}
   else if(row===8){for(let i=0;i<S.phases.length;i++){const [a,v]=S.phases[i],b=S.phases[i+1]?.[0]??S.coverage_end_tick;if(b<=lo||a>=hi)continue;o.g.fillStyle=color[v%8];o.g.fillRect(o.x(Math.max(a,lo)),y+7,Math.max(1,o.x(Math.min(b,hi))-o.x(Math.max(a,lo))),12);}}
   else if(row===9){for(const s of S.submissions){const b=s.end_tick??S.coverage_end_tick;if(b<lo||s.start_tick>=hi)continue;o.g.fillStyle='#56c69b';o.g.fillRect(o.x(Math.max(s.start_tick,lo)),y+7,Math.max(2,o.x(Math.min(b,hi))-o.x(Math.max(s.start_tick,lo))),12);}}
   else if(row===10){for(const [t,k]of S.presents)if(t>=lo&&t<hi){o.g.fillStyle=k==='present_request'?'#e6b45b':'#5faae8';o.g.fillRect(o.x(t),y+5,2,16);}}
   else if(row===11){o.g.fillStyle='#56c69b';for(const [t]of S.retires)if(t>=lo&&t<hi)o.g.fillRect(o.x(t),y+7,1,12);}
   else{for(let i=0;i<S.states.length;i++){const [a,v]=S.states[i],b=S.states[i+1]?.[0]??S.coverage_end_tick;if(!(v[8]&1)||!(v[8]&6)||!(v[8]&112)||b<=lo||a>=hi)continue;o.g.fillStyle='#e6b45b';o.g.fillRect(o.x(Math.max(a,lo)),y+7,Math.max(1,o.x(Math.min(b,hi))-o.x(Math.max(a,lo))),12);}}
  }
  for(const id of ['cmd-axis','load-axis']){const a=setup(id,27);a.g.fillStyle='#a3adba';for(let i=0;i<5;i++)a.g.fillText(axis(lo+(hi-lo)*i/5),a.x(lo+(hi-lo)*i/5),18);}
  const p=setup('load',channels.length*72+7*30);grid(p,channels.length*72+7*30);bins=makeBins(Math.max(2,Math.floor((p.w-232)/5)));
  channels.forEach((ch,row)=>{const y=row*72,values=bins.map(b=>ch.native?b.native:b.clients[ch.port]),peak=Math.max(1,...values.map(v=>v[0]+v[1]));p.g.fillStyle='#a3adba';p.g.fillText(ch.name+' · bytes/bin',8,y+18);p.g.fillText(ch.native?'4B slots / DQM separate':'8B payload · not pin bandwidth',8,y+34);
   bins.forEach((b,i)=>{if(b.start>=S.coverage_end_tick)return;const v=values[i],x=p.x(b.start),w=Math.max(.5,p.x(b.end)-x-1),r=v[0]/peak*48,wr=v[1]/peak*48;p.g.fillStyle='#5faae8';p.g.fillRect(x,y+62-r,w,r);p.g.fillStyle='#f28e60';p.g.fillRect(x,y+62-r-wr,w,wr);});
  });
  ports.forEach((name,port)=>{const y=channels.length*72+port*30;p.g.fillStyle='#a3adba';p.g.fillText(name+' request backpressure',8,y+18);for(let i=0;i<S.states.length;i++){const [a,v]=S.states[i],b=S.states[i+1]?.[0]??S.coverage_end_tick;if(!(v[9]&(1<<port))||b<=lo||a>=hi)continue;p.g.fillStyle='#e6b45b';p.g.fillRect(p.x(Math.max(a,lo)),y+7,Math.max(.5,p.x(Math.min(b,hi))-p.x(Math.max(a,lo))),12);}});
  if(S.coverage_end_tick<hi){p.g.fillStyle='#bec9d755';p.g.fillRect(p.x(Math.max(lo,S.coverage_end_tick)),0,p.w-p.x(Math.max(lo,S.coverage_end_tick)),channels.length*72+210);}
  $('resolution').textContent=`${bins.length} shared bins · exact bytes · blue read / orange write · shaded unknown`;
  const totals=channels.map(ch=>{const sum=bins.reduce((a,b)=>{const v=ch.native?b.native:b.clients[ch.port];return [a[0]+v[0],a[1]+v[1],a[2]+(v[2]||0)];},[0,0,0]);return `${ch.name}: R ${sum[0]}B, W slots ${sum[1]}B${ch.native?', unmasked '+sum[2]+'B':''}`;});$('gpu-memory-summary').textContent=totals.join(' · ');
  const ov=setup('overview',60),range=S.end_tick-S.origin_tick;ov.g.fillStyle='#353d49';ov.g.fillRect(220,14,ov.w-232,30);ov.g.fillStyle='#5faae8';ov.g.fillRect(220+(lo-S.origin_tick)/range*(ov.w-232),14,(hi-lo)/range*(ov.w-232),30);
 }
 function fit(a,b){lo=Math.max(S.origin_tick,a);hi=Math.min(S.end_tick,b);if(hi<=lo)hi=Math.min(S.end_tick,lo+148);draw();}
 for(const id of ['commands','load']){
  $(id).addEventListener('wheel',e=>{e.preventDefault();const x=e.offsetX,w=$(id).clientWidth,at=lo+(x-220)/(w-232)*(hi-lo),factor=e.deltaY>0?1.25:.8;fit(at-(at-lo)*factor,at+(hi-at)*factor);},{passive:false});
  $(id).addEventListener('mousemove',e=>{const t=lo+(e.offsetX-220)/($(id).clientWidth-232)*(hi-lo),b=bins.find(b=>b.start<=t&&t<b.end),s=stateAt(t),phase=S.phases.filter(p=>p[0]<=t).at(-1)?.[1];if(!b)return;$('cursor').textContent=`${axis(t)} · tick ${Math.floor(t)} · CP ${phaseNames[phase]??phase??'unknown'}\nBin [${Math.ceil(b.start)}, ${Math.ceil(b.end)}) ticks · ${b.end>S.coverage_end_tick?'PARTIAL/UNKNOWN':'covered'}\nClient R/W ${b.clients.map((v,i)=>ports[i]+':'+v.join('/')).join(' ')}\nNative32 R/W slot/unmasked ${b.native.join('/')} B\nAccepted counts ${b.accepted.join('/')} · requested bytes ${b.requested.join('/')} (missing descriptor sizes remain unavailable)\nOutstanding means ${b.outstanding.map(x=>x.toFixed(2)).join('/')}\nRequest wait logic-cycle equivalents ${b.request_wait.map(x=>x.toFixed(2)).join('/')}\nWrite terminal wait logic-cycle equivalents ${b.write_terminal_wait.map(x=>x.toFixed(2)).join('/')}\nRequest valid/wait masks ${s?s[8]+'/'+s[9]:'unavailable'}`;});
 }
 $('overview').addEventListener('pointerdown',e=>{selection=e.offsetX;$('overview').setPointerCapture(e.pointerId);});
 $('overview').addEventListener('pointerup',e=>{const w=$('overview').clientWidth-232,convert=x=>S.origin_tick+(x-220)/w*(S.end_tick-S.origin_tick);if(Math.abs(e.offsetX-selection)>3)fit(convert(Math.min(selection,e.offsetX)),convert(Math.max(selection,e.offsetX)));});
 $('fit').addEventListener('click',()=>fit(S.origin_tick,S.end_tick));$('unit').addEventListener('change',()=>{unit=$('unit').value;draw();});
 new ResizeObserver(draw).observe(document.querySelector('.main'));draw();
 window.systemProfiler={data:D,bins:()=>bins,fit:(a=S.origin_tick,b=S.end_tick)=>fit(a,b)};
}
