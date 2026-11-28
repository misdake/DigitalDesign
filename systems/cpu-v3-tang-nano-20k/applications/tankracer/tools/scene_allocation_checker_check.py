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
    parser.add_argument('--packed', action='store_true')
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
    group.add_argument('--ufos',action='store_true')
    group.add_argument('--projectile-ufo',action='store_true')
    group.add_argument('--vehicle-methods',action='store_true')
    group.add_argument('--obstacle-methods',action='store_true')
    group.add_argument('--ufo-transfer',action='store_true')
    group.add_argument('--fire',action='store_true')
    group.add_argument('--laps',action='store_true')
    group.add_argument('--trains',action='store_true')
    group.add_argument('--emitters',action='store_true')
    group.add_argument('--flocks',action='store_true')
    group.add_argument('--ambient',action='store_true')
    group.add_argument('--player',action='store_true')
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
    if args.ufos:mode='scene-ufos'
    if args.projectile_ufo:mode='scene-projectile-ufo'
    if args.vehicle_methods:mode='scene-vehicle-transfer'
    if args.obstacle_methods:mode='scene-obstacle-transfer'
    if args.ufo_transfer:mode='scene-ufo-transfer'
    if args.fire:mode='scene-fire'
    if args.laps:mode='scene-laps'
    if args.trains:mode='scene-train'
    if args.emitters:mode='scene-emitter'
    if args.flocks:mode='scene-flock'
    if args.ambient:mode='scene-ambient'
    if args.player:mode='scene-player'
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
    if args.ufos:
        faults=[('ufo-factory-0','point'),('ufo-factory-0','target'),
                ('ufo-factory-3','special'),('ufo-factory-4','collidable'),
                ('ufo-factory-6','view'),('ufo-factory-7','emitter'),
                ('ufo-factory-8','wrapped-height'),('ufo-factory-20','default-sprite'),
                ('ufo-factory-16','interface'),('ufo-factory-19','reuse'),
                ('ufo-factory-10','resource-prefix'),('ufo-factory-0','RNG')]
    if args.projectile_ufo:
        faults=[('projectile-ufo-0','attachment'),('projectile-ufo-0','ufo-target'),
                ('projectile-ufo-0','follow'),('projectile-ufo-0','rotated-basis'),
                ('projectile-ufo-11','released'),('projectile-ufo-11','retirement'),
                ('projectile-ufo-5','lifecycle'),('projectile-ufo-6','pool-free'),
                ('projectile-ufo-2','protected'),('projectile-ufo-20','existing-link'),
                ('projectile-ufo-24','terrain-priority'),('projectile-ufo-25','resource-priority'),
                ('projectile-ufo-27','partial-allocation'),('projectile-ufo-30','view'),
                ('projectile-ufo-31','emitter'),('projectile-ufo-33','slot-restore')]
    if args.vehicle_methods:
        faults=[('vehicle-methods-1','spin'),('vehicle-methods-8','collision'),
                ('vehicle-methods-9','driving'),('vehicle-methods-0','checkpoint'),
                ('vehicle-methods-0','velocity'),('vehicle-methods-24','pose-prefix'),
                ('vehicle-methods-17','failed-result'),('vehicle-methods-55','nested-slot'),
                ('vehicle-methods-35','outer-slot'),('vehicle-methods-39','cursor'),
                ('vehicle-methods-10','notification'),('vehicle-methods-53','activation'),
                ('vehicle-methods-34','nested-reset'),('vehicle-methods-56','nested-move'),
                ('vehicle-methods-0','RNG')]
    if args.obstacle_methods:
        faults=[('obstacle-methods-0','mode-order'),('obstacle-methods-2','duplicate-grid'),
                ('obstacle-methods-0','move-cell'),('obstacle-methods-10','height'),
                ('obstacle-methods-18','position-prefix'),('obstacle-methods-21','failed-position'),
                ('obstacle-methods-39','partial-grid'),('obstacle-methods-40','partial-move'),
                ('obstacle-methods-19','failed-result'),('obstacle-methods-26','maximum-slot'),
                ('obstacle-methods-13','default-radius'),('obstacle-methods-28','class-result'),
                ('obstacle-methods-36','banked-position'),('obstacle-methods-0','RNG')]
    if args.ufo_transfer:
        faults=[('ufo-transfer-4','approach'),('ufo-transfer-4','disabled'),
                ('ufo-transfer-28','coll-prefix'),('ufo-transfer-5','checkpoint'),
                ('ufo-transfer-5','reservation'),('ufo-transfer-5','visibility'),
                ('ufo-transfer-7','fade-start'),('ufo-transfer-14','fade-delay'),
                ('ufo-transfer-14','fade-release'),('ufo-transfer-35','overlap'),
                ('ufo-transfer-39','missing-body-prefix'),('ufo-transfer-18','occupied-error'),
                ('ufo-transfer-19','parent-error'),('ufo-transfer-16','occupied-release'),
                ('ufo-transfer-0','departure'),('ufo-transfer-43','travel'),
                ('ufo-transfer-44','negative-travel'),('ufo-transfer-45','arrival'),
                ('ufo-transfer-46','wrapped-math'),('ufo-transfer-47','packed-dt'),
                ('ufo-transfer-12','external-basis'),('ufo-transfer-12','sprite'),
                ('ufo-transfer-34','hidden-prefix'),('ufo-transfer-33','pose-prefix'),
                ('ufo-transfer-58','unsigned-car-radius'),('ufo-transfer-59','unsigned-obstacle-radius'),
                ('ufo-transfer-60','detached-target'),('ufo-transfer-61','detached-reset'),
                ('ufo-transfer-0','RNG')]
    if args.fire:
        faults=[('fire-3','cooldown-gate'),('fire-1','recoil'),('fire-5','mine-ammo'),
                ('fire-50','boost-control'),('fire-50','boost-feedback'),('fire-51','spin-count'),
                ('fire-54','spin-hold'),('fire-54','spin-RNG'),('fire-47','selection-prefix'),
                ('fire-47','private-output'),('fire-63','factory-prefix'),('fire-65','racer-order'),
                ('fire-71','collision-wrap'),('fire-72','recoil-wrap'),('fire-75','second-gate')]
    if args.laps:
        faults=[('laps-8','entered'),('laps-16','delayed'),('laps-32','flush-cursor'),
                ('laps-34','shared-limit'),('laps-39','failed-slot'),('laps-40','fall-basis'),
                ('laps-40','fall-alias'),('laps-41','external-basis'),('laps-46','restore-prefix'),
                ('laps-61','attached-stop'),('laps-63','missing-records'),('laps-64','enter-prefix'),
                ('laps-69','reused-identity'),('laps-68','wrapped-near'),('laps-0','RNG')]
    if args.trains:
        faults=[('train-0','activation'),('train-2','stagger'),('train-7','wait'),
                ('train-16','reset'),('train-17','reverse-velocity'),('train-17','reverse-basis'),
                ('train-28','failure-motion'),('train-28','failure-pose'),('train-28','failure-slot'),
                ('train-44','shared-RNG'),('train-45','peer-velocity'),('train-46','recovery')]
    if args.emitters:
        faults=[('emitter-0','timer'),('emitter-21','camera'),('emitter-19','equality'),
                ('emitter-15','gun-basis'),('emitter-16','debris-kind'),('emitter-37','alpha'),
                ('emitter-42','material'),('emitter-43','glow'),('emitter-26','resource-prefix'),
                ('emitter-30','activation-prefix'),('emitter-32','cross-bank'),('emitter-33','shared-RNG'),
                ('emitter-33','owner-link'),('emitter-29','constructor-prefix'),('emitter-31','failed-slot')]
    if args.flocks:
        faults=[('flock-0','takeoff'),('flock-3','stagger'),('flock-12','timer-equality'),
                ('flock-15','stage-target'),('flock-10','bird-motion'),('flock-25','wing-wrap'),
                ('flock-26','wing-alias'),('flock-27','missing-geometry'),('flock-28','partial-mesh'),
                ('flock-30','mesh-alias'),('flock-31','revision-wrap'),('flock-18','stop-membership'),
                ('flock-37','failed-stop-slot'),('flock-43','angle-prefix'),('flock-47','shared-RNG')]
    if args.ambient:
        faults=[('ambient-0','sequence'),('ambient-11','last-expired'),('ambient-12','life-equality'),
                ('ambient-13','last-free'),('ambient-14','billow'),('ambient-20','wrapped-camera'),
                ('ambient-22','negative-age'),('ambient-37','rotation-wrap'),('ambient-26','distance-freeze'),
                ('ambient-38','sprite-material'),('ambient-50','independent-noisy'),('ambient-44','invalid-selected'),
                ('ambient-51','region-leave'),('ambient-60','sequence-sharing'),('ambient-0','gameplay-RNG')]
    if args.player:
        faults=[('player-0','input'),('player-2','countdown'),('player-6','late-ammo'),('player-6','late-projectile'),
                ('player-10','failed-route-input'),('player-14','lap-event'),('player-21','shared-RNG'),
                ('player-22','dynamic-shape'),('player-29','lap-error-prefix'),('player-30','weather-error'),
                ('player-34','turret'),('player-38','fast-brake'),('player-33','inactive-race'),
                ('player-47','named-gate'),('player-1','without-keys'),('player-36','held-fire')]
    results = []
    for index, (tag, kind) in enumerate(faults):
        case = copy.deepcopy(next(c for c in data['cases'] if c['tag'] == tag))
        frame = 0
        expected = 'call 0 word 0:'
        if args.player:
            frame=2 if kind=='without-keys' else 1 if kind=='inactive-race' else 0;state=case['frames'][frame]['state']
            if kind=='shared-RNG':state['random']^=1;expected=f'call {frame} RNG word 0:'
            elif kind in ['weather-error','named-gate']:case['frames'][frame]['status']^=1
            elif kind in ['late-ammo','inactive-race','held-fire']:
                word={'late-ammo':2,'inactive-race':5,'held-fire':3}[kind];state['fire_state'][word]^=1;expected=f'fire state word {word}:'
            elif kind=='late-projectile':
                weapon=state['weapons'][0];weapon['payload'][0]^=1;expected=f"weapon {weapon['id']} payload word 0:"
            elif kind=='dynamic-shape':state['lap_bindings'][1443]^=1;expected='dynamic lap binding 240 word 3:'
            elif kind=='lap-event':state['lap_bindings'][75*6+2]^=1;expected=f'lap_bindings word {75*6+2}:'
            else:
                word={'input':0,'countdown':18,'failed-route-input':0,'lap-error-prefix':74,'turret':22,'fast-brake':10,'without-keys':86}[kind];state['player_fields'][word]^=1;expected=f'player fields word {word}:'
        elif args.ambient:
            frame=2 if kind=='sequence-sharing' else 1 if kind=='distance-freeze' else 0;state=case['frames'][frame]['state']
            if kind in ['sequence','sequence-sharing']:state['smoke_sequence']^=1;expected=f'call {frame} smoke sequence word 0:'
            elif kind=='gameplay-RNG':state['random']^=1;expected='call 0 RNG word 0:'
            elif kind in ['independent-noisy','region-leave']:state['noisy_selected'][0]^=1;expected='noisy selection word 0:'
            elif kind=='invalid-selected':case['frames'][frame]['status']^=1
            elif kind=='sprite-material':state['smoke_sprites'][0]^=1;expected='smoke sprites word 0:'
            else:
                word={'last-expired':420,'life-equality':34,'last-free':212,'billow':404,'wrapped-camera':6,'negative-age':34,'rotation-wrap':22,'distance-freeze':414}[kind]
                state['smoker_state'][word]^=1;expected=f'smoker state word {word}:'
        elif args.flocks:
            frame=2 if kind=='shared-RNG' else 0;state=case['frames'][frame]['state'];object=case['frames'][frame]['object'];actor=case['flock_objects'].index(object);base=actor*288
            if kind=='shared-RNG':state['random']^=1;expected=f'call {frame} RNG word 0:'
            elif kind in ['takeoff','stagger','timer-equality','stage-target','bird-motion','angle-prefix']:
                word=base+({'takeoff':0,'stagger':0,'timer-equality':4,'stage-target':6,'bird-motion':64,'angle-prefix':64}[kind]);state['flock_state'][word]^=1;expected=f'flock state word {word}:'
            elif kind=='failed-stop-slot':state['slot']^=1;expected='dispatch slot word 0:'
            elif kind=='stop-membership':state['lists'][4].append(object);expected='list 4'
            else:
                model_index=actor*2;model=state['flock_models'][model_index];word=model['low']*3+1
                if kind in ['wing-wrap','wing-alias']:
                    model['vertices'][word]^=1;expected=f'flock model {model_index} word {word}:'
                else:
                    mesh_index=next(i for i,m in enumerate(state['flock_meshes'])if m['id']==model['mesh']);mesh=state['flock_meshes'][mesh_index]
                    if kind=='partial-mesh':mesh['vertices'][word]^=1;expected=f'flock mesh {mesh_index} word {word}:'
                    else:
                        part=1 if kind=='revision-wrap' else 0;mesh['version'][part]^=1;expected=f'flock mesh {mesh_index} metadata word {2+part}:'
        elif args.emitters:
            frame=25 if kind=='cross-bank' else 4 if kind in ['shared-RNG','owner-link'] else 0
            state=case['frames'][frame]['state']
            if kind in ['timer','camera','equality','resource-prefix']:
                word=case['emitter_objects'].index(case['frames'][frame]['object'])*50;state['emitter_state'][word]^=1;expected=f'emitter state word {word}:'
            elif kind=='shared-RNG':state['random']^=1;expected=f'call {frame} RNG word 0:'
            elif kind=='failed-slot':state['slot']^=1;expected='dispatch slot word 0:'
            else:
                weapon=state['weapons'][-1];id=weapon['id']
                if kind=='gun-basis':state['colliders'][id][8]^=1;expected=f'collider {id} word 8:'
                elif kind in ['activation-prefix','owner-link','constructor-prefix']:
                    weapon['meta'][8]^=1;expected=f'weapon {id} scene data word 8:'
                else:
                    word={'debris-kind':4,'alpha':86,'material':78,'glow':54,'cross-bank':10}[kind]
                    weapon['payload'][word]^=1;expected=f'weapon {id} payload word {word}:'
        elif args.trains:
            frame=1 if kind in ['shared-RNG','peer-velocity','recovery'] else 0
            state=case['frames'][frame]['state'];id=case['train_objects'][0]
            if kind=='shared-RNG':state['random']^=1;expected=f'call {frame} RNG word 0:'
            elif kind=='failure-slot':state['slot']^=1;expected='dispatch slot word 0:'
            elif kind in ['reset','reverse-basis','failure-pose']:
                word=2 if kind=='reset' else 8;state['colliders'][id][word]^=1;expected=f'collider {id} word {word}:'
            else:
                word=6 if kind=='wait' else 54 if kind in ['reverse-velocity','failure-motion'] else 114 if kind in ['peer-velocity','recovery'] else 0
                state['train_state'][word]^=1;expected=f'train state word {word}:'
        elif args.laps:
            state=case['frames'][0]['state']
            if kind in ['shared-limit','missing-records']:
                case['frames'][0]['status']^=1
            elif kind in ['entered','reused-identity']:
                word=(75 if kind=='entered' else 76)*6+2;state['lap_bindings'][word]^=1;expected=f'lap_bindings word {word}:'
            elif kind in ['delayed','flush-cursor','wrapped-near']:
                list_index=9 if kind=='flush-cursor' else 8;state['lists'][list_index][0]^=1;expected=f'list {list_index} word 0:'
            elif kind=='failed-slot':state['slot']^=1;expected='dispatch slot word 0:'
            elif kind in ['fall-basis','fall-alias','external-basis','restore-prefix']:
                id=state['lap_backups'][0];word=26 if kind in ['fall-alias','external-basis'] else 68 if kind=='restore-prefix' else 8
                state['colliders'][id][word]^=1;expected=f'properties {id} word 8:' if kind=='restore-prefix' else f'collider {id} word {word}:'
            elif kind=='attached-stop':state['lists'][1][0]^=1;expected='list 1 word 0:'
            elif kind=='enter-prefix':state['lap_deformables'][-2]^=1;expected=f"lap_deformables word {len(state['lap_deformables'])-2}:"
            else:state['random']^=1;expected='call 0 RNG word 0:'
        elif args.fire:
            frame=1 if kind=='second-gate' else 0;state=case['frames'][frame]['state']
            if kind=='spin-RNG':state['random']^=1;expected='RNG word 0:'
            elif kind=='private-output':state['fire_shot'][0]^=1;expected='fire shot word 0:'
            elif kind in ['factory-prefix','racer-order']:
                parent=state['weapons'][-1];word=30 if kind=='racer-order' else 8
                parent['payload'][word]^=1;expected=f"weapon {parent['id']} payload word {word}:"
            else:
                word={'cooldown-gate':5,'recoil':9,'mine-ammo':2,'boost-control':19,'boost-feedback':57,'spin-count':63,'spin-hold':31,'selection-prefix':3,'collision-wrap':7,'recoil-wrap':11,'second-gate':9}[kind]
                state['fire_state'][word]^=1;expected=f'fire state word {word}:'
        elif args.ufo_transfer:
            frame=len(case['frames'])-1 if kind=='departure' else 1 if kind in ['fade-start','fade-release'] else 0
            state=case['frames'][frame]['state'];ufo=next(w for w in state['weapons'] if w['kind']==4)
            if kind in ['disabled','coll-prefix','checkpoint','pose-prefix']:
                word={'disabled':244,'coll-prefix':244,'checkpoint':210,'pose-prefix':52}[kind]
                state['transfer_cars'][word]^=1;expected=f'transfer car 0 word {word}:'
            elif kind in ['reservation','occupied-error']:
                state['transfer_occupied'][0]^=1;expected='transfer_occupied word 0:'
            elif kind in ['visibility','occupied-release']:
                name='transfer_visible' if kind=='visibility' else 'transfer_occupied'
                state[name].extend([0,0]);expected=f'{name} count'
            elif kind=='fade-start':state['overlay'][1]^=1;expected='overlay word 1:'
            elif kind in ['parent-error','detached-target','detached-reset']:state['slot']^=1;expected='dispatch slot word 0:'
            elif kind=='departure':
                entity=case['frames'][frame]['entity'];state['flags'][entity]^=2;expected=f'tick flags {entity} word 0:'
            elif kind=='external-basis':
                id=ufo['id'];state['colliders'][id][26]^=1;expected=f'collider {id} word 26:'
            elif kind=='RNG':state['random']^=1;expected='RNG word 0:'
            else:
                word={'approach':12,'fade-delay':2,'fade-release':2,'overlap':2,'missing-body-prefix':20,'travel':10,'negative-travel':14,'arrival':18,'wrapped-math':10,'packed-dt':12,'sprite':36,'hidden-prefix':2,'unsigned-car-radius':2,'unsigned-obstacle-radius':2}[kind]
                ufo['payload'][word]^=1;expected=f"weapon {ufo['id']} payload word {word}:"
        elif args.obstacle_methods:
            frame=2 if kind=='move-cell' else 1 if kind=='duplicate-grid' else 2 if kind=='banked-position' else 0
            state=case['frames'][frame]['state'];id=case['frames'][frame]['object']
            if kind=='mode-order':state['lists'][4][-1]^=1;expected='list 4 word 0:'
            elif kind in ['position-prefix','failed-position','banked-position']:
                state['colliders'][id][2]^=1;expected=f'collider {id} word 2:'
            elif kind=='move-cell':state['records'][id][4]^=1;expected=f'metadata {id} word 4:'
            elif kind in ['height','failed-result','class-result']:state['transfer_result'][0]^=1;expected='transfer_result word 0:'
            elif kind=='maximum-slot':state['slot']^=1;expected='dispatch slot word 0:'
            elif kind=='partial-move':state['colliders'][id][2]^=1;expected=f'collider {id} word 2:'
            elif kind=='partial-grid':
                cell=next(i for i,(a,b) in enumerate(zip(case['initial']['grid'],state['grid'])) if len(a)!=len(b))
                state['grid'][cell].append(id);expected=f'grid {cell} count'
            elif kind in ['duplicate-grid','default-radius']:
                cell=next(i for i,v in enumerate(state['grid']) if id in v);word=state['grid'][cell].index(id)
                state['grid'][cell][word]^=1;expected=f'grid {cell} item {word} word 0:'
            else:state['random']^=1;expected='call 0 RNG word 0:'
        elif args.vehicle_methods:
            frame=6 if kind in ['checkpoint','velocity'] else 1 if kind=='notification' else 0
            state=case['frames'][frame]['state']
            if kind in ['spin','collision','driving','checkpoint','velocity','pose-prefix','nested-reset','nested-move']:
                word={'spin':244,'collision':246,'driving':248,'checkpoint':210,'velocity':222,'pose-prefix':52,'nested-reset':248,'nested-move':0}[kind]
                state['transfer_car'][word]^=1;expected=f'transfer_car word {word}:'
            elif kind=='failed-result':state['transfer_result'][0]^=1;expected='transfer_result word 0:'
            elif kind in ['nested-slot','outer-slot']:state['slot']^=1;expected=f'call {frame} dispatch slot word 0:'
            elif kind=='cursor':state['next']^=1;expected=f'call {frame} tick cursor word 0:'
            elif kind=='notification':state['lists'][1][0]^=1;expected='list 1 word 0:'
            elif kind=='activation':
                entity=case['frames'][frame]['entity'];state['flags'][entity]^=2;expected=f'call {frame} tick flags {entity} word 0:'
            else:state['random']^=1;expected=f'call {frame} RNG word 0:'
        elif args.projectile_ufo:
            frame=2 if kind in ['follow','rotated-basis','retirement'] else 0
            state=case['frames'][frame]['state']
            parent=next(w for w in state['weapons'] if w['kind']==1)
            if kind in ['attachment','released','lifecycle','pool-free','existing-link']:
                word=54 if kind=='attachment' else 52 if kind=='existing-link' else 8
                parent['payload'][word]^=1;expected=f"weapon {parent['id']} payload word {word}:"
            elif kind in ['ufo-target','view']:
                weapon=next(w for w in state['weapons'] if w['kind']==4);word=24 if kind=='view' else 0
                weapon['payload'][word]^=1;expected=f"weapon {weapon['id']} payload word {word}:"
            elif kind=='emitter':
                weapon=next(w for w in state['weapons'] if w['kind']==4);weapon['meta'][7]^=1;expected=f"weapon {weapon['id']} scene data word 7:"
            elif kind in ['follow','terrain-priority','resource-priority']:
                id=parent['id'];state['colliders'][id][2]^=1;expected=f'collider {id} word 2:'
            elif kind=='rotated-basis':state['ufo_car_basis'][0]^=1;expected='UFO car 0 basis word 0:'
            elif kind=='protected':
                weapon=next(w for w in state['weapons'] if w['kind']==3);weapon['payload'][8]^=1;expected=f"weapon {weapon['id']} payload word 8:"
            elif kind=='retirement':
                entity=case['frames'][frame]['entity'];state['flags'][entity]^=2;expected=f'call {frame} tick flags {entity} word 0:'
            else:state['slot']^=1;expected=f'call {frame} dispatch slot word 0:'
        elif args.ufos:
            frame=2 if kind=='reuse' else 0
            state=case['frames'][frame]['state']
            if kind in ['point','target','special','collidable','view','wrapped-height','default-sprite']:
                word={'point':10,'target':0,'special':30,'collidable':32,'view':24,'wrapped-height':12,'default-sprite':34}[kind]
                weapon=state['weapons'][-1];weapon['payload'][word]^=1
                expected=f"weapon {weapon['id']} payload word {word}:"
            elif kind=='emitter':
                weapon=state['weapons'][-1];weapon['meta'][8]^=1;expected=f"weapon {weapon['id']} scene data word 8:"
            elif kind=='interface':state['slot']^=1;expected='dispatch slot word 0:'
            elif kind=='resource-prefix':case['frames'][frame]['status']=-4
            elif kind=='reuse':
                weapon=state['weapons'][-1];weapon['meta'][3]^=1;expected=f"weapon {weapon['id']} scene data word 3:"
            else:state['random']^=1;expected='call 0 RNG word 0:'
        elif args.projectile_direct:
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
        if args.creation or args.mines or args.mine_tick or args.explosions or args.explosion_tick or args.weapon_explosion or args.mine_explosion or args.mine_hit or args.projectile_flight or args.projectile_area or args.projectile_direct or args.ufos or args.projectile_ufo or args.vehicle_methods or args.obstacle_methods or args.ufo_transfer or args.fire or args.laps or args.trains or args.emitters or args.flocks or args.ambient or args.player:
            for name in ['sine.i16.bin', 'angle.i16.bin']:
                shutil.copyfile(fixture / name, folder / name)
        if args.mines or args.mine_tick or args.projectile_flight or args.projectile_area or args.projectile_direct or args.projectile_ufo or args.fire or args.player:shutil.copyfile(fixture/'projectile-cells.bin',folder/'projectile-cells.bin')
        result = subprocess.run([str(args.checker.resolve()), str(folder / 'check'),
                                 str(folder), '0', mode + ('-packed' if args.packed else '')],
                                capture_output=True, text=True, timeout=120)
        message = result.stdout + result.stderr
        if result.returncode == 0 or expected not in message:
            raise RuntimeError(f'{kind} was not rejected at its expected boundary: {message[-2000:]}')
        results.append(dict(kind=kind, tag=tag, frame=frame, rejected=True))
    (args.output / 'report.json').write_text(json.dumps(dict(passed=True, packed=args.packed, faults=results), indent=2) + '\n')


if __name__ == '__main__':
    main()
