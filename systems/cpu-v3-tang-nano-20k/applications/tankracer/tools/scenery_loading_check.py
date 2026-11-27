"""Check canonical identity ownership and compact scenery loading on a full driving artifact."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools'))
from application_artifact import Artifact


def resource_offset(package, payload):
    cursor = 4
    matches = []
    while cursor + 8 <= len(package):
        _, control = struct.unpack_from('<II', package, cursor)
        cursor += 8
        length = control & 0x7fffffff
        if length == 0:
            break
        if control >> 31 == 0:
            data = package[cursor:cursor + length]
            if data.startswith(payload) and not any(data[len(payload):]):
                matches.append(cursor)
            cursor += length
    if len(matches) != 1:
        raise ValueError('one complete COPY resource required')
    return matches[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifact', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--player', action='store_true')
    parser.add_argument('--clock', action='store_true')
    parser.add_argument('--tail', action='store_true')
    parser.add_argument('--impact', action='store_true')
    parser.add_argument('--contact', action='store_true')
    parser.add_argument('--mines', action='store_true')
    parser.add_argument('--ufos', action='store_true')
    parser.add_argument('--object-ufos', action='store_true')
    parser.add_argument('--emitters', action='store_true')
    parser.add_argument('--trains', action='store_true')
    parser.add_argument('--ambient', action='store_true')
    parser.add_argument('--flocks', action='store_true')
    parser.add_argument('--world', action='store_true')
    args = parser.parse_args()
    artifact = Artifact(args.artifact)
    assets = args.artifact / 'tankracer-driving-assets'
    manifest = json.loads((assets / 'assets.json').read_text())
    if manifest['qualification_scope'] not in ['complete', 'full', 'player', 'clock', 'tail', 'impact', 'contact', 'mines', 'ufos', 'object-ufos', 'emitters', 'trains', 'ambient', 'flocks', 'world']:
        raise ValueError('complete or full driving artifact required')
    image = (assets / 'scenery.bin').read_bytes()
    header = struct.unpack('<48H', image[:96])
    identities = (assets / 'identity.bin').read_bytes()
    lifetime_words = (assets / 'lifetime.bin').stat().st_size // 2
    identity_words = struct.unpack('<' + 'H' * (len(identities) // 2), identities)
    obstacle = 75
    entity = next(i for i in range((len(identity_words) - 12) // 6)
                  if identity_words[12 + i * 6] & 31 == 7
                  and identity_words[14 + i * 6:16 + i * 6] == (obstacle, 0))
    tests = [
        ('class-snapshot', 'scenery.bin', header[12], 1, True, None),
        ('object-snapshot', 'scenery.bin', header[14], 1, True, None),
        ('component-presence', 'scenery.bin', header[4] + obstacle * 8 + 7, 1, False, None),
        ('canonical-class', 'identity.bin', 13 + entity * 6, 1, False, None),
        ('component-high-word', 'identity.bin', 15 + entity * 6, 1, False, 0x7f5b),
        ('component-range', 'identity.bin', 14 + entity * 6, 512, False, 0x7f5b),
        ('free-link-high-word', 'identity.bin', 17 + entity * 6, 1, False, 0x7f5b),
        ('free-link-range', 'identity.bin', 16 + entity * 6, 1024, False, 0x7f5b),
        ('tick-flags-high-word', 'scenery.bin', header[10] + 1, 1, False, 0x7f11),
        ('tick-flags-section-count', 'scenery.bin', 11, 1, False, 0x7f11),
        ('section-offset', 'scenery.bin', 4, 1, False, 0x7f11),
        ('flag-count', 'scenery.bin', 3, 512, False, 0x7f11),
        ('section-length', 'scenery.bin', 5, 32768, False, 0x7f11),
        ('lifetime-range', 'lifetime.bin', 2, 240, False, 0x7f22),
        ('lap-membership-capacity', 'lifetime.bin', 3, 128, False, 0x7f22),
        ('truncated-final-list', 'lifetime.bin', lifetime_words - 1, 1, False, 0x7f22),
    ]
    if args.world:
        if manifest['qualification_scope'] != 'world':
            raise ValueError('world perturbations require the authored-world artifact')
        live_file = 'scenery-initial-' + manifest['cases'][0]['name'] + '.bin'
        live_header = struct.unpack('<48H', (assets / live_file).read_bytes()[:96])
        payload = (assets / 'player-0.bin').read_bytes()
        first = struct.unpack_from('<H', payload)[0]
        peer = next(i for i in range((len(identity_words) - 12) // 6)
                    if identity_words[12 + i * 6] & 31 == 11
                    and identity_words[14 + i * 6:16 + i * 6] == (1, 0))
        pickup = next(i for i in range((len(identity_words) - 12) // 6)
                      if identity_words[12 + i * 6] & 31 == 7
                      and identity_words[14 + i * 6:16 + i * 6] == (186, 0))
        tests = [
            ('raw-dt-input', 'frames-0.bin', 0, 1, False, None),
            ('peer-tick-disabled', live_file, live_header[10] + peer * 2, 16, False, None),
            ('pickup-tick-disabled', live_file, live_header[10] + pickup * 2, 16, False, None),
            ('peer-racer-answer', 'player-0.bin', first + 34, 1, False, None),
            ('pickup-flight-answer', 'world-flights-0.bin', 54, 1, False, None),
            ('pickup-flight-initial', 'player-pickup-flights.bin', 54, 1, False, None),
            ('flock-model-count', 'flock-initial.bin', 17, 1, False, 0x7f80),
            ('flock-image-size', 'flock-initial.bin', 7, 1, False, 0x7f7f),
            ('clock-answer', 'clock-0.bin', 6, 1, False, None),
        ]
        if 'pickup' in manifest['cases'][0]['name']:
            words = struct.unpack('<' + 'H' * (len(payload) // 2), payload)
            packets = []
            cursor = 0
            while cursor < len(words):
                size = words[cursor]
                if size < first or size > len(words) - cursor:
                    raise ValueError('invalid collection answer packet')
                packets.append(words[cursor:cursor + size])
                cursor += size
            if packets[0][1] != 0 or not any(p[62] == 1 and p[64] > 0 for p in packets[1:]):
                raise ValueError('collection must publish a newly collected mine inventory')
            if not any(len(p) > first + 1 and p[first + 1] == 2
                       and p[62:66] == (0, 0, 0, 0) for p in packets[1:]):
                raise ValueError('collection must spend that inventory on a real mine')
            tests += [
                ('view-position-input', 'world-view-0.bin', 0, 256, False, None),
                ('view-basis-input', 'world-view-0.bin', 6, 4096, False, None),
                ('pickup-model-source', 'player-pickup-bindings.bin', 2, 1, False, None),
                ('collection-frame-answer', 'complete-0.bin', 14, 1, False, None),
                ('collected-inventory-answer', 'player-0.bin', first + 62, 1, False, None),
            ]
    if args.player:
        if manifest['qualification_scope'] != 'player':
            raise ValueError('player perturbations require the player-only artifact')
        context_words = struct.unpack('<56H', (assets / 'player-context.bin').read_bytes()[:112])
        player_bytes = (assets / 'player-0.bin').read_bytes()
        initial_words = struct.unpack_from('<H', player_bytes)[0]
        second = initial_words + struct.unpack_from('<H', player_bytes, initial_words * 2)[0]
        tests = [
            ('fire-key', 'frames-0.bin', 58, 128, False, None),
            ('gun-position', 'complete-initial-0.bin', 6810 + 42, 1, False, None),
            ('route-answer', 'player-0.bin', initial_words + 2 + 14, 1, False, None),
            ('inventory-answer', 'player-0.bin', initial_words + 62 + 2, 1, False, None),
            ('flight-answer', 'player-0.bin', initial_words + 70, 1, False, None),
            ('projectile-answer', 'player-0.bin', initial_words * 2 + 2 + 30 + 10, 1, False, None),
            ('birth-age-answer', 'player-0.bin', initial_words * 2 + 2 + 30 + 6, 1, False, None),
            ('flight-age-answer', 'player-0.bin', second + initial_words + 2 + 30 + 6, 1, False, None),
            ('overlay-answer', 'player-0.bin', initial_words + 138, 1, False, None),
            ('queued-shot-target', 'cases.bin', 2 * 24 + 22, 1, False, None),
            ('context-total', 'player-context.bin', 42, 65535, False, 0x7f61),
            ('context-total-small', 'player-context.bin', 42, context_words[42], False, 0x7f61),
            ('context-section', 'player-context.bin', 9, 65535, False, 0x7f61),
        ]
    if args.impact:
        if args.player or args.clock or args.tail or manifest['qualification_scope'] != 'impact':
            raise ValueError('impact perturbations require an impact-only artifact')
        case = next(i for i, c in enumerate(manifest['cases']) if c['name'].endswith('-warp'))
        filename = f'player-{case}.bin'
        player_bytes = (assets / filename).read_bytes()
        def packet(frame):
            offset = 0
            for _ in range(frame):
                offset += struct.unpack_from('<H', player_bytes, offset * 2)[0]
            return offset
        base = struct.unpack_from('<H', player_bytes)[0]
        impact = next(packet(frame) for frame in range(1,9)
                      if struct.unpack_from('<H', player_bytes, (packet(frame) + 197) * 2)[0] != 0)
        later = packet(8)
        if struct.unpack_from('<H', player_bytes, (impact + 1) * 2)[0] != 2:
            raise ValueError('impact frame must contain a projectile and explosion')
        context_words = struct.unpack('<56H', (assets / 'player-context.bin').read_bytes()[:112])
        tests = [
            ('mark-count-answer', filename, impact + 143, 1, False, None),
            ('explosion-world-answer', filename, impact + 197, 1, False, None),
            ('blast-total-answer', filename, impact + 297 + 16, 1, False, None),
            ('component-kind-answer', filename, impact + base + 1, 2, False, None),
            ('explosion-payload-answer', filename, impact + base + 642 + 2 + 30, 1, False, None),
            ('later-explosion-answer', filename, later + base + 642 + 2 + 30, 1, False, None),
            ('packet-length-answer', filename, impact, 1, False, None),
            ('impact-resource-section', 'player-context.bin', 39, 1, False, 0x7f61),
            ('explosion-class-resource', 'player-context.bin', context_words[38], 65535, False, None),
        ]
    if args.contact:
        if args.player or args.clock or args.tail or args.impact or manifest['qualification_scope'] != 'contact':
            raise ValueError('contact perturbations require a contact-only artifact')
        case = next(i for i, c in enumerate(manifest['cases']) if c['name'].endswith('-warp'))
        filename = f'player-{case}.bin'
        payload = (assets / filename).read_bytes()
        offsets = []
        cursor = 0
        while cursor * 2 < len(payload):
            offsets.append(cursor)
            cursor += struct.unpack_from('<H', payload, cursor * 2)[0]
        if cursor * 2 != len(payload):
            raise ValueError('unaligned contact packet sequence')
        hit = next(i for i in range(1, len(offsets))
                   if max(struct.unpack_from('<12H', payload, (offsets[i] + 701) * 2)) > 6)
        if hit + 1 >= len(offsets):
            raise ValueError('contact qualification requires a subsequent update')
        context = struct.unpack('<56H', (assets / 'player-context.bin').read_bytes()[:112])
        tests = [
            ('hit-count-answer', filename, offsets[hit] + 701, 1, False, None),
            ('body-revision-answer', filename, offsets[hit] + 713, 1, False, None),
            ('live-grip-answer', filename, offsets[hit] + 721, 1, False, None),
            ('later-hit-answer', filename, offsets[hit + 1] + 701, 1, False, None),
            ('turret-geometry-answer', filename, offsets[hit] + 725, 1, False, None),
            ('combat-model-offset', 'player-context.bin', 44, 65535, False, 0x7f6c),
            ('combat-vertex-capacity', 'player-context.bin', context[38] + context[44] + 8, 512, False, 0x7f6c),
        ]
        turret_case = next((i for i, c in enumerate(manifest['cases'])
                            if c['name'] == 'complete-player-hit-turret-player'), None)
        if turret_case is not None:
            filename = f'player-{turret_case}.bin'
            payload = (assets / filename).read_bytes()
            cursor = 0
            while max(struct.unpack_from('<12H', payload, (cursor + 701) * 2)) <= 6:
                cursor += struct.unpack_from('<H', payload, cursor * 2)[0]
            context_payload = (assets / 'player-context.bin').read_bytes()
            vertices, faces = struct.unpack_from('<2H', context_payload, (context[38] + context[43] + 8) * 2)
            target_geometry = 725 + (vertices + faces) * 3
            tests.extend([
                ('dented-turret-revision', filename, cursor + 717, 1, False, None),
                ('dented-turret-geometry', filename, cursor + target_geometry, 1, False, None),
            ])
    if args.mines:
        if args.player or args.clock or args.tail or args.impact or args.contact or manifest['qualification_scope'] != 'mines':
            raise ValueError('mine perturbations require a mine-only artifact')
        def packets(filename):
            payload = (assets / filename).read_bytes()
            offsets = []
            cursor = 0
            while cursor * 2 < len(payload):
                offsets.append(cursor)
                cursor += struct.unpack_from('<H', payload, cursor * 2)[0]
            if cursor * 2 != len(payload):
                raise ValueError('unaligned mine packet sequence')
            return payload, offsets
        case = next(i for i,c in enumerate(manifest['cases']) if c['name'] == 'complete-player-mines-ordinary')
        filename = f'player-{case}.bin'
        payload, offsets = packets(filename)
        base = struct.unpack_from('<H', payload)[0]
        if len(offsets) != 19 or struct.unpack_from('<H', payload, (offsets[17] + 1) * 2)[0] != 3:
            raise ValueError('mine repeat qualifier requires three drops and a following retirement')
        tests = [
            ('mine-count-answer', filename, offsets[1] + (base-1988) - 154, 1, False, None),
            ('mine-order-answer', filename, offsets[17] + (base-1988) - 154 + 2, 1, False, None),
            ('mine-child-order', filename, offsets[17] + (base-1988) - 150 + 2, 1, False, None),
            ('mine-elapsed-answer', filename, offsets[2] + base + 48, 1, False, None),
            ('mine-repeat-answer', filename, offsets[17] + base + 38, 1, False, None),
            ('mine-retirement-answer', filename, offsets[18] + base, 1, False, None),
            ('initial-mine-order', filename, (base-1988) - 154 + 2, 1, False, 0x7f64),
        ]
        case = next(i for i,c in enumerate(manifest['cases']) if c['name'] == 'complete-player-mines-hit-electric')
        filename = f'player-{case}.bin'
        payload, offsets = packets(filename)
        armed = next(offset for offset in offsets[1:]
                     if struct.unpack_from('<H', payload, (offset + base + 1) * 2)[0] == 2
                     and struct.unpack_from('<H', payload, (offset + base + 42) * 2)[0] != 0)
        hit = next(i for i in range(1, len(offsets))
                   if struct.unpack_from('<H', payload, (offsets[i] + 671) * 2)[0] != 0)
        tests.extend([
            ('mine-arming-answer', filename, armed + base + 42, 1, False, None),
            ('electric-shield-tail', f'complete-{case}.bin', (hit - 1) * 10468 + 9086 + 90 + 88, 1, False, None),
            ('peer-mine-placement', f'world-events-{case}.bin', 2, 1, False, None),
        ])
    if args.ufos:
        if manifest['qualification_scope'] != 'ufos':
            raise ValueError('UFO perturbations require the complete UFO artifact')
        case = next(i for i,c in enumerate(manifest['cases']) if c['name']=='complete-player-ufo-transfer')
        filename=f'player-{case}.bin'
        payload=(assets/filename).read_bytes()
        base=struct.unpack_from('<H',payload)[0]
        offsets=[];cursor=0
        while cursor*2<len(payload):
            offsets.append(cursor);cursor+=struct.unpack_from('<H',payload,cursor*2)[0]
        if cursor*2!=len(payload) or len(offsets)!=49:raise ValueError('complete UFO sequence required')
        def word(frame,column):return struct.unpack_from('<H',payload,(offsets[frame]+column)*2)[0]
        hidden=next(i for i in range(1,len(offsets)) if word(i,base-1988-85)==1)
        shown=next(i for i in range(hidden+1,len(offsets)) if word(i,base-1988-85)==2)
        release=next(i for i in range(shown,len(offsets)) if word(i,base+34)&6==0)
        context=struct.unpack('<56H',(assets/'player-context.bin').read_bytes()[:112])
        frame_words=artifact.constants['DRIVING_FRAME_WORDS']['value']
        tests=[
            ('ufo-target-answer',filename,offsets[1]+base+32,1,False,None),
            ('ufo-timer-answer',filename,offsets[9]+base+52,1,False,None),
            ('ufo-transfer-flags',filename,offsets[hidden]+base+34,1,False,None),
            ('occupied-checkpoint-answer',filename,offsets[hidden]+(base-1988)-82,1,False,None),
            ('visible-order-answer',filename,offsets[hidden]+(base-1988)-18,1,False,None),
            ('ufo-reservation-answer',filename,offsets[hidden]+base+60,1,False,None),
            ('shown-count-answer',filename,offsets[shown]+base-1988-85,1,False,None),
            ('release-flags-answer',filename,offsets[release]+base+34,2,False,None),
            ('retired-component-answer',filename,offsets[46]+1,1,False,None),
            ('inactive-player-input',f'frames-{case}.bin',hidden*frame_words+58,4,True,0x600d),
            ('hidden-racer-clock-answer',filename,offsets[hidden+1]+24,1,False,None),
            ('initial-ufo-capacity',filename,(base-1988)-86,33,False,0x7f72),
            ('ufo-point-metadata','player-context.bin',51,1,False,0x7f71),
            ('ufo-resource-height','player-context.bin',context[38]+context[48]+4,1,False,None),
            ('independent-ufo-target',f'world-events-{case}.bin',2,1,False,None),
        ]
    if args.object_ufos:
        if manifest['qualification_scope'] != 'object-ufos' or len(manifest['cases']) != 1:
            raise ValueError('object UFO checks require one complete transfer trajectory')
        filename = 'player-0.bin'
        payload = (assets / filename).read_bytes()
        base = struct.unpack_from('<H', payload)[0]
        offsets = []; cursor = 0
        while cursor * 2 < len(payload):
            offsets.append(cursor); cursor += struct.unpack_from('<H', payload, cursor * 2)[0]
        if cursor * 2 != len(payload) or len(offsets) != 81:
            raise ValueError('80-update object UFO sequence required')
        def word(frame, column):
            return struct.unpack_from('<H', payload, (offsets[frame] + column) * 2)[0]
        hidden = next(i for i in range(1, len(offsets)) if word(i, base + 34) & 6 == 2)
        shown = next(i for i in range(hidden + 1, len(offsets)) if word(i, base + 34) & 16)
        released = next(i for i in range(shown, len(offsets)) if word(i, base + 34) & 6 == 0)
        retired = next(i for i in range(released + 1, len(offsets)) if word(i, 1) == 0)
        tests = [
            ('object-target-input', 'world-events-0.bin', 2, 1, False, None),
            ('object-destination-input', 'world-events-0.bin', 3, 1, False, None),
            ('object-target-answer', filename, offsets[1] + base + 32, 1, False, None),
            ('object-destination-answer', filename, offsets[1] + base + 36, 1, False, None),
            ('object-disable-answer', filename, offsets[9] + base + 34, 8, False, None),
            ('object-hide-answer', filename, offsets[hidden] + base + 34, 2, False, None),
            ('object-show-answer', filename, offsets[shown] + base + 34, 16, False, None),
            ('object-release-answer', filename, offsets[released] + base + 34, 4, False, None),
            ('object-retirement-answer', filename, offsets[retired] + 1, 1, False, None),
            ('object-resource-height', 'player-context.bin',
             struct.unpack_from('<H', (assets / 'player-context.bin').read_bytes(), 76)[0]
             + struct.unpack_from('<H', (assets / 'player-context.bin').read_bytes(), 96)[0] + 4,
             1, False, None),
        ]
        event = struct.unpack('<4H', (assets / 'world-events-0.bin').read_bytes()[:8])
        uses_authored_mesh = manifest['cases'][0]['name'].endswith('-prop')
        if not uses_authored_mesh:
            tests.append(('live-mesh-binding-precedence', 'object-meshes.bin', event[2],
                          65535, True, 0x600d))
    if args.emitters:
        if manifest['qualification_scope'] != 'emitters':
            raise ValueError('emitter perturbations require an emitter artifact')
        case = next(i for i,c in enumerate(manifest['cases']) if c['name'].endswith('-rain'))
        filename = f'player-{case}.bin'
        payload = (assets / filename).read_bytes()
        base = struct.unpack_from('<H', payload)[0]
        offsets=[];cursor=0
        while cursor*2<len(payload):
            offsets.append(cursor);cursor+=struct.unpack_from('<H',payload,cursor*2)[0]
        if cursor*2!=len(payload) or len(offsets)!=49:
            raise ValueError('48-update rain sequence required')
        context=struct.unpack('<56H',(assets/'player-context.bin').read_bytes()[:112])
        timer=base-1988+50
        tests=[
            ('timer-answer',filename,offsets[1]+timer,1,False,None),
            ('inactive-timer-answer',filename,offsets[13]+timer,1,False,None),
            ('reentry-timer-answer',filename,offsets[16]+timer,1,False,None),
            ('emitted-kind-answer',filename,offsets[4]+base+32+4,1,False,None),
            ('emitter-reference-answer',filename,offsets[4]+base+2+8,1,False,None),
            ('active-flash-answer',filename,offsets[39]+139,1,False,None),
            ('expired-flash-presence',filename,offsets[42]+138,1,False,None),
            ('inactive-flash-padding',filename,offsets[42]+139,1,True,0x600d),
            ('live-camera-input',f'camera-{case}.bin',0,32768,False,None),
            ('authored-period','player-context.bin',context[38]+context[54]+51+1+24,1,False,None),
            ('emitter-object','player-context.bin',context[38]+context[54]+51,1,False,0x7f77),
            ('emitter-count','player-context.bin',55,1,False,0x7f71),
            ('emitter-section','player-context.bin',54,1,False,0x7f71),
        ]
    if args.trains:
        if manifest['qualification_scope'] != 'trains':
            raise ValueError('train perturbations require a train artifact')
        case=next(i for i,c in enumerate(manifest['cases']) if c['name'].endswith('-reversal'))
        filename=f'player-{case}.bin';payload=(assets/filename).read_bytes()
        base=struct.unpack_from('<H',payload)[0];offsets=[];cursor=0
        while cursor*2<len(payload):
            offsets.append(cursor);cursor+=struct.unpack_from('<H',payload,cursor*2)[0]
        if cursor*2!=len(payload) or len(offsets)!=83:
            raise ValueError('82-update reversal and wait sequence required')
        def word(frame,column):return struct.unpack_from('<H',payload,(offsets[frame]+column)*2)[0]
        tail=base-1618-60
        arrived=next(i for i in range(1,len(offsets)) if word(i,tail+4)!=0)
        resumed=next(i for i in range(arrived+1,len(offsets)) if word(i,tail+4)==0)
        context=struct.unpack('<56H',(assets/'player-context.bin').read_bytes()[:112])
        frame_words=artifact.constants['DRIVING_FRAME_WORDS']['value']
        scene_name=f'scenery-{case}.bin';scene=(assets/scene_name).read_bytes();position=0
        for _ in range(arrived):position+=struct.unpack_from('<H',scene,(position+2)*2)[0]
        h=struct.unpack_from('<24H',scene,position*2);row=position+h[11]
        found=None
        for _ in range(h[12]):
            if struct.unpack_from('<H',scene,row*2)[0]==239:found=row+1;break
            row+=77
        if found is None:raise ValueError('live train collider publication required')
        tests=[
            ('train-state-answer',filename,offsets[1]+tail,1,False,None),
            ('arrival-answer',filename,offsets[arrived]+tail+4,1,False,None),
            ('wait-answer',filename,offsets[arrived+1]+tail+6,1,False,None),
            ('wait-end-answer',filename,offsets[resumed]+tail+4,1,False,None),
            ('reversed-path-answer',filename,offsets[arrived]+tail+42,1,False,None),
            ('shared-RNG-answer',f'frames-{case}.bin',arrived*frame_words-2,1,False,0x7f05),
            ('train-position-answer',scene_name,found+2,1,False,None),
            ('train-external-basis-answer',scene_name,found+26,1,False,None),
            ('independent-dt',f'frames-{case}.bin',0,1,False,None),
            ('independent-initial-RNG','seeds.bin',4+case*2,1,False,0x7f03),
            ('initial-RNG-header','seeds.bin',0,1,False,0x7f7c),
            ('authored-speed','player-context.bin',context[38]+context[7]+2+38,1,False,None),
            ('train-object','player-context.bin',context[38]+context[7],1,False,0x7f7a),
            ('train-sound','player-context.bin',context[38]+context[7]+1,2,False,0x7f7a),
            ('train-section','player-context.bin',7,1,False,0x7f71),
        ]
    if args.ambient:
        if manifest['qualification_scope'] != 'ambient':
            raise ValueError('ambient perturbations require an ambient artifact')
        case = next(i for i,c in enumerate(manifest['cases']) if c['name'].endswith('-cycle'))
        filename = f'player-{case}.bin'
        payload = (assets / filename).read_bytes()
        base = struct.unpack_from('<H', payload)[0]
        offsets=[];cursor=0
        while cursor*2<len(payload):
            offsets.append(cursor);cursor+=struct.unpack_from('<H',payload,cursor*2)[0]
        if cursor*2!=len(payload) or len(offsets)!=57:
            raise ValueError('56-update smoke freeze/reentry and sound-region sequence required')
        context=struct.unpack('<60H',(assets/'player-context.bin').read_bytes()[:120])
        ambient=context[38]+context[56]
        tail=base-1618
        # selected i32, sequence u16 plus padding, five i32 smoke fields, 16 particles.
        smoke=tail+4
        def word(frame,column):return struct.unpack_from('<H',payload,(offsets[frame]+column)*2)[0]
        active=next(i for i in range(1,len(offsets)) if word(i,smoke+6)!=0)
        frozen=next(i for i in range(active+1,len(offsets)) if word(i,smoke+6)==0)
        resumed=next(i for i in range(frozen+1,len(offsets)) if word(i,smoke+6)!=0)
        particle=next(i for i in range(16) if word(active,smoke+10+i*26+20)!=0)
        row=smoke+10+particle*26
        tests=[
            ('sound-selection-answer',filename,offsets[1]+tail,1,False,None),
            ('smoke-sequence-answer',filename,offsets[active]+tail+2,1,False,None),
            ('smoke-timer-answer',filename,offsets[active]+smoke+4,1,False,None),
            ('smoke-visible-answer',filename,offsets[active]+smoke+6,1,False,None),
            ('smoke-particle-answer',filename,offsets[active]+row+14,1,False,None),
            ('frozen-particle-answer',filename,offsets[frozen]+row+14,1,False,None),
            ('resumed-particle-answer',filename,offsets[resumed]+row+14,1,False,None),
            ('live-camera-input',f'camera-{case}.bin',0,32768,False,None),
            ('ambient-magic','player-context.bin',ambient,1,False,0x7f7d),
            ('ambient-count','player-context.bin',ambient+2,1,False,0x7f7d),
            ('ambient-sequence-range','player-context.bin',ambient+3,16,False,0x7f7d),
            ('ambient-section','player-context.bin',56,1,False,0x7f7d),
            ('ambient-owner','player-context.bin',58,1,False,0x7f7d),
            ('context-queue-boundary','player-context.bin',42,context[42] ^ 5008,False,0x7f61),
        ]
    if args.flocks:
        if manifest['qualification_scope'] != 'flocks' or len(manifest['cases']) != 1:
            raise ValueError('flock perturbations require one complete flock trajectory')
        filename='player-0.bin';payload=(assets/filename).read_bytes()
        base=struct.unpack_from('<H',payload)[0];offsets=[];cursor=0
        while cursor*2<len(payload):
            offsets.append(cursor);cursor+=struct.unpack_from('<H',payload,cursor*2)[0]
        if cursor*2!=len(payload) or len(offsets)!=65:
            raise ValueError('64-update flock trajectory required')
        index=int(manifest['cases'][0]['name'].rsplit('-',1)[1])-183
        tail=base-1188+index*396
        def word(frame,column):return struct.unpack_from('<H',payload,(offsets[frame]+column)*2)[0]
        takeoff=next(i for i in range(1,len(offsets)) if word(i,tail)==1)
        stage=next(i for i in range(takeoff+1,len(offsets)) if word(i,tail+2)==1)
        initial=(assets/'flock-initial.bin').read_bytes();row=16+index*348
        low=struct.unpack_from('<H',initial,(row+293)*2)[0]
        surfaces=(assets/'deform-bound.bin').read_bytes()
        surface_count=artifact.constants['DRIVING_DEFORM_COUNT']['value']
        last_surface=sum(5+struct.unpack_from('<H',surfaces,(i*40+1)*2)[0]*3
                         for i in range(surface_count-1))
        owner_symbol='DRIVING_PLAYER_DEFORM_IDS'
        while owner_symbol in manifest['resource_aliases']:
            owner_symbol=manifest['resource_aliases'][owner_symbol]
        owner_file=next(r['file'] for r in manifest['resources'] if r['symbol']==owner_symbol)
        tests=[
            ('takeoff-answer',filename,offsets[takeoff]+tail,1,False,None),
            ('bird-position-answer',filename,offsets[takeoff+1]+tail+64,1,False,None),
            ('timer-equality-answer',filename,offsets[stage-1]+tail+4,1,False,None),
            ('stage-target-answer',filename,offsets[stage]+tail+6,1,False,None),
            ('wing-model-answer',filename,offsets[takeoff+1]+tail+288+low*3+1,1,False,None),
            ('wing-mesh-answer',filename,offsets[takeoff+1]+tail+342+low*3+1,1,False,None),
            ('mesh-revision-answer',filename,offsets[takeoff]+tail+338,1,False,None),
            ('stopped-wing-answer',filename,offsets[60]+tail+288+low*3+1,1,False,None),
            ('shared-RNG-answer','frames-0.bin',takeoff*artifact.constants['DRIVING_FRAME_WORDS']['value']-2,1,False,0x7f05),
            ('independent-frame-number','frames-0.bin',52,1,False,None),
            ('independent-camera','camera-0.bin',0,32768,False,None),
            ('flock-magic','flock-initial.bin',0,1,False,0x7f7f),
            ('flock-count','flock-initial.bin',2,1,False,0x7f7f),
            ('flock-extent','flock-initial.bin',7,1,False,0x7f7f),
            ('flock-object','flock-initial.bin',row,1,False,0x7f80),
            ('wing-extrema','flock-initial.bin',row+293,16,False,0x7f80),
            ('inactive-surface-answer','surfaces-0.bin',last_surface*2,1,False,None),
            ('inactive-mesh-revision','mesh-versions-0.bin',(surface_count-1)*4,1,False,None),
            ('surface-flag-offset','deform-bound.bin',4,1,False,0x7f82),
            ('surface-vertex-count','deform-bound.bin',1,1,False,0x7f82),
            ('surface-record-count','deform-bound.bin',38,1,False,0x7f82),
            ('latent-owner-binding',owner_file,surface_count-1,1,False,None),
        ]
    if args.clock:
        if args.player or manifest['qualification_scope'] != 'clock':
            raise ValueError('clock perturbations require the clock-only artifact')
        player = (assets / 'player-0.bin').read_bytes()
        offset = struct.unpack_from('<H', player)[0]
        first = offset
        for _ in range(3):
            offset += struct.unpack_from('<H', player, offset * 2)[0]
        if struct.unpack_from('<i', player, (offset + 24) * 2)[0] != 250:
            raise ValueError('clock trajectory must expose raw 250-ms racer elapsed')
        tests = [
            ('raw-elapsed', 'frames-0.bin', 0, 1, False, None),
            ('derived-stage-unused', 'frames-0.bin', 58, 512, True, None),
            ('clock-answer', 'clock-0.bin', 6, 1, False, None),
            ('racer-elapsed-answer', 'player-0.bin', offset + 24, 1, False, None),
            ('countdown-driving-answer', 'player-0.bin', first + 28, 1, False, None),
            ('initial-clock', 'clock-0.bin', 0, 1, False, None),
        ]
    if args.tail:
        if args.clock or args.player or manifest['qualification_scope'] != 'tail':
            raise ValueError('tail perturbations require the tail-only artifact')
        initial_words = struct.unpack_from('<H', (assets / 'player-0.bin').read_bytes())[0]
        cases = struct.unpack('<24H', (assets / 'cases.bin').read_bytes()[:48])
        frame_words = artifact.constants['DRIVING_FRAME_WORDS']['value']
        if len(manifest['cases']) != 2 or cases[20] != 3:
            raise ValueError('tail fixture must contain horn and hidden three-frame trajectories')
        tests = [
            ('horn-key', 'frames-0.bin', 58, 256, False, None),
            ('horn-timer-answer', 'player-0.bin', initial_words + 126, 1, False, None),
            ('gun-revision-answer', 'player-0.bin', initial_words + 127, 1, False, None),
            ('hidden-input', 'frames-1.bin', frame_words + 54, 1, False, None),
        ]
    tests.insert(0, ('baseline', 'identity.bin', 0, 0, True, 0x600d))
    package = artifact.files[artifact.manifest['package']]
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for label, name, word, mask, passed, expected_signal in tests:
        payload = (assets / name).read_bytes()
        offset = resource_offset(package, payload)
        changed = bytearray(package)
        value = struct.unpack_from('<H', changed, offset + word * 2)[0]
        struct.pack_into('<H', changed, offset + word * 2, value ^ mask)
        folder = args.output / label
        folder.mkdir()
        image_path = folder / 'changed.cb06'
        image_path.write_bytes(changed)
        command = [str(args.runtime.resolve()), '--package', str(image_path.resolve()),
                   '--cpu-mode', 'instruction', '--frames', '4096',
                   '--max-cpu-steps', '300000000', '--out-dir', str((folder / 'sim').resolve())]
        with (folder / 'run.log').open('w') as log:
            subprocess.run(command, check=True, timeout=90, stdout=log, stderr=subprocess.STDOUT)
        summary = json.loads((folder / 'sim/summary-instruction.json').read_text())
        signal = summary['halt_signal']
        if summary['cpu_fault'] or summary['gpu_error'] or signal is None:
            raise ValueError(f'{label}: {summary}')
        if (signal == 0x600d) != passed or (expected_signal is not None and signal != expected_signal):
            raise ValueError(f'{label}: unexpected halt {signal:#x}')
        results.append(dict(label=label, file=name, word=word, expected_pass=passed, halt=signal))
    report = dict(passed=True, artifact_id=artifact.manifest['artifact_id'], tests=results,
                  scope='Authored world order, peer Tank updates, raw clock, pickup flight owners and bounded flock storage are checked.' if args.world else
                        'Flock takeoff, flight and stop, all wing vertices and mesh revisions, shared RNG, independent camera/frame number and bounded initial owners are checked.' if args.flocks else
                        'Ambient selection, full smoke state, independent camera, frozen/reentered particles, shared sequence and property-page bounds are checked.' if args.ambient else
                        'Live Train activation, arrival, reversal, wait expiry, shared RNG, collider pose/external basis, independent dt and authored metadata are checked.' if args.trains else
                        'Authored emitter timers, camera exit/reentry, live projectile kind/emitter, independent camera input and source metadata are checked.' if args.emitters else
                        'Object UFO target and destination inputs, nested disable/move/show/release, retirement and shared resource height are checked.' if args.object_ufos else
                        'UFO attachment, transfer, reservation, visibility, retirement, immutable metadata and independent events are checked; hidden Player input is unused.' if args.ufos else
                        'Mine count/order, repeat/arming/retirement, strided shield state and independent peer placement are checked against continuous source updates.' if args.mines else
                        'Live hit counts, body revisions, per-car grip and subsequent updates are checked; malformed combat metadata is rejected.' if args.contact else
                        'Horn keys, horn timer, gun revision and hidden-vehicle input are checked against complete original Player calls.' if args.tail else
                        'Raw elapsed and persistent clock/racer answers are checked; source display-stage controls are recomputed.' if args.clock else
                        'Independent Player keys/gun and route/inventory/flight/projectile answers are checked; malformed context is rejected.' if args.player else
                        'Diagnostic class/object snapshots are unused; canonical identities and live '
                        'components remain checked, and malformed source sections are rejected before loading.')
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(passed=True, perturbations=len(results))))


if __name__ == '__main__':
    main()
