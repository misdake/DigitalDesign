"""Bounded replay source-identity checks; no scene or GPU simulation required."""
from pathlib import Path
import json
import os
import shutil
import sys
import tempfile
import unittest
from unittest.mock import Mock

import replay_export as replay
from stream_contract import fnv1a64, load_contract


class ReplayExportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.peer = self.root/'peer'
        self.out = self.root/'run'
        self.peer.mkdir()
        self.out.mkdir()
        self.source = self.root/'selected.v'
        self.source.write_text('module selected(output value); assign value=1; endmodule\n')
        shutil.copyfile(self.source, self.out/'dut.v')
        self.expected = replay.sha(self.source)

    def contract(self, files=('peer.v',)):
        document = dict(schema=1, profile='stream-54-108-cl3-v1', logic_hz=54_000_000,
                        controller_hz=108_000_000, cas=3, rcd=3, rp=3, capture_latency=3,
                        registered_read_return=1, dq_input_delay_taps=32, phase_degrees=157.5,
                        init_cycles=40, files={})
        for name in files:
            data = (self.peer/name).read_bytes()
            document['files'][name] = dict(bytes=len(data), fnv1a64=fnv1a64(data))
        (self.peer/'stream-contract.json').write_text(json.dumps(document))

    def run_simulation(self, execute):
        return replay.run_checked_simulation(
            execute, ['compiler'], 'simulator', self.out, self.source, self.expected, 10)

    def test_peer_cannot_replace_selected_dut_or_generated_bench(self):
        (self.peer/'DUT.V').write_text('old GPU: must never compile\n')
        (self.peer/'tb.v').write_text('old bench: must never compile\n')
        (self.peer/'ports.vh').write_text('`define PEER_VALUE 1\n')
        (self.peer/'peer.v').write_text('module peer; endmodule\n')
        (self.peer/'unused.txt').write_text('not a compiler input\n')
        (self.peer/'directory.v').mkdir()
        self.contract(('peer.v', 'ports.vh'))
        bench = self.out/'tb.v'
        bench.write_text('generated bench\n')
        sources = replay.copy_peer_sources(self.peer, self.out)
        self.assertEqual(sources, [self.out/'peer.v'])
        self.assertEqual(replay.sha(self.out/'dut.v'), self.expected)
        self.assertEqual(bench.read_text(), 'generated bench\n')
        self.assertEqual(sorted(p.name for p in self.out.iterdir()),
                         ['dut.v', 'peer.v', 'ports.vh', 'stream-contract.json', 'tb.v'])

    def test_old_peer_without_current_contract_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'Missing current stream contract'):
            replay.copy_peer_sources(self.peer, self.out)

    def test_mixed_peer_sources_are_rejected(self):
        (self.peer/'peer.v').write_text('module peer; endmodule\n')
        self.contract()
        (self.peer/'peer.v').write_text('module old_peer; endmodule\n')
        with self.assertRaisesRegex(ValueError, 'Stale or mixed'):
            load_contract(self.peer)

    def test_ports_include_must_be_bound_and_cannot_be_mixed(self):
        (self.peer/'peer.v').write_text('module peer; endmodule\n')
        (self.peer/'ports.vh').write_text('wire correct_port;\n')
        self.contract()
        with self.assertRaisesRegex(ValueError, 'ports.vh is not bound'):
            replay.copy_peer_sources(self.peer, self.out)
        self.contract(('peer.v', 'ports.vh'))
        (self.peer/'ports.vh').write_text('wire stale_port;\n')
        with self.assertRaisesRegex(ValueError, 'Stale or mixed'):
            replay.copy_peer_sources(self.peer, self.out)

    def test_retired_physical_timing_is_rejected(self):
        self.contract(files=())
        path = self.peer/'stream-contract.json'
        document = json.loads(path.read_text())
        document['controller_hz'] = 99_000_000
        document['cas'] = 2
        path.write_text(json.dumps(document))
        with self.assertRaisesRegex(ValueError, 'contract mismatch'):
            load_contract(self.peer)

    def test_wrong_staged_dut_stops_before_compilation(self):
        (self.out/'dut.v').write_text('old GPU\n')
        execute = Mock()
        with self.assertRaisesRegex(ValueError, 'staged DUT hash mismatch'):
            self.run_simulation(execute)
        execute.assert_not_called()

    def test_changed_export_stops_before_compilation(self):
        self.source.write_text('changed selected export\n')
        execute = Mock()
        with self.assertRaisesRegex(ValueError, 'selected export hash mismatch'):
            self.run_simulation(execute)
        execute.assert_not_called()

    def test_changed_dut_after_compilation_stops_before_simulation(self):
        def compile_only(*_args):
            (self.out/'dut.v').write_text('changed during compilation\n')
            return '', 0
        execute = Mock(side_effect=compile_only)
        with self.assertRaisesRegex(ValueError, 'staged DUT hash mismatch'):
            self.run_simulation(execute)
        self.assertEqual(execute.call_count, 1)

    def test_large_output_stays_in_log_and_only_counters_are_returned(self):
        log = self.out/'large.log'
        code = "print('noise'*10000); print('ACTIVITY covered=4'); print('PASS test')"
        summary, _ = replay.execute_logged([sys.executable, '-c', code], self.out, log, 10)
        self.assertGreater(log.stat().st_size, 40000)
        self.assertEqual(summary.splitlines(), ['ACTIVITY covered=4', 'PASS test'])

    def test_failure_reports_only_a_bounded_tail_and_log_path(self):
        log = self.out/'failure.log'
        code = "print('noise'*10000); print('ERROR deliberate'); raise SystemExit(2)"
        with self.assertRaises(RuntimeError) as error:
            replay.execute_logged([sys.executable, '-c', code], self.out, log, 10)
        self.assertIn(str(log), str(error.exception))
        self.assertIn('ERROR deliberate', str(error.exception))
        self.assertLess(len(str(error.exception)), 4400)

    def test_tiny_simulation_uses_selected_dut_with_contaminated_peer(self):
        compiler = shutil.which(os.environ.get('IVERILOG_EXE', 'iverilog'))
        simulator = shutil.which(os.environ.get('VVP_EXE', 'vvp'))
        if not compiler or not simulator:
            self.skipTest('Icarus Verilog is not configured')
        (self.peer/'dut.v').write_text('old GPU: must never compile\n')
        (self.peer/'tb.v').write_text('old bench: must never compile\n')
        (self.peer/'ports.vh').write_text('// Bound test-specific ports\n')
        self.contract(('ports.vh',))
        sources = replay.copy_peer_sources(self.peer, self.out)
        (self.out/'tb.v').write_text('''
module tb;
 wire value; selected dut(value);
 reg clk=0; always #1 clk=~clk;
 integer cycles=0;
 always @(posedge clk) begin
  cycles=cycles+1;
  if(cycles==2) begin
   if(value!==1'b1) $fatal(1,"wrong selected DUT");
   $display("PASS selected DUT"); $finish;
  end
  if(cycles>=8) $fatal(1,"bounded timeout");
 end
endmodule
''')
        command = [compiler, '-g2012', '-s', 'tb', '-o', self.out/'sim.vvp',
                   self.out/'dut.v', *sources, self.out/'tb.v']
        log, _ = replay.run_checked_simulation(
            replay.execute_logged, command, simulator, self.out, self.source, self.expected, 10)
        self.assertIn('PASS selected DUT', log)


if __name__ == '__main__':
    unittest.main()
