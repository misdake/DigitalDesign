"""Bind the ordinary level-29 scenario to one verified, linked application."""
import re


def bind(artifact):
    from application_artifact import require
    metadata = [raw for name, raw in artifact.files.items() if name.endswith('/assets.json')]
    import json
    require(any(json.loads(raw).get('initial_level') == 29 for raw in metadata),
            'level-29 scenario requires an artifact built with --start-level 29')
    def word(name, index=0):
        symbol = artifact.symbol(name, 'global')
        match = re.fullmatch(r'Buf<u16, (\d+)>', symbol['type'])
        require(match and index < int(match[1]), 'probe shape '+name)
        return symbol['physical_word']+index
    words = {key: word(name, offset) for key, name, offset in [
        ('phase','SESSION',0),('moving','MOTION',5),('level','MOTION',6),
        ('x','GAME',0),('z','GAME',1),('orientation','GAME',2),('mask','GAME',3),
        ('transition','TRANSITION',0),('motion_age','MOTION',4),('epoch','PROFILER_CLOCK',1)]}
    words.update({f'switch_age_{i}': word('SWITCH_FX',i) for i in range(1,10)})
    before = artifact.call('run','gamepad_capture','gamepad_capture();\n        let input = gamepad_take_frame();')
    consumed = artifact.call('run','gamepad_take_frame')
    sample = artifact.call('run','session_tick')
    vblank = artifact.call('run',artifact.caller('gpu_present_wait_vblank'))
    wait = artifact.call('wait_game',statement='let completed = gpu.wait();')
    preceding = [i['text'] for i in artifact.instructions('wait_game') if i['pc']+i['words']-1 < wait['execute_pc']]
    require(preceding[-4:] == ['mov r8, r6', 'mov r9, r5', 'mov r10, r4', 'mov r11, r3'],
            'ordinary wait argument ABI changed')
    following = [i['text'] for i in artifact.instructions('wait_game') if i['pc'] >= wait['return_pc']]
    require(len(following) > 1 and following[1] == 'cmpui r10, 0', 'ordinary wait finish ABI changed')
    pair = lambda site, key: [site['segment'],site[key]]
    clocks = {}
    for key, name, mnemonic in [('clear','time_clear_microseconds','send'),('read','time_microseconds','recv')]:
        if name not in artifact.functions:
            continue
        sites = [i for i in artifact.instructions(name) if i['text'].startswith(mnemonic+' ')]
        require(len(sites) == 1, 'unique timer MMIO '+name)
        clocks[key] = dict(code_segment=artifact.manifest['target']['code_segment'],pc=sites[0]['pc']+sites[0]['words']-1)
    require(not clocks or set(clocks) == {'clear','read'}, 'incomplete timer endpoints')
    return dict(schema='digitaldesign.ordinary-profile-plan',version=1,site_observation='first-unheld-execute',
                words=words,gates=dict(before_input=pair(before,'execute_pc'),consumed=pair(consumed,'return_execute_pc'),sample=pair(sample,'return_execute_pc')),
                sites=dict(vblank_return=pair(vblank,'return_execute_pc'),final_wait_return=pair(wait,'return_execute_pc')),
                clock_sites=clocks,application_identity=dict(artifact_id=artifact.manifest['artifact_id'],
                    scope='Same-build source/global/call/listing binding; no recompile; no fabricated timer endpoints'))
