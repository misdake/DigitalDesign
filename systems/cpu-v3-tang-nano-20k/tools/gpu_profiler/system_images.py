"""Native historical color/D16 from the one backing and committed cache journal."""
import base64
import hashlib
import json
from pathlib import Path
import shutil
import struct
from images import History, preview, preview_policy
from layout import COLUMN
from system_emu import require, native_time


def selected_submissions(data, limit):
    s=data['system'];requests=[(t,v) for t,k,v in s['presents'] if k=='present_request']
    explicit=s.get('image_journal',{}).get('selected_presentations')
    if explicit is not None:
        require(type(explicit) is list and len(explicit)<=limit<=4 and explicit==sorted(set(explicit))
                and all(type(n) is int and 1<=n<=len(requests) for n in explicit),'selected presentation groups')
        selected=[]
        for n in explicit:
            tick=requests[n-1][0];previous=requests[n-2][0] if n>1 else s['origin_tick']-1
            selected.extend(sub['id'] for sub in s['submissions'] if previous<sub['start_tick']<=tick
                            and sub['end_tick'] is not None and sub['end_tick']<=tick)
        return selected,'Explicit ordinary-run presentation groups; all preceding physical/cache writes retained'
    if requests:
        selected=[];previous=s['origin_tick']-1
        for tick,_ in requests[:limit]:
            selected.extend(sub['id'] for sub in s['submissions'] if previous<sub['start_tick']<=tick
                            and sub['end_tick'] is not None and sub['end_tick']<=tick)
            previous=tick
        if len(requests)<limit:
            proven={c['submission'] for c in data['commands'] if c.get('complete_cycle') is not None}
            selected.extend(sub['id'] for sub in s['submissions'] if sub['start_tick']>previous and sub['id'] in proven)
        return selected,'first presentation target groups, including only proven command prefixes of an unpresented group'
    proven={c['submission'] for c in data['commands'] if c.get('complete_cycle') is not None}
    return [sub['id'] for sub in s['submissions'] if sub['end_tick'] is not None or sub['id'] in proven][:limit], 'first GPU submission target scopes, including proven command prefixes; no presentation request observed'


def select_target(data,targets,command,commands=None):
    axis=data['system']['axis_origin_tick']
    if commands is None:commands={c['id']:c for c in data['commands']}
    at=axis+(command['complete_cycle'] if command['opcode']==10 else command['execute'])*148
    bound=next((target for tick,target in reversed(targets) if tick<=at),None)
    if bound is not None:return dict(bound,selection='newly-bound-target' if command['opcode']==10 else 'bound-at-execute')
    # As in a replay's declared output preimage, this is a real buffer at the
    # snapshot time. Its identity comes from a later observed binding in this
    # same CB, never from decoding object bytes or pretending it was bound.
    future=next((target for tick,target in targets if tick>at and target is not None
                 and commands[target['command']]['submission']==command['submission']),None)
    return None if future is None else dict(future,epoch=None,selection='selected-output-preimage',
        binding='Selected output preimage before GPU binding; later observed SET_TARGET #'+str(future['command']))


def build(data, preimage, physical_writes, output, *, limit=None, preview_mode='display1', known_preimage=None):
    budget=data['system'].get('image_journal',{}).get('completed_target_budget',2)
    if limit is None:limit=budget
    require(type(limit) is int and 1<=limit<=4 and (limit<=2 or
            budget==4 and 'selected_presentations' in data['system'].get('image_journal',{})),
            'native completed-target budget')
    require(len(preimage)==8388608,'native preimage size')
    s=data['system'];axis=s['axis_origin_tick'];gpu=s['gpu']
    selected,selection_scope=selected_submissions(data,limit)
    known=bytes([1])*len(preimage) if known_preimage is None else known_preimage
    require(len(known)==len(preimage) and all(v in (0,1) for v in known),'native known-preimage mask')
    history=History(preimage,known,COLUMN)
    linear,presentation=preview_policy(preview_mode,1,True)
    timeline=[];targets=[]
    writers={t:args for t,kind,args in gpu['image_events'] if kind=='write_owner'}
    master_ticks=dict(gpu.get('service_bindings') or [])
    for tick,kind,args in gpu['image_events']:
        if kind=='target':
            cid,color,depth,valid=args
            require(valid==1,'unbound committed target')
            target=dict(epoch=len(targets)+1,color=color,depth=depth,binding='actual committed SET_TARGET',command=cid)
            targets.append((tick+148,target));timeline.append((tick+148,2,'target',target))
        elif kind=='target_reset':
            targets.append((tick+148,None));timeline.append((tick+148,2,'invalidate',None))
        elif kind=='cache_write':
            bank,address,word,owner=args
            timeline.append((tick+148,0,'cache',((address>>5)&7,bank,address,word,owner)))
    for tick,kind,args in gpu['cache']:timeline.append((tick,3,'state',args))
    writes=[t for t in s['transactions'] if t['writing']]
    if not s.get('capabilities',{}).get('shared_service64',True):
        writes=[dict(port=6,writing=True,start_tick=t,address_halfword=address,requested_beats=(count+1)*4)
                for t,port,address,count in gpu['master_requests'] if port==2]
        terminals=[t+148 for t,port,error in gpu['master_terminals'] if port==2]
        for i,tx in enumerate(writes):tx['end_tick']=terminals[i] if i<len(terminals) else None
    index,offset=0,0;previous=s['origin_tick']
    for row in physical_writes:
        require(type(row) is list and len(row)==3 and row[1]=='physical_write','physical journal envelope')
        tick,_,args=row
        rational=data.get('provenance',{}).get('schema')=='digitaldesign.profiling-capture' and data['provenance'].get('version')==2
        require((native_time(tick) if rational else type(tick) is int) and previous<=tick<=s['coverage_end_tick'] and (rational or tick%74==37),
                'physical journal time')
        previous=tick
        require(type(args) is list and len(args)==3 and all(type(v) is int and v>=0 for v in args),
                'physical write fields')
        address,word,mask=args
        require(index<len(writes),'physical write without an accepted descriptor')
        tx=writes[index]
        require(tx['requested_beats'] is not None and tx['start_tick']<=tick
                and address==tx['address_halfword']*2+offset and word<=0xffffffff and mask<16,
                'physical address/order/mask differs from accepted write')
        owner=0 # CPU writes have their own real time, not a GPU command owner.
        if tx['port']==6:
            master_tick=master_ticks.get(tx.get('id'),tx['start_tick'])
            require(master_tick in writers,'physical GPU write without accepted path owner')
            command,mode=writers[master_tick];owner=None if mode==0 else command
        else: require(tx['port']==2,'unexpected physical writer')
        timeline.append((tick,1,'bus',(address,word,mask,owner)))
        offset+=4
        if offset==tx['requested_beats']*8:index+=1;offset=0
    if not data['partial']:require(index==len(writes) and offset==0,'incomplete physical write journal')
    else:require(all(t.get('end_tick') is None for t in writes[index:]),'completed writer has missing physical journal')
    unavailable={};commands={c['id']:c for c in data['commands']}
    image_limit=s.get('image_journal',{}).get('max_command_images')
    if image_limit is not None:
        require(type(image_limit) is int and 1<=image_limit<=2048, 'native command image limit')
        eligible=sum(c['submission'] in selected and c.get('complete_cycle') is not None
                     and select_target(data,targets,c,commands) is not None for c in commands.values())
        require(eligible<=image_limit,'native command image budget exceeded; no images silently dropped')
    for c in commands.values():
        if c['submission'] not in selected:unavailable[str(c['id'])]='Outside explicit completed-target budget'
        elif c.get('complete_cycle') is None:unavailable[str(c['id'])]='Asynchronous owner remains open'
        elif s.get('image_journal',{}).get('truncated') and axis+c['complete_cycle']*148>=s['coverage_end_tick']//148*148:
            unavailable[str(c['id'])]='Event limit cut the final edge; completion journal is not closed'
        else:timeline.append((axis+c['complete_cycle']*148,4,'snapshot',c['id']))
    output=Path(output);output.mkdir(parents=True,exist_ok=True)
    assets={};images={};last_state=None
    def asset(raw):
        sha=hashlib.sha256(raw).hexdigest();name='plane-'+sha+'.bin'
        if name not in assets:(output/name).write_bytes(raw);assets[name]=dict(sha256=sha,bytes=len(raw))
        return name,sha
    for tick,_,kind,args in sorted(timeline,key=lambda row:(row[0],row[1])):
        if kind=='target':history.epoch=args['epoch']
        elif kind=='invalidate':history.epoch=0
        elif kind=='state':
            key=(history.epoch,json.dumps(args,separators=(',',':')))
            if key==last_state:continue
            last_state=key;valid,tags,color,depth=args
            for slot in range(8):history.state(slot,valid[slot],tags[slot],history.epoch,color*512,depth*512)
        elif kind=='cache':history.cache(tick,*args)
        elif kind=='bus':
            address,word,mask,owner=args
            for half in range(2):
                enabled=(mask>>(half*2))&3
                if enabled==0:continue
                at=address+half*2;old=struct.unpack_from('<H',history.physical,at)[0]
                bits=(255 if enabled&1 else 0)|(65280 if enabled&2 else 0)
                value=(old&~bits)|((word>>(half*16))&bits)
                history.bus(tick,at,value,owner)
        else:
            c=commands[args]
            target=select_target(data,targets,c,commands)
            if target is None:unavailable[str(c['id'])]='No GPU target bound at this command';continue
            color,overrides=history.plane(target['color'],target['epoch'],c['id'])
            depth,_=history.plane(target['depth'],target['epoch'],c['id'])
            color_file,color_sha=asset(color);depth_file,depth_sha=asset(depth)
            name='native-'+color_sha+'.png'
            if not (output/name).exists():preview(color,output/name,linear,COLUMN)
            png=(output/name).read_bytes()
            images[str(c['id'])]=dict(command=c['id'],cycle=c['complete_cycle'],
                target_base_bytes=target['color'],depth_base_bytes=target['depth'],target_epoch=target['epoch'],
                binding=target['binding'],format=1,cache_override_words=overrides,preview=presentation,
                target_selection=target['selection'],target_binding_command=target['command'],
                plane_sha256=color_sha,depth_plane_sha256=depth_sha,color_file=color_file,depth_file=depth_file,
                file=name,png_sha256=hashlib.sha256(png).hexdigest(),src='data:image/png;base64,'+base64.b64encode(png).decode())
    final=dict(max(images.values(),key=lambda v:(v['cycle'],v['command'])),command=None) if images else None
    result=dict(available=bool(images),complete=True,scope='Native 400x240 tiled RGB565/D16 at proven completion; valid cache included; younger GPU command owners excluded',
                selection_scope=selection_scope,completed_target_budget=limit,selected_submission_ids=selected,
                final=final,commands=images,unavailable_commands=unavailable,assets=assets,
                preimage_sha256=hashlib.sha256(preimage).hexdigest(),partial_capture=data['partial'])
    if not images:result['reason']='No proven command image with a bound target in the selected completed groups'
    (output/'native-index.json').write_text(json.dumps({k:v for k,v in result.items() if k not in ('commands','final')},indent=2)+'\n')
    return result


def copy_assets(data, source, output):
    """Portable raw downloads are exact files, never decoded from a screenshot."""
    images=data.get('images',{})
    if not images.get('available'):return
    source,output=Path(source)/'images',Path(output)/'images';output.mkdir(parents=True,exist_ok=True)
    for name,identity in images.get('assets',{}).items():
        require(Path(name).name==name and name.startswith('plane-') and name.endswith('.bin'),'native asset path')
        raw=(source/name).read_bytes()
        require(len(raw)==identity['bytes']==COLUMN.plane_bytes
                and hashlib.sha256(raw).hexdigest()==identity['sha256'],'native raw asset binding')
        if (source/name).resolve()!=(output/name).resolve():shutil.copyfile(source/name,output/name)


def project(data, owned):
    original=data.get('images')
    if original is None:return data
    ids={str(c['id']) for c in data['commands'] if c['submission'] in owned}
    commands={key:value for key,value in original['commands'].items() if key in ids}
    files={image[key] for image in commands.values() for key in ('color_file','depth_file')}
    images=dict(original,commands=commands,available=bool(commands),
                final=dict(max(commands.values(),key=lambda v:(v['cycle'],v['command'])),command=None) if commands else None,
                selected_submission_ids=[sid for sid in original['selected_submission_ids'] if sid in owned],
                unavailable_commands={k:v for k,v in original['unavailable_commands'].items() if k in ids},
                assets={name:identity for name,identity in original['assets'].items() if name in files})
    return dict(data,images=images)


def validate(data):
    images=data.get('images')
    if images is None:return
    require(type(images) is dict and type(images.get('available')) is bool
            and images.get('completed_target_budget')==data['system'].get('image_journal',{}).get('completed_target_budget',2)
            and images['completed_target_budget'] in (2,4),'native image scope')
    s=data['system'];scope=s.get('frame_view',{}).get('owned_submission_ids')
    image_limit=s.get('image_journal',{}).get('max_command_images')
    if image_limit is not None:
        require(type(image_limit) is int and 1<=image_limit<=2048
                and len(images['commands'])<=image_limit,'native command image limit')
    selected,_=selected_submissions(data,images['completed_target_budget'])
    if scope is not None:selected=[sid for sid in selected if sid in scope]
    require(images.get('selected_submission_ids')==selected,'native image submission scope')
    commands={str(c['id']):c for c in data['commands'] if scope is None or c['submission'] in scope}
    all_commands={c['id']:c for c in data['commands']};targets=[]
    for tick,kind,args in s['gpu']['image_events']:
        if kind=='target':
            cid,color,depth,valid=args
            targets.append((tick+148,dict(command=cid,color=color,depth=depth,epoch=len(targets)+1,binding='actual committed SET_TARGET')))
        elif kind=='target_reset':targets.append((tick+148,None))
    require(set(images['commands']).isdisjoint(images['unavailable_commands'])
            and set(images['commands'])|set(images['unavailable_commands'])==set(commands),'native image coverage declaration')
    require(images['available']==bool(images['commands']),'native image availability')
    require(images['preimage_sha256']==data['provenance']['report_inputs']['files']['initial-memory.bin'],
            'native image preimage binding')
    for name,identity in images['assets'].items():
        require(name=='plane-'+identity['sha256']+'.bin' and len(identity['sha256'])==64
                and identity['bytes']==COLUMN.plane_bytes,'native raw image identity')
    for key,image in images['commands'].items():
        c=commands[key]
        require(c['submission'] in selected and c.get('complete_cycle') is not None
                and image['command']==c['id'] and image['cycle']==c['complete_cycle'],'native image completion owner')
        target=select_target(data,targets,c,all_commands)
        require(target is not None and (image['target_base_bytes'],image['depth_base_bytes'],image['target_epoch'],
                image.get('target_binding_command'),image.get('target_selection'),image['binding'])==
                (target['color'],target['depth'],target['epoch'],target['command'],target['selection'],target['binding']),
                'native image target binding differs from observed selection')
        require(image['target_base_bytes']%65536==0 and image['depth_base_bytes']%65536==0
                and image['format']==1,'native image target')
        for field,sha in [('color_file','plane_sha256'),('depth_file','depth_plane_sha256')]:
            require(image[field] in images['assets'] and images['assets'][image[field]]['sha256']==image[sha],
                    'native image plane binding')
        png=base64.b64decode(image['src'].removeprefix('data:image/png;base64,'),validate=True)
        require(image['src'].startswith('data:image/png;base64,') and len(png)<=1000000
                and png.startswith(b'\x89PNG\r\n\x1a\n') and struct.unpack('>II',png[16:24])==(400,240)
                and hashlib.sha256(png).hexdigest()==image['png_sha256'],'native image PNG binding')
        require(image['file']=='native-'+image['plane_sha256']+'.png','native PNG filename')
    final=dict(max(images['commands'].values(),key=lambda v:(v['cycle'],v['command'])),command=None) if images['commands'] else None
    require(images['final']==final,'native final selection differs')


def write_pngs(data,folder):
    images=data.get('images',{})
    if not images.get('available'):return
    folder=Path(folder)/'images';folder.mkdir(parents=True,exist_ok=True)
    for image in images['commands'].values():(folder/image['file']).write_bytes(base64.b64decode(image['src'].split(',',1)[1],validate=True))
    portable=dict(images,commands={k:{f:v for f,v in image.items() if f!='src'} for k,image in images['commands'].items()},
                  final={k:v for k,v in images['final'].items() if k!='src'})
    (folder/'native-index.json').write_text(json.dumps(portable,indent=2)+'\n')
