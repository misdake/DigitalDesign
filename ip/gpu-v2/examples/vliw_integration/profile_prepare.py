"""Replay with observers separating private prepare work from publication wait."""
import hashlib
import json
import re
import sys
from pathlib import Path

import replay_export as replay


def observe(bench):
    counters = dict(
        lifetime='dut.core.front.s_busy',
        work='pw_work',
        rom='dut.core.front.interpolator.prepare.math_phase',
        quant='pw_quant',
        source_wait='dut.core.front.s_busy && dut.core.front.interpolator.prepare.input_phase && !dut.core.front.raw_word_valid',
        publish_wait='dut.core.front.s_busy && dut.core.front.interpolator.prepare.commit_phase && !dut.core.front.interpolator.execute_grant',
        publish_r_only='pw_publish && pw_r && !pw_i',
        publish_i_only='pw_publish && !pw_r && pw_i',
        publish_both='pw_publish && pw_r && pw_i',
        overlap_r='pw_work && dut.core.front.raster_busy',
        overlap_i='pw_work && dut.core.front.interpolator.p_busy',
        raster_start='dut.core.front.raster_start',
        raster_start_i_busy='dut.core.front.raster_start && pw_i',
        context_switch_r_busy='dut.core.front.interpolator.cfg_apply && pw_r',
        r_active_i_tail='pw_r && !dut.core.front.run_enable && pw_i',
        empty_context='pw_empty',
        empty_with_old_r='pw_empty && pw_r',
        empty_with_old_i='pw_empty && pw_i',
    )
    declarations = '''
wire pw_r=dut.core.front.raster_busy,pw_i=dut.core.front.interpolator.p_busy;
wire pw_publish=dut.core.front.s_busy &&
 dut.core.front.interpolator.prepare.commit_phase &&
 !dut.core.front.interpolator.execute_grant;
wire pw_quant=dut.core.front.interpolator.prepare.quant_read ||
 dut.core.front.interpolator.prepare.read_valid ||
 (|dut.core.front.interpolator.prepare.quant_pipe);
wire pw_work=dut.core.front.interpolator.prepare.input_take || pw_quant ||
 dut.core.front.interpolator.prepare.math_phase ||
 dut.core.front.interpolator.cfg_apply || dut.core.front.s_done || pw_empty;
'''
    export = Path(sys.argv[sys.argv.index('--export')+1])
    empty = ('dut.core.front.interpolator.discard_context'
             if 'wire discard_context=' in export.read_text() else "1'b0")
    declarations = 'wire pw_empty='+empty+';\n'+declarations
    raster = 'dut.core.front.raster.'
    # R lifetime is partitioned exclusively. The additional activity counters
    # deliberately overlap it, and must not be added to this partition.
    for name, condition in dict(
        import_='life==1', bound='life==3',
        exec_prep='life==2 && evt_prep',
        exec_scan='life==2 && !evt_prep && (scan_mode==4 || scan_mode==2)',
        exec_load='life==2 && !evt_prep && scan_mode==1',
        exec_align='life==2 && !evt_prep && scan_mode==3',
        exec_wait_tile='life==2 && !evt_prep && scan_mode==0 && !walk_done',
        exec_drain='life==2 && !evt_prep && scan_mode==0 && walk_done',
        walk_issue='micro_issue', walk_retire='evt_walk_retire',
        walk_branch_wait='evt_branch_wait', walk_mailbox_full='evt_tile_wait',
        scan_check='life==2 && scan_mode==2',
        scan_full='life==2 && scan_mode==4 && scan_full',
        scan_partial='life==2 && scan_mode==4 && !scan_full',
        scan_credit='evt_credit_wait', scan_check_start='check_start',
        scan_quad='quad_take', scan_empty='evt_empty_quad',
        tile_load='start_tile && !prefetch_handoff',
        tile_prefetched='prefetch_handoff',
    ).items():
        condition = re.sub(r'\b[a-zA-Z_]\w*\b', lambda m: raster+m[0], condition)
        counters['r_'+name.rstrip('_')] = 'pw_r && ('+condition+')'
    declarations += ''.join(f'integer pw_count_{key}=0;\n' for key in counters)
    declarations += 'always @(posedge clk)if(ce&&!reset)begin\n'
    declarations += ''.join(f' if({condition})pw_count_{key}=pw_count_{key}+1;\n'
                            for key, condition in counters.items()) + 'end\n'
    # These observers never drive DUT inputs or CE. Preserve the bounded bench.
    assert bench.count('gpu_v2_vliw_sdram dut(.*);') == 1
    bench = bench.replace('gpu_v2_vliw_sdram dut(.*);',
                          'gpu_v2_vliw_sdram dut(.*);\n' + declarations)
    report = '$display("PROFILE ' + ' '.join(f'{k}=%0d' for k in counters)
    report += '",' + ','.join(f'pw_count_{k}' for k in counters) + ');\n '
    assert bench.count('$display("PASS BUNNY') == 1
    return bench.replace('$display("PASS BUNNY', report + '$display("PASS BUNNY')


def main():
    observer_source = Path(__file__).read_bytes()
    original_load = replay.load

    def load(name, path):
        module = original_load(name, path)
        if name == 'export_replay_bunny':
            original_bench = module.bench
            module.bench = lambda *a, **kw: observe(original_bench(*a, **kw))
        return module

    replay.load = load
    replay.main()
    out = Path(sys.argv[sys.argv.index('--output')+1]).resolve()
    (out/'profile-observer.py').write_bytes(observer_source)
    line = next(s for s in (out/'run.log').read_text().splitlines()
                if s.startswith('PROFILE '))
    counters = {k: int(v) for k, v in re.findall(r'(\w+)=(\d+)', line)}
    receipt = json.loads((out/'receipt.json').read_text())
    assert counters['publish_wait'] == sum(counters['publish_'+k]
                                          for k in ('r_only', 'i_only', 'both'))
    partition = ('import', 'bound', 'exec_prep', 'exec_scan', 'exec_load',
                 'exec_align', 'exec_wait_tile', 'exec_drain')
    assert receipt['activity']['Rbusy'] == sum(counters['r_'+k] for k in partition)
    result = dict(counters=counters, dut_sha256=receipt['dut_sha256'],
                  bench_sha256=receipt['source_sha256']['tb.v'],
                  observer_sha256=hashlib.sha256(observer_source).hexdigest(),
                  boundary='Enabled-edge observers; work is a union, subcounts overlap. '
                           'Publication wait owns no runtime port. Replay receipt verifies outputs.',
                  partitions=dict(publication=['publish_r_only', 'publish_i_only', 'publish_both'],
                                  raster_lifetime=['r_'+k for k in partition]),
                  note='Other activity counters overlap. With split handoff, R may already '
                       'belong to the new triangle while I drains the old triangle.')
    (out/'prepare-profile.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(counters, indent=2))


if __name__ == '__main__':
    main()
