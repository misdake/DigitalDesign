"""Correlate actual GPU handshakes; slot generations live only in this observer."""
from bisect import bisect_right
from collections import defaultdict, deque
from model import METRICS, union
from system_emu import integer, require

LOAD_COLUMNS = ('dma vt_owned vt_step t_queue r_busy i_busy quad_total quad_producer '
                'light_pending sample_pending light_tokens sample_quads final_wait final_tokens '
                'rop_contexts r_quad i_pixel light_issue sample_issue final_issue rop_lane '
                'vt_owner vt_kind r_owner i_owner prepare_busy prepare_owner texture_work '
                'texture_reserved texture_public fb_hint fb_maintenance ro_wait fb_read_wait '
                'fb_write_wait ce light_tick sample_tick reset').split()


def append_span(spans,span):
    if spans and spans[-1][1]==span[0] and spans[-1][2:]==span[2:]:spans[-1][1]=span[1]
    else:spans.append(span)


def normalize(data):
    s = data['system']; rows = s.get('gpu_journal', [])
    require(type(rows) is list and len(rows)<=(112000000 if s.get('runner') else 10000000),'GPU journal bounds')
    if not s['capabilities'].get('gpu_signal_journal'):
        require(not rows, 'GPU journal without capability')
        return None
    require(s.get('gpu_load_columns') == LOAD_COLUMNS, 'GPU load columns')
    axis, end = s['axis_origin_tick'], s['coverage_end_tick']
    cycle = lambda tick: (tick-axis)//148
    commands = {c['id']: c for c in data['commands']}
    source_subs={sub['id']:sub for sub in s['submissions']}
    by_sub = defaultdict(list)
    for c in commands.values(): by_sub[c['submission']].append(c)
    starts = {sid:[c['latch_tick'] for c in cs] for sid,cs in by_sub.items()}
    def owner(sid, tick):
        index = bisect_right(starts.get(sid, []), tick)-1
        require(index >= 0, 'GPU event before actual command latch')
        return by_sub[sid][index]
    jobs, transactions, meshes, quads, triangles = [], [], [], [], []
    job_slots, returns, pages = [None]*2, [None]*2, [None]*2
    t_slots, q_slots, contexts = [None]*16, [None]*32, [None]*2
    active, prepare, raster, interpolation = None, None, None, None
    public = [None]*16
    allocated, retired, published_t, released_t = 0, 0, 0, 0
    loads, cache, image_events, events, controls, masters = [], [], [], [], [], []
    master_payloads, master_terminals = [], []
    probes=[]
    edge_tick=None;edge_seen=set()
    for row in rows:
        require(type(row) is list and len(row) == 3, 'GPU journal envelope')
        tick, kind, value = row
        require(integer(tick) and s['origin_tick'] <= tick <= end and tick % 148 == 0
                and (not events or events[-1][0] <= tick), 'GPU journal tick')
        events.append(row)
        if tick!=edge_tick:edge_tick=tick;edge_seen.clear()
        if kind!='gpu_event':
            require(kind not in edge_seen,'duplicate GPU sample on one edge')
            edge_seen.add(kind)
        if kind == 'gpu_load':
            require(type(value) is list and len(value) == len(LOAD_COLUMNS)
                    and all(integer(v) for v in value), 'GPU load values')
            values = dict(zip(LOAD_COLUMNS, value))
            require(all(values[k]<=1 for k in ('dma','vt_owned','vt_step','r_busy','i_busy','prepare_busy',
                    'ce','light_tick','sample_tick','reset')) and values['vt_kind']<=5,'GPU load flags')
            if values['reset']:
                job_slots, returns, pages = [None]*2, [None]*2, [None]*2
                t_slots, q_slots, contexts, public = [None]*16, [None]*32, [None]*2, [None]*16
                active, prepare, raster, interpolation = None, None, None, None
                allocated, retired, published_t, released_t = 0, 0, 0, 0
            for key, _, metric_kind, capacity in METRICS:
                if key in values and capacity: require(values[key] <= capacity, 'GPU load capacity '+key)
            values.update(vt_owner=active['id'] if active else 0,
                          prepare_owner=prepare['id'] if prepare else 0,
                          r_owner=raster['id'] if raster else 0,
                          i_owner=interpolation['id'] if interpolation else 0)
            loads.append([tick, values])
            continue
        if kind == 'gpu_cache':
            require(type(value) is list and len(value) == 4 and type(value[0]) is list
                    and len(value[0]) == 8 and all(type(v) is bool for v in value[0])
                    and type(value[1]) is list and len(value[1]) == 8
                    and all(integer(v) and v < 1024 for v in value[1])
                    and all(integer(v) and v < 16384 for v in value[2:]), 'GPU cache state')
            if not cache or cache[-1][2]!=value:cache.append(row)
            continue
        if kind == 'gpu_control':
            require(type(value) is list and len(value) in (2,3) and integer(value[0])
                    and type(value[1]) is list and len(value[1])==9
                    and all(integer(v) for v in value[1]) and value[1][0]%4==0
                    and value[1][1]<=22 and all(v<=1 for v in value[1][2:])
                    and (len(value)==2 or integer(value[2]) and value[2]<=0xffffffff),'GPU CP predicate sample')
            if value[1][-1] and (not controls or controls[-1][2]!=value):controls.append(row)
            continue
        require(kind == 'gpu_event' and type(value) is list and len(value) == 3,
                'GPU event envelope')
        sid, name, args = value
        require(integer(sid) and type(name) is str and type(args) is list
                and all(integer(v) for v in args), 'GPU event values')
        key=(name,args[0] if name in ('cache_write','master_request','master_payload','master_terminal') and args else None)
        require(key not in edge_seen,'duplicate GPU handshake on one edge')
        edge_seen.add(key)
        if name not in ('target_reset','target_unbind'):
            sub=source_subs.get(sid)
            require(sub is not None and sub['start_tick']<=tick
                    and (sub['end_tick'] is None or tick<sub['end_tick']),'GPU event outside submission lifetime')
        def shape(count): require(len(args) == count, 'GPU event shape '+name)
        def slot(items, index):
            require(index < len(items), 'GPU slot range '+name)
            return items[index]
        def live(items, index):
            item = slot(items, index)
            require(item is not None, 'GPU unowned slot '+name)
            return item
        if name == 'dma_job':
            shape(6); place, mesh, page, source, size, blocks = args
            old = slot(job_slots, place)
            require(old is None or old['end_tick'] is not None, 'DMA overwrote live job')
            require(mesh <= 1 and page <= 1 and source < (1<<22) and size < 4 and blocks < 8,
                    'DMA job descriptor')
            c = owner(sid, tick)
            require(c['opcode'] in (1,2,11), 'DMA job command owner')
            job = dict(id=len(jobs)+1, submission=sid, command=c['id'], pc=c['pc'], mesh=bool(mesh),
                       slot=page, source=source, size=size, blocks=blocks,
                       start=cycle(tick), end=None, start_tick=tick, end_tick=None, transactions=[])
            jobs.append(job); job_slots[place] = job
            if mesh: pages[page] = job
        elif name == 'dma_request':
            shape(4); place, job_slot, address, count = args
            require(slot(returns,place) is None and job_slot <= 2 and address < (1<<22)
                    and count in (0,1,3), 'DMA request descriptor/slot')
            job = None if job_slot == 2 else live(job_slots,job_slot)
            tx = dict(id=len(transactions)+1, job=job['id'] if job else 0, submission=sid,
                      start=cycle(tick), end=None, start_tick=tick, end_tick=None,
                      address=address, count=count)
            transactions.append(tx); returns[place] = tx
            if job: job['transactions'].append(tx)
        elif name == 'dma_return':
            shape(1); tx = live(returns,args[0]); tx.update(end_tick=tick+148,end=cycle(tick)+1)
            returns[args[0]] = None
        elif name == 'dma_done':
            shape(1); job=live(job_slots,args[0]); require(job['end_tick'] is None,'duplicate DMA done')
            require(all(t['end_tick'] is not None for t in job['transactions']), 'DMA done before return')
            job.update(end_tick=tick+148,end=cycle(tick)+1)
        elif name == 'draw':
            shape(1); dma=live(pages,args[0]); c=owner(sid,tick)
            require(c['opcode'] == 3 and dma['end_tick'] is not None and dma['end_tick'] <= tick,
                    'DRAW owner/page not completed')
            require(dma['submission']==sid,'DRAW uses a page from a different submission')
            active=dict(id=len(meshes)+1, submission=sid, command=c['id'], pc=c['pc'], slot=args[0],
                        dma=dma['id'], start=cycle(tick), start_tick=tick, vt_end=None, name='Meshlet '+str(len(meshes)+1),
                        vt=[],r=[],i=[],p=[],prepare=[],final=[],rop=[],quads=0,triangles=0,last_quad=None)
            meshes.append(active)
        elif name == 'vt_end':
            shape(0); require(active is not None and active['vt_end'] is None,'VT end owner')
            active['vt_end']=cycle(tick)+1
        elif name == 't_publish':
            shape(1); require(active is not None and args[0]==published_t%16
                and slot(t_slots,args[0]) is None, 'T overwrite/unowned/generation')
            triangle=dict(id=len(triangles)+1,meshlet=active['id'],slot=args[0],publish_tick=tick,prepare_tick=None,release_tick=None)
            triangles.append(triangle);t_slots[args[0]]=triangle;active['triangles']+=1
            published_t+=1
        elif name == 'prepare':
            shape(1);triangle=live(t_slots,args[0]);require(triangle['prepare_tick'] is None,'duplicate triangle prepare')
            triangle['prepare_tick']=tick;prepare=meshes[triangle['meshlet']-1]
        elif name == 't_release':
            shape(1); require(args[0]==released_t%16,'T release order')
            triangle=live(t_slots,args[0]);require(triangle['prepare_tick'] is not None,'triangle release before prepare')
            triangle['release_tick']=tick+148;t_slots[args[0]]=None;released_t+=1
        elif name == 'r_start':
            shape(0); require(prepare is not None,'Raster missing prepare owner'); raster=prepare
        elif name == 'r_end':
            shape(0); require(raster is not None,'Raster end owner')
        elif name == 'i_context':
            shape(0); require(prepare is not None,'Interpolator missing prepare owner'); interpolation=prepare
        elif name == 'q_alloc':
            shape(1); old=slot(q_slots,args[0])
            require(args[0]==allocated%32 and allocated-retired<24 and raster is not None
                    and (old is None or old['retire_tick'] is not None), 'Quad overwrite/unowned/generation')
            q=dict(id=len(quads)+1,meshlet=raster['id'],slot=args[0],start_tick=tick,
                   publish_tick=None,retire_tick=None,rop_tick=None,commit_tick=None,final_tick=None,empty=None,mask=None)
            quads.append(q);q_slots[args[0]]=q;raster['quads']+=1
            allocated+=1
        elif name == 'q_publish':
            shape(2); q=live(q_slots,args[0]);require(q['publish_tick'] is None and args[1]<16,'quad publication')
            q.update(publish_tick=tick,mask=args[1])
        elif name == 'q_retire':
            shape(2);q=live(q_slots,args[0]);require(q['retire_tick'] is None and q['publish_tick'] is not None
                and args[0]==retired%32 and args[1]<=1 and bool(args[1]) == (q['mask']==0),'quad retirement')
            q.update(retire_tick=tick+148,empty=bool(args[1]))
            retired+=1
        elif name == 'final':
            shape(2);q=live(q_slots,args[0]);require(q['publish_tick'] is not None and args[1]<4,'Final owner')
            if q['final_tick'] is None:q['final_tick']=tick
        elif name == 'rop_begin':
            shape(2);q=live(q_slots,args[1]);require(slot(contexts,args[0]) is None and q['rop_tick'] is None,'ROP context overwrite')
            contexts[args[0]]=q;q['rop_tick']=tick
        elif name == 'cache_write':
            shape(5);bank,address,word,known,context=args;q=live(contexts,context)
            require(bank<4 and address<1024 and word<65536 and known==65535,'cache write bits/address')
            image_events.append([tick,'cache_write',[bank,address,word,meshes[q['meshlet']-1]['command']]])
        elif name == 'q_commit':
            shape(1);q=live(contexts,args[0]);require(q['commit_tick'] is None,'duplicate cache commit')
            q['commit_tick']=tick+148;contexts[args[0]]=None
        elif name == 'sample_publish':
            shape(1);q=live(q_slots,args[0]);place=args[0]&15
            require(public[place] is None and q['publish_tick'] is not None,'Sampling public owner overwrite')
            public[place]=q
        elif name == 'sample_last':
            shape(1);live(public,args[0]);public[args[0]]=None
        elif name == 'master_request':
            shape(3);port,address,count=args
            require(port<3 and address<(1<<22) and count in (0,1,3),'GPU master accepted descriptor')
            masters.append([tick,port,address,count])
        elif name == 'master_payload':
            shape(5);port,low,high,mask,error=args
            require(port<3 and low<=0xffffffff and high<=0xffffffff and mask<=255 and error<=1,'GPU master payload')
            master_payloads.append([tick,*args])
        elif name == 'master_terminal':
            shape(2);require(args[0]<3 and args[1]<=1,'GPU master terminal')
            master_terminals.append([tick,*args])
        elif name == 'texture_request': shape(0)
        elif name in ('vt_step','vt_rf_write','t_word','light_accept','light_result','sample_accept','sample_result'):
            shape({'vt_step':5,'vt_rf_write':3,'t_word':6,'light_accept':3,
                   'light_result':6,'sample_accept':2,'sample_result':4}[name])
            if name=='vt_step':require(args[0]<512 and args[1]<=1 and all(v<=0xffffffff for v in args[2:4]) and args[4]<256,'V/T ROM fields')
            if name=='vt_rf_write':require(args[0]<16 and args[1]<=0xffffffff and args[2]<=0xffffffff and args[1]&~args[2]==0,'V/T RF known bits')
            if name=='t_word':require(args[0]<16 and args[1]<16 and args[2]<=0xffffffff and args[3]<1<<22 and args[4]<=0xffffffff and args[5]<1<<22 and args[2]&~args[4]==0 and args[3]&~args[5]==0,'T output known bits/index')
            if name=='light_accept':require(args[0]<32 and args[1]<4 and args[2]<16,'Lighting tuple')
            if name=='light_result':require(args[0]<128 and args[1]<=1 and all(v<512 for v in args[2:]) and args[2]&~args[3]==0 and args[4]&~args[5]==0,'Lighting result known bits')
            if name=='sample_accept':require(args[0]<16 and args[1]<4,'Sampling tuple')
            if name=='sample_result':require(args[0]<64 and args[1]<=1 and args[2]<1<<24 and args[3]<1<<24 and args[2]&~args[3]==0,'Sampling result known bits')
            if name in ('vt_step','vt_rf_write'):
                require(active is not None,'V/T probe without DRAW owner');mesh=active
            elif name=='t_word':mesh=meshes[live(t_slots,args[0])['meshlet']-1]
            elif name=='light_accept':mesh=meshes[live(q_slots,args[0])['meshlet']-1]
            elif name=='light_result':mesh=meshes[live(q_slots,args[0]>>2)['meshlet']-1]
            else:
                q=live(public,args[0] if name=='sample_accept' else args[0]>>2)
                mesh=meshes[q['meshlet']-1]
            probes.append([tick,name,mesh['id'],args])
        elif name == 'write_owner':
            shape(1);require(args[0]<4,'framebuffer write owner mode')
            image_events.append([tick,'write_owner',[owner(sid,tick)['id'],args[0]]])
        elif name == 'target':
            shape(3);c=owner(sid,tick)
            require(c['opcode']==10 and args[2]<=1 and all(v%65536==0 and v<=8388608-262144 for v in args[:2]),
                    'committed target binding')
            image_events.append([tick,'target',[c['id'],*args]])
        elif name == 'target_reset':
            shape(0);image_events.append([tick,'target_reset',[]])
        elif name == 'target_unbind':
            shape(1);require(args[0]<=1,'target unbind reason')
            image_events.append([tick,'target_reset',[]])
        else: raise ValueError('Unknown committed GPU event: '+name)
    if not data['partial']:
        require(all(j['end_tick'] is not None for j in jobs) and not any(returns)
                and not any(t_slots) and not any(contexts) and not any(public), 'GPU owner left open at complete capture')
        require(all(q['publish_tick'] is not None and q['retire_tick'] is not None
                    and bool(q['commit_tick'] is not None) != bool(q['empty']) for q in quads),
                'GPU quad conservation at completion')
    tracks = {key:[] for key,*_ in METRICS}
    previous = {}
    for index,(tick, values) in enumerate(loads):
        until = loads[index+1][0] if index+1<len(loads) else end
        values.update(light_busy=int(values['light_tokens']>0), sample_busy=int(values['sample_quads']>0))
        for key,_,kind,_ in METRICS:
            value=values[key]
            if kind == 'rate':
                if value:tracks[key].append([cycle(tick),value])
            elif previous.get(key) != value:
                tracks[key].append([cycle(tick),value]);previous[key]=value
        for key,busy,owner_key in [('vt','vt_owned','vt_owner'),('r','r_busy','r_owner'),
                                  ('i','i_busy','i_owner'),('prepare','prepare_busy','prepare_owner')]:
            if values[busy] and values[owner_key]:
                mesh=meshes[values[owner_key]-1]
                append_span(mesh[key],[cycle(tick),cycle(until)] + ([values['vt_kind']] if key=='vt' else []))
    for q in quads:
        mesh=meshes[q['meshlet']-1]
        if q['retire_tick'] is not None:
            require(q['start_tick']<q['publish_tick']<q['retire_tick'],'GPU quad publication/retirement order')
        if q['commit_tick'] is not None:
            require(q['retire_tick'] is not None and q['retire_tick']<=q['commit_tick']
                    and q['publish_tick']<=q['rop_tick']<q['commit_tick'],'GPU quad cache commit order')
        finish=q['commit_tick'] if q['commit_tick'] is not None else q['retire_tick'] if q['empty'] else None
        for key,start in [('p',q['publish_tick']),('final',q['final_tick']),('rop',q['rop_tick'])]:
            stop=q['retire_tick'] if key=='final' else finish
            if start is not None: mesh[key].append([cycle(start),cycle(stop if stop is not None else end)])
        if finish is not None and (mesh['last_quad'] is None or cycle(finish)>mesh['last_quad']['end']):
            mesh['last_quad']=dict(id=q['id'],end=cycle(finish),empty=q['empty'],
                                  pool_retire=cycle(q['retire_tick']) if q['retire_tick'] is not None else None,
                                  cache_commit=cycle(q['commit_tick']) if q['commit_tick'] is not None else None)
    for mesh in meshes:
        for key in ('r','i','p','prepare','final','rop'):mesh[key]=union(mesh[key])
        mesh['open_quad_ids']=[q['id'] for q in quads if q['meshlet']==mesh['id']
                               and q['commit_tick'] is None and not(q['empty'] and q['retire_tick'] is not None)]
        own=[t for t in triangles if t['meshlet']==mesh['id']]
        mesh['open_triangle_ids']=[t['id'] for t in own if t['release_tick'] is None]
        mesh['t_end']=max((cycle(t['release_tick']) for t in own if t['release_tick'] is not None),default=None)
    for mesh in meshes:
        mesh['stats']={key:dict(active=sum(b-a for a,b in mesh[key]),
            first=mesh[key][0][0] if mesh[key] else None,last=mesh[key][-1][1] if mesh[key] else None)
            for key in ('r','i','p','prepare','final','rop')}
        from model import VT_KIND
        mesh['stats']['vt']={label:sum(b-a for a,b,k in mesh['vt'] if k==kind) for kind,label in VT_KIND.items()}
    definitions=[]
    for key,label,kind,capacity in METRICS:
        points=tracks[key]
        integral=sum(v for _,v in points) if kind=='rate' else sum(((points[i+1][0] if i+1<len(points) else cycle(end))-t)*v for i,(t,v) in enumerate(points))
        definitions.append(dict(key=key,label=label,kind=kind,capacity=capacity,
            peak=max((v for _,v in points),default=0),integral=integral))
    require(type(s['capabilities'].get('gpu_master_requests',False)) is bool
            and (s['capabilities'].get('gpu_master_requests') or not masters),'GPU master request capability')
    bindings=bind_services(data,masters) if s['capabilities'].get('gpu_master_requests') and s['capabilities'].get('shared_service64',True) else None
    return dict(jobs=jobs,dma_transactions=transactions,meshlets=meshes,quads=quads,triangles=triangles,
                tracks=tracks,metric_defs=definitions,
                cache=cache,image_events=image_events,controls=controls,
                master_requests=masters,service_bindings=bindings,
                **(dict(master_payloads=master_payloads,master_terminals=master_terminals)
                   if s['capabilities'].get('gpu_master_payloads') else {}),
                **(dict(probes=probes) if probes else {}))


def bind_services(data,masters):
    """The board's one-descriptor slots preserve each port's FIFO and fields.

    Acceptance before a slot and acceptance by the shared arbiter have different
    timestamps. Responses/payload bypass the slots and keep their real times.
    """
    queues=[deque() for _ in range(3)]
    for request in masters:queues[request[1]].append(request)
    bindings=[]
    for tx in data['system']['transactions']:
        if tx['port']<4:continue
        queue=queues[tx['port']-4]
        require(bool(queue),'shared GPU descriptor without a master acceptance')
        tick,port,address,count=queue.popleft()
        require(tick<=tx['start_tick'] and address==tx['address_halfword']
                and (count+1)*4==tx['requested_beats'],'GPU descriptor slot FIFO/field binding')
        bindings.append([tx['id'],tick])
    require(data['partial'] or not any(queues),'complete capture retains an unserved GPU descriptor slot')
    return bindings


def attach(data, gpu):
    """Add common viewer fields without manufacturing retirement for open owners."""
    if gpu is None: return data
    data.update({key:gpu[key] for key in ('jobs','dma_transactions','meshlets','tracks','metric_defs')})
    data['system']['gpu']={key:gpu[key] for key in ('quads','triangles','cache','image_events','master_requests','service_bindings')}
    for key in ('master_payloads','master_terminals','probes'):
        if key in gpu:data['system']['gpu'][key]=gpu[key]
    end=(data['system']['coverage_end_tick']-data['system']['axis_origin_tick'])//148
    for c in data['commands']:
        c['control_word_updates']=[]
        c['jobs']=[j['id'] for j in gpu['jobs'] if j['command']==c['id']]
        meshes=[m for m in gpu['meshlets'] if m['command']==c['id']]
        if meshes:c['meshlet']=meshes[0]['id']
        ends=[c['end'],*[j['end'] for j in gpu['jobs'] if j['command']==c['id']]]
        for m in meshes:
            ends.append(m['vt_end'])
            if m['open_quad_ids'] or m['open_triangle_ids']:ends.append(None)
            if m['t_end'] is not None:ends.append(m['t_end'])
            ends.extend(span[1] for key in ('vt','r','i','p','prepare','final','rop') for span in m[key])
            # A bounded capture cannot close a DRAW while its T/prepare/R/I
            # observation still touches the coverage boundary.
            if data['partial'] and any(span[1]>=end for key in ('vt','r','i','prepare') for span in m[key]):ends.append(None)
        c['complete_cycle']=None if any(v is None for v in ends) else max(ends)
    by_pc={(c['submission'],c['pc']):c for c in data['commands']}
    for index,(tick,_,record) in enumerate(gpu['controls']):
        sid,values=record[:2]
        pc,phase,render_idle,work_busy,dma_idle,memory_busy,flush,ce,busy=values
        c=by_pc.get((sid,pc))
        if c is None or not busy:continue
        if len(record)==3 and tick>=c['latch_tick']+148:
            updates=c['control_word_updates']
            if not updates or updates[-1][1]!=record[2]:updates.append([tick,record[2]])
        a=max((tick-data['system']['axis_origin_tick'])//148,c['start'])
        next_tick=gpu['controls'][index+1][0] if index+1<len(gpu['controls']) else data['system']['coverage_end_tick']
        b=min((next_tick-data['system']['axis_origin_tick'])//148,c['end'] if c['end'] is not None else end)
        if b<=a:continue
        append_span(c['states'],[a,b,phase])
        if c['opcode']==4 and phase==7:
            mask=int(c['word'],16)&3
            if mask&1 and work_busy:append_span(c['waits'],[a,b,'DMA'])
            if mask&2 and not render_idle:append_span(c['waits'],[a,b,'Render / cache drain'])
        elif c['opcode']==7 and phase==8 and not flush:append_span(c['waits'],[a,b,'Dirty framebuffer flush'])
        elif c['opcode']==15 and phase==9 and (not render_idle or not dma_idle or memory_busy):
            append_span(c['waits'],[a,b,'Render / DMA / memory drain'])
    for c in data['commands']:
        if c['execute'] is None or c['end'] is None:continue
        kinds={w[2] for w in c['waits']}
        if 'DMA' in kinds:
            c['dependencies'].extend(dict(pc=j['pc'],command=j['command'],time=j['end'],kind='DMA LAST')
                for j in gpu['jobs'] if j['submission']==c['submission'] and j['end'] is not None
                and j['start']<c['end'] and j['end']>c['execute'])
        if any(kind.startswith('Render') for kind in kinds):
            c['dependencies'].extend(dict(pc=m['pc'],command=m['command'],time=m['last_quad']['end'],kind='Last quad / cache commit')
                for m in gpu['meshlets'] if m['submission']==c['submission'] and m['last_quad']
                and c['execute']<m['last_quad']['end']<=c['end'])
        if c['opcode']==4 and not c['dependencies']:
            c['dependencies'].append(dict(pc=None,command=None,time=c['end']-1,
                kind='Predicate already satisfied / control edge' if not kinds else 'Observed predicate release / control edge'))
    data['memory']=memory_report(data,gpu)
    return data


def memory_report(data,gpu):
    """Reuse the GPU master's exact accounting at its original 64-bit boundary."""
    from memory import build
    s=data['system'];axis=s['axis_origin_tick'];events=defaultdict(list)
    cycle=lambda t:(t-axis)//148
    requests={(t['start_tick'],t['address'],t['count']):t for t in gpu['dma_transactions']}
    texture={t for t,k,v in s['gpu_journal'] if k=='gpu_event' and v[1]=='texture_request'}
    writers={t:args for t,kind,args in gpu['image_events'] if kind=='write_owner'}
    jobs={j['id']:j for j in gpu['jobs']}
    masters=gpu['master_requests'] if s['capabilities'].get('gpu_master_requests') else [
        [t['start_tick'],t['port']-4,t['address_halfword'],t['requested_beats']//4-1]
        for t in s.get('transactions',[]) if t['port']>=4]
    for tick,port,address,count in masters:
        source=5
        if port==0:
            key=(tick,address,count)
            require((key in requests) != (tick in texture),'GPU master source lineage')
            if key in requests:
                job=requests[key]['job'];source=1 if job==0 else 2 if jobs[job]['mesh'] else 3
            else:source=4
        if port==2:
            require(tick in writers,'GPU framebuffer accepted without write path')
            source=6 if writers[tick][1]==1 else 5
        events['MEM_REQUEST'].append([cycle(tick),port,address,count,source])
    if s['capabilities'].get('gpu_master_payloads'):
        for tick,port,error in gpu['master_terminals']:
            events['MEM_COMPLETE'].append([cycle(tick)+1,port,error])
        for tick,port,low,high,mask,error in gpu['master_payloads']:
            events['MEM_DATA'].append([cycle(tick),port,mask,error])
    else:
        for t in s.get('transactions',[]):
            if t['port']<4:continue
            port=t['port']-4
            if t['end_tick'] is not None:events['MEM_COMPLETE'].append([cycle(t['end_tick']),port,int(t['error'])])
        for tick,port,read,write in s.get('client_payload',[]):
            if port>=4:events['MEM_DATA'].append([cycle(tick),port-4,255,0])
    stop=max(1,(s['coverage_end_tick']-axis+147)//148)
    if any(row[0]>=stop for row in events['MEM_DATA']):stop+=1
    return build(events,0,stop,data.get('clock_hz',54000000),not data['partial'],data['commands'],data['tracks'],(0,stop))
