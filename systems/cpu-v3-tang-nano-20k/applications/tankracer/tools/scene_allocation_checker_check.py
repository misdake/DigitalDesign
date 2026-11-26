"""Reject wrong transient IDs, property rows, free-pop prefixes, growth and RNG."""
import argparse
import copy
import json
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('checker', type=Path)
    parser.add_argument('fixture', type=Path)
    parser.add_argument('output', type=Path)
    group=parser.add_mutually_exclusive_group()
    group.add_argument('--creation', action='store_true')
    group.add_argument('--mines',action='store_true')
    group.add_argument('--mine-tick',action='store_true')
    group.add_argument('--explosions',action='store_true')
    group.add_argument('--explosion-tick',action='store_true')
    group.add_argument('--weapon-explosion',action='store_true')
    group.add_argument('--mine-explosion',action='store_true')
    group.add_argument('--mine-hit',action='store_true')
    group.add_argument('--projectile-flight',action='store_true')
    group.add_argument('--projectile-area',action='store_true')
    group.add_argument('--projectile-direct',action='store_true')
    args = parser.parse_args()
    fixture = args.fixture.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    mode = 'scene-creation' if args.creation else 'scene-allocation'
    if args.mines: mode='scene-mines'
    if args.mine_tick:mode='scene-mine-tick'
    if args.explosions:mode='scene-explosions'
    if args.explosion_tick:mode='scene-explosion-tick'
    if args.weapon_explosion:mode='scene-weapon-explosion'
    if args.mine_explosion:mode='scene-mine-explosion'
    if args.mine_hit:mode='scene-mine-hit'
    if args.projectile_flight:mode='scene-projectile-flight'
    if args.projectile_area:mode='scene-projectile-area'
    if args.projectile_direct:mode='scene-projectile-direct'
    data = json.loads((fixture / (mode + '.json')).read_text())
    faults = [('transient-1', 'object'), ('transient-0', 'properties'),
              ('transient-6', 'free-pop'), ('transient-0', 'growth'),
              ('transient-0', 'RNG')]
    if args.creation:
        faults = [('constructor-0', 'default-state'), ('constructor-0', 'sort-position'),
                  ('constructor-7', 'homing-reference'), ('constructor-7', 'shared-flight'),
                  ('constructor-9', 'launch-prefix'), ('constructor-12', 'active-prefix'),
                  ('constructor-0', 'retired-component'), ('constructor-7', 'RNG')]
    if args.mines:
        faults=[('mine-factory-0','height'),('mine-factory-1','aligned-basis'),
                ('mine-factory-0','bounds'),('mine-factory-0','grid'),
                ('mine-factory-0','limit-count'),('mine-factory-0','limit-order'),
                ('mine-factory-7','limit-prefix'),('mine-factory-6','miss-prefix'),
                ('mine-factory-9','birth-frame')]
    if args.mine_tick:
        faults=[('mine-tick-0','clock'),('mine-tick-0','live-owner'),
                ('mine-tick-13','birth-stamp'),('mine-tick-3','partial-counter'),
                ('mine-tick-8','nested-child'),('mine-tick-11','nested-slot'),
                ('mine-tick-10','repeat-order'),('mine-tick-0','RNG'),
                ('mine-tick-19','cursor-prefix')]
    if args.explosions:
        faults=[('explosion-factory-0','RNG'),('explosion-factory-0','replacement'),
                ('explosion-factory-5','fog'),('explosion-factory-0','evicted-count'),
                ('explosion-factory-8','shared-count'),('explosion-factory-8','quad-release'),
                ('explosion-factory-10','partial-release'),('explosion-factory-11','held-slot'),
                ('explosion-factory-4','null-initialize'),('explosion-factory-14','capacity-prefix')]
    if args.explosion_tick:
        faults=[('explosion-tick-3','owner-prefix'),('explosion-tick-2','attached-smoke'),
                ('explosion-tick-10','shared-RNG'),('explosion-tick-12','deferred-owner'),
                ('explosion-tick-13','late-owner-error'),('explosion-tick-14','natural-release'),
                ('explosion-tick-20','queued-before-release'),('explosion-tick-22','first-owner-clear'),
                ('explosion-tick-23','cursor'),('explosion-tick-18','held-slot'),
                ('explosion-tick-1','smoke-fade'),('explosion-tick-9','motion-prefix'),
                ('explosion-tick-25','dynamic-origin')]
    if args.weapon_explosion:
        faults=[('weapon-explosion-0','band-link'),('weapon-explosion-0','warp-visibility'),
                ('weapon-explosion-1','contact-fragments'),('weapon-explosion-2','spin-RNG'),
                ('weapon-explosion-3','body-prefix'),('weapon-explosion-4','missing-vehicle'),
                ('weapon-explosion-8','exhausted-slot'),('weapon-explosion-9','partial-quad'),
                ('weapon-explosion-10','overflow-alias'),('weapon-explosion-11','late-owner'),
                ('weapon-explosion-14','success-flash'),('weapon-explosion-15','failed-flash'),
                ('weapon-explosion-19','contact-tick'),('weapon-explosion-20','stage-priority'),
                ('weapon-explosion-22','zero-stamp')]
    if args.mine_explosion:
        faults=[('mine-explosion-0','owner'),('mine-explosion-0','velocity'),
                ('mine-explosion-0','electric-kind'),('mine-explosion-0','ring-link'),
                ('mine-explosion-0','wide-kind'),('mine-explosion-1','attached-smoke'),
                ('mine-explosion-2','pool-prefix'),('mine-explosion-4','exhausted-slot'),
                ('mine-explosion-5','partial-quad'),('mine-explosion-6','overflow-alias'),
                ('mine-explosion-7','late-owner'),('mine-explosion-10','flash'),
                ('mine-explosion-13','signed-velocity'),('mine-explosion-14','zero-stamp')]
    if args.mine_hit:
        faults=[('mine-hit-0','velocity'),('mine-hit-0','angular'),
                ('mine-hit-1','electric-timer'),('mine-hit-13','restore'),
                ('mine-hit-8','same-tail'),('mine-hit-12','ordered-peer'),
                ('mine-hit-3','tail-owner'),('mine-hit-0','camera'),
                ('mine-hit-4','protection'),('mine-hit-6','impulse-prefix'),
                ('mine-hit-14','arming'),('mine-hit-22','health'),
                ('mine-hit-24','held-slot'),('mine-hit-26','attached-tick'),
                ('mine-hit-28','unarmed-RNG'),('mine-hit-0','deactivated-count')]
    if args.projectile_flight:
        faults=[('projectile-flight-14','pose-prefix'),('projectile-flight-14','external-prefix'),
                ('projectile-flight-10','expired'),('projectile-flight-7','queued-retirement'),
                ('projectile-flight-6','trail-fade'),('projectile-flight-8','overlay'),
                ('projectile-flight-21','appended-pose'),('projectile-flight-21','launch-RNG'),
                ('projectile-flight-12','resources'),('projectile-flight-13','terrain'),
                ('projectile-flight-19','ground-mark'),('projectile-flight-20','car-hit'),
                ('projectile-flight-17','carrier'),('projectile-flight-22','released-identity'),
                ('projectile-flight-23','reflection-owner'),('projectile-flight-23','reflection-RNG'),
                ('projectile-flight-23','reflection-flag'),('projectile-flight-23','reflection-basis')]
    if args.projectile_area:
        faults=[('projectile-area-11','mark-prefix'),('projectile-area-15','explosion-prefix'),
                ('projectile-area-8','physical-anchor'),('projectile-area-7','outside-camera'),
                ('projectile-area-22','ordered-RNG'),('projectile-area-2','owner-filter'),
                ('projectile-area-4','protected'),('projectile-area-23','visibility'),
                ('projectile-area-0','flash-alpha'),('projectile-area-21','emitter'),
                ('projectile-area-20','child-tick'),('projectile-area-25','held-slot'),
                ('projectile-area-18','pool-exhaustion'),('projectile-area-20','retirement')]
    if args.projectile_direct:
        faults=[('projectile-direct-0','impulse'),('projectile-direct-0','dent'),
                ('projectile-direct-27','turret-revision'),('projectile-direct-18','smoke'),
                ('projectile-direct-2','temporary'),('projectile-direct-2','temporary-RNG'),
                ('projectile-direct-5','sidegrip'),('projectile-direct-10','refresh'),
                ('projectile-direct-11','failed-temporary'),('projectile-direct-9','basis-prefix'),
                ('projectile-direct-14','temporary-prefix'),('projectile-direct-15','pool-prefix'),
                ('projectile-direct-6','reflection'),('projectile-direct-7','spin-filter'),
                ('projectile-direct-17','inventory'),('projectile-direct-2','child-update'),
                ('projectile-direct-28','part-prefix'),('projectile-direct-29','ownership-prefix')]
    results = []
    for index, (tag, kind) in enumerate(faults):
        case = copy.deepcopy(next(c for c in data['cases'] if c['tag'] == tag))
        frame = 0
        expected = 'call 0 word 0:'
        if args.projectile_direct:
            frame=1 if kind=='child-update' else 0
            state=case['frames'][frame]['state']
            if kind in ['impulse','spin-filter']:
                word=20 if kind=='impulse' else 48
                state['area_cars'][word]^=1;expected=f'area cars word {word}:'
            elif kind in ['turret-revision','smoke','sidegrip','failed-temporary','basis-prefix','inventory','part-prefix','ownership-prefix']:
                word={'turret-revision':59,'smoke':41,'sidegrip':66,'failed-temporary':66,'basis-prefix':21,'inventory':21,'part-prefix':40,'ownership-prefix':34}[kind]
                state['damage_profiles'][word]^=1;expected=f'damage car 0 field {word} word 0:'
            elif kind=='dent':
                word=next(i for i,(a,b) in enumerate(zip(case['initial']['damage_geometry'],state['damage_geometry'])) if a!=b)
                state['damage_geometry'][word]^=1;expected=f'damage geometry word {word}:'
            elif kind in ['temporary','refresh']:
                word=next(i for i,(a,b) in enumerate(zip(case['initial']['temporary'],state['temporary'])) if a!=b)
                state['temporary'][word]^=1;expected=f'temporary word {word}:'
            elif kind=='temporary-RNG':
                state['random']^=1;expected='call 0 RNG word 0:'
            else:
                weapon=state['weapons'][-1] if kind in ['temporary-prefix','pool-prefix','child-update'] else state['weapons'][0]
                word=68 if kind=='child-update' else 8
                weapon['payload'][word]^=1;expected=f"weapon {weapon['id']} payload word {word}:"
        elif args.projectile_area:
            frame=1 if kind=='child-tick' else 6 if kind=='retirement' else 0
            state=case['frames'][frame]['state']
            if kind in ['physical-anchor','owner-filter','protected','visibility']:
                word={'physical-anchor':84,'owner-filter':20,'protected':104,'visibility':90}[kind]
                state['area_cars'][word]^=1;expected=f'area cars word {word}:'
            elif kind in ['explosion-prefix','flash-alpha','child-tick']:
                weapon=state['weapons'][-1] if kind=='child-tick' else state['weapons'][0]
                word={'explosion-prefix':8,'flash-alpha':26,'child-tick':68}[kind]
                weapon['payload'][word]^=1;expected=f"weapon {weapon['id']} payload word {word}:"
            elif kind=='mark-prefix':state['marks'][0]^=1;expected='marks word 0:'
            elif kind=='outside-camera':state['camera_impulse'][2]^=1;expected='camera impulse word 2:'
            elif kind=='ordered-RNG':state['random']^=1;expected=f'call {frame} RNG word 0:'
            elif kind=='emitter':
                weapon=state['weapons'][-1];weapon['meta'][8]^=1;expected=f"weapon {weapon['id']} scene data word 8:"
            elif kind=='pool-exhaustion':state['blast_effects'][5514]^=1;expected='effect pool word 5140:'
            elif kind=='held-slot':state['slot']^=1;expected=f'call {frame} dispatch slot word 0:'
            else:
                entity=case['frames'][frame]['entity'];state['flags'][entity]^=2;expected=f'call {frame} tick flags {entity} word 0:'
        elif args.projectile_flight:
            frame=1 if kind=='appended-pose' else 0;state=case['frames'][frame]['state']
            if kind in ['pose-prefix','external-prefix','appended-pose','carrier','reflection-basis']:
                id=case['frames'][frame]['object'];word=26 if kind in ['external-prefix','reflection-basis'] else 2
                state['colliders'][id][word]^=1;expected=f'collider {id} word {word}:'
            elif kind in ['expired','trail-fade','reflection-owner']:
                weapon=state['weapons'][0];word={'expired':8,'trail-fade':101,'reflection-owner':2}[kind]
                weapon['payload'][word]^=1;expected=f"weapon {weapon['id']} payload word {word}:"
            elif kind=='queued-retirement':
                entity=case['frames'][frame]['entity'];state['flags'][entity]^=2;expected=f'call {frame} tick flags {entity} word 0:'
            elif kind=='overlay':state['overlay'][3]^=1;expected='overlay word 3:'
            elif kind in ['launch-RNG','reflection-RNG']:state['random']^=1;expected=f'call {frame} RNG word 0:'
            elif kind=='reflection-flag':case['frames'][frame]['reflection']=False;expected='reflection word 0:'
            elif kind in ['resources','terrain']:case['frames'][frame]['status']=-4;expected=f'call {frame} word 0:'
            elif kind in ['ground-mark','car-hit']:
                word=8 if kind=='ground-mark' else 17;case['frames'][frame]['action'][word]^=1;expected=f'flight action word {word}:'
            else:state['slot']^=1;expected=f'call {frame} dispatch slot word 0:'
        elif args.mine_hit:
            frame=2 if kind=='arming' else 1 if kind=='attached-tick' else 0
            state=case['frames'][frame]['state']
            if kind in ['velocity','angular','electric-timer','restore','same-tail','ordered-peer','protection','impulse-prefix','health']:
                word={'velocity':12,'angular':18,'electric-timer':48,'restore':50,'same-tail':52,'ordered-peer':180,'protection':12,'impulse-prefix':12,'health':44}[kind]
                state['mine_cars'][word]^=1;expected=f'mine_cars word {word}:'
            elif kind=='tail-owner':state['shield_tails'][0]^=1;expected='shield_tails word 0:'
            elif kind=='camera':state['camera_impulse'][2]^=1;expected='camera_impulse word 2:'
            elif kind in ['arming','attached-tick']:
                weapon=state['weapons'][0] if kind=='arming' else state['weapons'][-1];word=10 if kind=='arming' else 68
                weapon['payload'][word]^=1;expected=f"weapon {weapon['id']} payload word {word}:"
            elif kind=='held-slot':state['slot']^=1;expected=f'call {frame} dispatch slot word 0:'
            elif kind=='deactivated-count':state['mine_count']^=1;expected='mine count word 0:'
            else:state['random']^=1;expected=f'call {frame} RNG word 0:'
        elif args.mine_explosion:
            frame={'electric-kind':1,'wide-kind':3,'attached-smoke':1,'zero-stamp':2}.get(kind,0)
            state=case['frames'][frame]['state'];weapon=state['weapons'][-1]
            if kind in ['owner','velocity','ring-link','attached-smoke','pool-prefix','late-owner','signed-velocity']:
                word={'owner':18,'velocity':20,'ring-link':14,'attached-smoke':68,'pool-prefix':20,'late-owner':14,'signed-velocity':20}[kind]
                weapon['payload'][word]^=1;expected=f"weapon {weapon['id']} payload word {word}:"
            elif kind in ['electric-kind','wide-kind','exhausted-slot','partial-quad','overflow-alias']:
                word={'electric-kind':98+138+14,'wide-kind':98+138+14,'exhausted-slot':16,'partial-quad':44,'overflow-alias':5518}[kind]
                state['blast_effects'][word]^=1
                expected=f'shared blasts word {word}:' if word<374 else f'effect pool word {word-374}:'
            elif kind=='flash':state['explosion_world'][6]^=1;expected='explosion slots word 6:'
            else:state['random']^=1;expected=f'call {frame} RNG word 0:'
        elif args.weapon_explosion:
            frame=2 if kind in ['warp-visibility','zero-stamp'] else (1 if kind in ['band-link','contact-fragments','spin-RNG','body-prefix','missing-vehicle','exhausted-slot','partial-quad','overflow-alias','contact-tick'] else 0)
            state=case['frames'][frame]['state'];weapon=state['weapons'][-1]
            if kind in ['band-link','warp-visibility','contact-fragments','body-prefix','missing-vehicle','late-owner','contact-tick']:
                word={'band-link':12,'warp-visibility':6,'contact-fragments':34,'body-prefix':26,'missing-vehicle':16,'late-owner':6,'contact-tick':68}[kind]
                weapon['payload'][word]^=1;expected=f"weapon {weapon['id']} payload word {word}:"
            elif kind in ['spin-RNG','zero-stamp']:state['random']^=1;expected=f'call {frame} RNG word 0:'
            elif kind in ['exhausted-slot','partial-quad','overflow-alias']:
                word={'exhausted-slot':16,'partial-quad':44,'overflow-alias':5518}[kind];state['blast_effects'][word]^=1
                expected=f'shared blasts word {word}:' if word<374 else f'effect pool word {word-374}:'
            elif kind in ['success-flash','failed-flash']:state['explosion_world'][6]^=1;expected='explosion slots word 6:'
            else:case['frames'][frame]['status']=-4;expected=f'call {frame} word 0:'
        elif args.explosion_tick:
            frame=1 if kind in ['shared-RNG','dynamic-origin'] else 0;state=case['frames'][frame]['state']
            if kind in ['owner-prefix','attached-smoke','deferred-owner','late-owner-error','first-owner-clear','smoke-fade','motion-prefix','dynamic-origin']:
                word={'owner-prefix':2,'attached-smoke':68,'deferred-owner':12,'late-owner-error':10,'first-owner-clear':12,'smoke-fade':98,'motion-prefix':46,'dynamic-origin':68}[kind]
                weapon=state['weapons'][0] if kind in ['deferred-owner','first-owner-clear'] else state['weapons'][-1]
                weapon['payload'][word]^=1;expected=f"weapon {weapon['id']} payload word {word}:"
            elif kind=='shared-RNG':state['random']^=1;expected=f'call {frame} RNG word 0:'
            elif kind=='natural-release':state['blast_effects'][16]^=1;expected='shared blasts word 16:'
            elif kind=='queued-before-release':
                entity=case['frames'][frame]['entity'];state['flags'][entity]^=2;expected=f'call {frame} tick flags {entity} word 0:'
            elif kind=='cursor':state['next']=-1;expected=f'call {frame} tick cursor word 0:'
            else:state['slot']^=1;expected=f'call {frame} dispatch slot word 0:'
        elif args.explosions:
            frame={'replacement':15,'fog':15,'evicted-count':33,'shared-count':1,'quad-release':1,'partial-release':1,'held-slot':1,'capacity-prefix':15}.get(kind,0)
            state=case['frames'][frame]['state']
            if kind=='RNG':state['random']^=1;expected=f'call {frame} RNG word 0:'
            elif kind in ['replacement','fog','evicted-count','capacity-prefix']:
                word={'replacement':12,'fog':4,'evicted-count':0,'capacity-prefix':2}[kind]
                state['explosion_world'][word]^=1;expected=f'explosion slots word {word}:'
            elif kind in ['shared-count','quad-release']:
                word=16 if kind=='shared-count' else 376;state['blast_effects'][word]^=1
                expected='shared blasts word 16:' if kind=='shared-count' else 'effect pool word 2:'
            elif kind=='partial-release':
                weapon=state['weapons'][0];weapon['payload'][12]^=1;expected=f"weapon {weapon['id']} payload word 12:"
            else:
                state['slot']^=1;expected=f'call {frame} dispatch slot word 0:'
        elif args.mine_tick:
            frame=0 if kind=='clock' else 1
            state=case['frames'][frame]['state'];parent=state['weapons'][0];child=state['weapons'][-1]
            if kind in ['clock','partial-counter']:
                word=16 if kind=='clock' else 6;parent['payload'][word]^=1
                expected=f"weapon {parent['id']} payload word {word}:"
            elif kind=='live-owner':
                state['colliders'][child['id']][2]^=1
                expected=f"collider {child['id']} word 2:"
            elif kind=='birth-stamp':
                child['payload'][12]^=1;expected=f"weapon {child['id']} payload word 12:"
            elif kind=='nested-child':
                state['weapons'].pop();expected=f"weapon {child['id']} presence"
            elif kind=='nested-slot':
                assert state['slot']==8;state['slot']=12
                expected=f'call {frame} dispatch slot word 0:'
            elif kind=='repeat-order':
                state['mine_order'][0],state['mine_order'][1]=state['mine_order'][1],state['mine_order'][0]
                expected='mine order word 0:'
            elif kind=='cursor-prefix':
                assert state['next']==case['initial']['next'] and state['next']!=-1;state['next']=-1
                expected=f'call {frame} tick cursor word 0:'
            else:
                state['random']^=1;expected=f'call {frame} RNG word 0:'
        elif args.mines:
            if kind in ['limit-count','limit-prefix']: frame=30
            if kind=='limit-order':frame=31
            state=case['frames'][frame]['state'];weapon=state['weapons'][-1];object_id=weapon['id']
            if kind in ['height','aligned-basis','bounds']:
                word={'height':4,'aligned-basis':8,'bounds':44}[kind]
                state['colliders'][object_id][word]^=1
                expected=f'collider {object_id} word {word}:'
            elif kind=='grid':
                cell_index=next(i for i,cell in enumerate(state['grid']) if object_id in cell)
                item=state['grid'][cell_index].index(object_id)
                state['grid'][cell_index][item]^=1
                expected=f'grid {cell_index} item {item} word 0:'
            elif kind in ['limit-count','limit-prefix']:
                state['mine_count']^=1
                expected='mine count word 0:'
            elif kind=='limit-order':
                state['mine_order'][0],state['mine_order'][1]=state['mine_order'][1],state['mine_order'][0]
                expected='mine order word 0:'
            elif kind=='miss-prefix':
                weapon['meta'][2]=0
                expected=f'weapon {object_id} scene data word 2:'
            else:
                weapon['payload'][12]^=1
                expected=f'weapon {object_id} payload word 12:'
        elif args.creation:
            if kind in ['homing-reference', 'shared-flight', 'RNG']:
                frame = 2
            elif kind == 'retired-component':
                frame = next(i for i, c in enumerate(case['controls']) if c[0] == 2 and c[2] == 3)
            state = case['frames'][frame]['state']
            weapon = state['weapons'][0] if state['weapons'] else None
            if kind == 'default-state':
                weapon['payload'][0] ^= 1
                expected = f"weapon {weapon['id']} payload word 0:"
            elif kind == 'sort-position':
                weapon['meta'][1] ^= 1
                expected = f"weapon {weapon['id']} scene data word 1:"
            elif kind == 'homing-reference':
                weapon = next(w for w in state['weapons'] if w['payload'][4] == 2)
                weapon['payload'][60] ^= 1
                expected = f"weapon {weapon['id']} payload word 60:"
            elif kind == 'shared-flight':
                state['flight'][2] ^= 1
                expected = 'shared flight word 2:'
            elif kind == 'launch-prefix':
                weapon['payload'][2] ^= 1
                expected = f"weapon {weapon['id']} payload word 2:"
            elif kind == 'active-prefix':
                entity = state['colliders'][weapon['id']][0]
                assert state['flags'][entity] != 0
                state['flags'][entity] = 0
                expected = f'call {frame} tick flags {entity} word 0:'
            elif kind == 'retired-component':
                old = case['frames'][0]['state']['weapons'][0]
                state['weapons'].append(copy.deepcopy(old))
                expected = f"weapon {old['id']} kind"
            else:
                state['random'] ^= 1
                expected = f'call {frame} RNG word 0:'
        elif kind == 'object':
            case['frames'][0]['object'] ^= 1
        elif kind == 'properties':
            object_id = case['frames'][0]['object']
            case['frames'][0]['state']['colliders'][object_id][60] ^= 1
            expected = f'properties {object_id} word 0:'
        elif kind == 'free-pop':
            case['frames'][0]['state']['lists'][5].append(case['initial']['lists'][5][-1])
            expected = 'call 0 list 5'
        elif kind == 'growth':
            capacity = case['initial']['identity'][0]
            frame = next(i for i, f in enumerate(case['frames'])
                         if f['state']['identity'][0] != capacity)
            case['frames'][frame]['state']['identity'][0] = capacity
            expected = f'call {frame} identity word 0:'
        else:
            case['frames'][0]['state']['random'] ^= 1
            expected = 'call 0 RNG word 0:'
        case['frames'] = case['frames'][:frame + 1]
        case['controls'] = case['controls'][:frame + 1]
        folder = args.output / str(index)
        folder.mkdir(exist_ok=False)
        changed = dict(data, cases=[case])
        (folder / (mode + '.json')).write_text(json.dumps(changed))
        shutil.copyfile(fixture / case['scene'], folder / case['scene'])
        if args.creation or args.mines or args.mine_tick or args.explosions or args.explosion_tick or args.weapon_explosion or args.mine_explosion or args.mine_hit or args.projectile_flight or args.projectile_area or args.projectile_direct:
            for name in ['sine.i16.bin', 'angle.i16.bin']:
                shutil.copyfile(fixture / name, folder / name)
        if args.mines or args.mine_tick or args.projectile_flight or args.projectile_area or args.projectile_direct:shutil.copyfile(fixture/'projectile-cells.bin',folder/'projectile-cells.bin')
        result = subprocess.run([str(args.checker.resolve()), str(folder / 'check'),
                                 str(folder), '0', mode],
                                capture_output=True, text=True, timeout=120)
        message = result.stdout + result.stderr
        if result.returncode == 0 or expected not in message:
            raise RuntimeError(f'{kind} was not rejected at its expected boundary: {message[-2000:]}')
        results.append(dict(kind=kind, tag=tag, frame=frame, rejected=True))
    (args.output / 'report.json').write_text(json.dumps(dict(passed=True, faults=results), indent=2) + '\n')


if __name__ == '__main__':
    main()
