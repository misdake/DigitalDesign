// The real shared renderer, fed by the real finite GPU fixture report adapter.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const html=fs.readFileSync(process.argv[2],'utf8'),embedded=html.match(/^const D=(.*);$/m);
const canonical=JSON.parse(embedded[1]);
function run(data,search=''){
 const nodes=new Map(),noop=()=>{};let created=0;const resized=[];
 function el(id){if(nodes.has(id))return nodes.get(id);const listeners={},children=[],draws=[],captured=new Set();
  const ctx=new Proxy({measureText:s=>({width:s.length*6}),fillRect:(...a)=>draws.push(a)},{get:(o,k)=>k in o?o[k]:noop,set:(o,k,v)=>(o[k]=v,true)});
  const n={id,listeners,children,draws,dataset:{},style:{},clientWidth:1000,offsetHeight:70,scrollTop:0,innerHTML:'',textContent:'',value:'',parentElement:{clientWidth:1000},
   append:(...a)=>children.push(...a),replaceChildren:(...a)=>{children.length=0;children.push(...a);},
   setAttribute:(k,v)=>{n[k]=v;if(k==='id')nodes.set(v,n);},removeAttribute:k=>delete n[k],getAttribute:k=>n[k]??null,
   getContext:()=>ctx,getBoundingClientRect:()=>({left:0,top:0,width:1000,height:2000}),querySelector:()=>({style:{}}),
   addEventListener:(k,f)=>(listeners[k]??=[]).push(f),setPointerCapture:p=>captured.add(p),hasPointerCapture:p=>captured.has(p),releasePointerCapture:p=>captured.delete(p)};
  nodes.set(id,n);return n;
 }
 const context={document:{getElementById:el,createElement:tag=>el(tag+'-'+created++),createTextNode:t=>({textContent:t}),documentElement:{},querySelector:el,querySelectorAll:()=>[el('h2-a'),el('h2-b')]},
  getComputedStyle:()=>({getPropertyValue:k=>k==='--left'?'264px':'#ffffff'}),window:{devicePixelRatio:1,location:{search}},
  ResizeObserver:class{constructor(fn){resized.push(fn);}observe(){}},setTimeout:fn=>{fn();return 1;},clearTimeout:noop,innerWidth:1400,innerHeight:900,console};
 vm.createContext(context);
 let script=html.split('<script>')[1].split('</script>')[0].replace(/^const D=.*;$/m,'const D='+JSON.stringify(data)+';');
 script=script.replace('const $=','globalThis.viewerEval=code=>eval(code);\nconst $=');
 vm.runInContext(script,context,{timeout:20000});
 const emit=(id,type,args={})=>{for(const fn of el(id).listeners[type]??[])fn({button:0,pointerId:1,clientX:550,clientY:10,offsetX:550,preventDefault:noop,...args});};
 return {context,el,emit,resized};
}
for(const syntheticFrame of !canonical.system.frame_view&&canonical.system.submissions.length>=2?[false,true]:[false]){
 const data=JSON.parse(JSON.stringify(canonical));
 if(syntheticFrame){const sub=data.system.submissions[1];data.system.frame_view={presentation:2,start_tick:sub.start_tick,end_tick:data.system.end_tick,owned_submission_ids:[2],
  scanout_index:Math.max(0,data.system.scanouts.findIndex(f=>f.applied_present_count===2)),window_scope:'Synthetic UI boundary fixture',ownership_scope:'Submission2 only',session_report:'../../profiler.html'};}
 const frame=data.system.frame_view;
 const before=JSON.stringify(data),{context,el,emit,resized}=run(data),ui=context.window.gpuProfiler;
 assert.ok(ui);assert.equal(context.window.systemProfiler,undefined);
 const expected=frame?data.commands.filter(c=>frame.owned_submission_ids.includes(c.submission)):data.commands;
 const expectedSubmissions=data.system.submissions.filter(s=>!frame||frame.owned_submission_ids.includes(s.id));
 assert.equal(ui.data.submissions.length,expectedSubmissions.length);
 assert.ok(ui.data.submissions.every(s=>typeof s.name==='string'&&s.name.length>0));
 if(frame)assert.ok(ui.data.submissions.every(s=>s.frame_id===frame.presentation));
 assert.equal(ui.data.commands.length,expected.length);
 assert.ok(el('commands').draws.length>0&&el('load').draws.length>0);
 assert.ok(el('system-source-scope').textContent.includes(data.system.scope));
 assert.equal(ui.data.cpu.length,0);
 if(!data.images?.available&&data.system.scanouts.length){assert.ok(el('frame-label').textContent.includes('registered scanout')||el('frame-label').textContent==='Registered scanout');assert.ok(el('frame-image').src.startsWith('data:image/png;base64,'));}
 emit('filter-all','click');assert.ok(el('command-summary').textContent.includes(data.system.capabilities.command_control_lineage?expected.length+' / '+expected.length:'Instruction trace not captured'));
 // An explicit native-image budget may omit every warm-up draw. Focus an
 // actually captured image when one belongs to this view; absence is valid.
 const nativeDraw=expected.find(c=>c.opcode===3&&data.images?.commands?.[String(c.id)]?.depth_file);
 const draw=nativeDraw??expected.find(c=>c.opcode===3)??expected[0];if(draw){ui.focus(draw.id);
 assert.equal(ui.getView().selected,draw.id);
 assert.ok(el('detail').innerHTML.includes('Actual quantum ticks'));
 if(draw.opcode===3)assert.ok(el('detail').innerHTML.includes('Published triangles'));
 }
 if(nativeDraw)assert.ok(el('frame-caption').children.some(n=>n.textContent?.includes('Native D16')));
 if(!data.system.capabilities.gpu_signal_journal){assert.ok(el('system-source-scope').textContent.includes('Missing tracks are unavailable, not zero'));assert.ok(el('detail').innerHTML.includes('unavailable'));}
 const lo=((frame?data.system.frame_view.start_tick:data.system.origin_tick)-data.system.axis_origin_tick)/148,
       hi=((frame?data.system.frame_view.end_tick:data.system.end_tick)-data.system.axis_origin_tick)/148;
 assert.ok(Number(el('gpu-memory-summary').dataset.rangeStart)>=lo);
 assert.ok(Number(el('gpu-memory-summary').dataset.rangeEnd)<=hi);
 ui.fit();assert.equal(ui.getView().lo,lo);assert.equal(ui.getView().hi,hi);
 for(const resize of resized)resize();assert.equal(ui.getView().lo,lo);assert.equal(ui.getView().hi,hi);
 emit('commands','wheel',{ctrlKey:true,deltaY:-1});assert.ok(ui.getView().lo>=lo&&ui.getView().hi<=hi);
 ui.fit();emit('filter-none','click');assert.equal(el('command-empty').hidden,false);emit('filter-all','click');
 if(frame){assert.ok(!el('gpu-memory-summary').innerHTML.includes('Full capture reference'));assert.ok(el('range').textContent.includes('Frame '+frame.presentation+' window'));
  assert.ok(ui.data.commands.every(c=>frame.owned_submission_ids.includes(c.submission)));assert.ok(ui.data.meshlets.every(m=>frame.owned_submission_ids.includes(m.submission)));
  if(ui.data.images)assert.ok(Object.keys(ui.data.images.commands).every(id=>expected.some(c=>String(c.id)===id)));
 }
 assert.equal(vm.runInContext('JSON.stringify(D)',context),before,'view projection mutated canonical facts');
}
const overview=run(canonical,'?view=system');assert.ok(overview.context.window.systemProfiler);
assert.equal(overview.context.window.gpuProfiler,undefined);
if(!canonical.system.capabilities.client_payload64&&!canonical.system.capabilities.native_model32){
 overview.emit('load','mousemove',{offsetX:550});
 assert.ok(overview.el('gpu-memory-summary').textContent.includes('unavailable'));
 assert.ok(overview.el('cursor').textContent.includes('unavailable'));
 assert.ok(!overview.el('cursor').textContent.includes('Display:0/0'));
}
const f=canonical.system.frame_view;
assert.equal(overview.context.window.systemProfiler.bins().reduce((n,b)=>n+b.clients.reduce((s,v)=>s+v[0]+v[1],0),0),canonical.system.client_payload.filter(v=>!f||f.start_tick<=v[0]&&v[0]<f.end_tick).reduce((n,v)=>n+v[2]+v[3],0));
console.log('Shared GPU viewer passed: actual lineage/commands and available native raw links; system view keeps client bytes; frame lists, statistics, Fit, resize and zoom stay scoped; canonical facts unchanged.');
