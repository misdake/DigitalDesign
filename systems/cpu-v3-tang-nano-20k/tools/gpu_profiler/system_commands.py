"""Normalize committed CP facts. Never read initial CB bytes or emulate commands."""
from model import OP


def normalize(system, partial):
    from system_emu import require, integer
    events = system.get('command_events', [])
    capable = system['capabilities'].get('command_control_lineage', False)
    require(type(capable) is bool and (capable or not events), 'command capability')
    if not capable:
        return [], []
    require(type(events) is list and len(events) <= 10000000, 'command event bounds')
    axis = system['axis_origin_tick']
    cycle = lambda t: (t-axis)//148
    source = {s['id']: s for s in system['submissions']}
    submissions, commands = [], []
    sub, command = None, None
    prior_tick = system['origin_tick']
    for row in events:
        require(type(row) is list and len(row) == 3, 'command event envelope')
        tick, kind, value = row
        require(integer(tick) and prior_tick <= tick <= system['coverage_end_tick'] and tick % 148 == 0,
                'command event tick')
        prior_tick = tick
        require(type(value) is list and all(integer(v) for v in value), 'command event values')
        if kind == 'cb_submit':
            require(len(value) == 3, 'command descriptor shape')
            sid, base, size = value
            require(sid == len(submissions)+1 and sid in source and source[sid]['start_tick'] == tick,
                    'command descriptor submit binding')
            require(sub is None or sub.get('drained_tick') is not None or sub.get('reset_tick') is not None,
                    'command submit before drain/reset')
            require(base % 128 == 0 and base < 8388608 and 4 <= size <= 65536 and size % 4 == 0
                    and base + ((size+127)//128)*128 <= 8388608, 'command descriptor range')
            sub = dict(id=sid, base=base, bytes=size, start=cycle(tick), end=None,
                       drained_tick=None, stop_tick=None, stop_reason=None, reset_tick=None)
            submissions.append(sub); command = None
        else:
            require(sub is not None and value and value[0] == sub['id'] and sub['drained_tick'] is None
                    and (sub['stop_tick'] is None or kind == 'cb_reset' and sub['reset_tick'] is None),
                    'command event owner')
            if kind in ('cb_abort', 'cb_reset'):
                require(len(value) == 1, 'command stop shape')
                if sub['stop_tick'] is None:sub.update(stop_tick=tick, stop_reason=kind.removeprefix('cb_'))
                if kind == 'cb_reset': sub['reset_tick'] = tick
                continue
            if kind == 'cb_drain':
                require(len(value) == 1 and command is not None and command['opcode'] == 15
                        and command['release_tick'] is not None and command['pc']+4 == sub['bytes'],
                        'command drain before END release')
                sub.update(drained_tick=tick, end=cycle(tick)+1)
                command['end'] = cycle(tick)+1
                continue
            require(kind in ('cb_latch', 'cb_execute', 'cb_issue', 'cb_release') and len(value) == 3,
                    'command event kind/shape')
            _, pc, word = value
            require(pc % 4 == 0 and pc < sub['bytes'] and word <= 0xffffffff, 'command PC/word')
            if kind == 'cb_latch':
                require(command is None and pc == 0 or command is not None
                        and command['release_tick'] is not None and command['opcode'] != 15
                        and pc == command['pc']+4, 'command latch order')
                start = sub['start'] if command is None else cycle(command['release_tick'])+1
                command = dict(id=len(commands)+1, submission=sub['id'], pc=pc, address_bytes=sub['base']+pc,
                               word=f'{word:08x}', opcode=word >> 28, name=OP[word >> 28],
                               start=start, execute=None, end=None, latch_tick=tick, execute_tick=None,
                               issue_tick=None, release_tick=None, states=[], waits=[], jobs=[], dependencies=[])
                commands.append(command)
            else:
                require(command is not None and command['pc'] == pc and command['word'] == f'{word:08x}',
                        'command event word/PC differs from actual latch')
                key = {'cb_execute':'execute_tick', 'cb_issue':'issue_tick', 'cb_release':'release_tick'}[kind]
                prerequisite = {'cb_execute':'latch_tick', 'cb_issue':'execute_tick', 'cb_release':'issue_tick'}[kind]
                require(command[key] is None and command[prerequisite] is not None
                        and command[prerequisite] <= tick, 'command event order/duplicate')
                command[key] = tick
                if kind == 'cb_execute': command['execute'] = cycle(tick)
                if kind == 'cb_release' and command['opcode'] != 15: command['end'] = cycle(tick)+1
    if not partial:
        require(len(submissions) == len(source) and all(s['drained_tick'] is not None for s in submissions),
                'complete capture missing command drain/descriptor')
    for sub in submissions:
        original = source[sub['id']]
        if original['end_tick'] is not None:
            require(sub['drained_tick'] is not None and original['end_tick'] == sub['drained_tick']+148,
                    'GPU done differs from committed drain')
    return submissions, commands


def validate(data):
    from system_emu import require
    subs, commands = normalize(data['system'], data['partial'])
    from system_gpu import attach, normalize as normalize_gpu
    expected=dict(data,submissions=subs,commands=commands,system=dict(data['system']))
    gpu=normalize_gpu(expected);attach(expected,gpu)
    for key in ('submissions','commands') + (('jobs','meshlets','dma_transactions','metric_defs','tracks') if gpu else ()):
        require(data[key] == expected[key], 'normalized '+key+' facts differ from committed trace')
    actual=data['system'].get('gpu');graph=expected['system'].get('gpu')
    if graph and actual:
        # Earlier profile-v2 files retained the exact raw journal but not these
        # optional derived arrays. Present arrays must still match every fact.
        graph=dict(graph)
        for key in ('master_payloads','master_terminals','probes'):
            if key not in actual:graph.pop(key,None)
    require(actual==graph, 'normalized GPU owner facts differ')
    if gpu:require(data.get('memory')==expected['memory'],'GPU memory accounting differs from observed master events')
