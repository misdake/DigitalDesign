"""Offline process/ready boundary tests; never open a serial port."""
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import snapshot_cli as cli
from snapshot import Abi, DEFAULT_ABI, SnapshotError


class RecorderTest(unittest.TestCase):
    def test_ready_precedes_operator_notice_and_decode_waits_for_exit(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / 'record'
            events = []
            class Process:
                done = False
                def __init__(self, args, **kwargs):
                    self.args = args
                    events.append('opened')
                    Path(args[args.index('-Out') + 1]).write_bytes(b'raw-placeholder')
                    Path(args[args.index('-ReadyFile') + 1]).write_text('ready')
                def poll(self):
                    return 0 if self.done else None
                def wait(self, timeout):
                    events.append('complete')
                    self.done = True
                    return 0
            def decoded(raw, path, abi):
                self.assertIn('Recorder ready.', stream.getvalue())
                self.assertEqual(events, ['opened', 'complete'])
                return {'attempts': []}
            stream = StringIO()
            with patch.object(cli.subprocess, 'Popen', Process), patch.object(cli, 'decode_file', decoded), redirect_stdout(stream):
                cli.receive_uart('OFFLINE-NO-PORT', 1, 115200, output, Abi(DEFAULT_ABI))

    def test_failed_start_never_announces_ready_or_decodes(self):
        with tempfile.TemporaryDirectory() as temp:
            class Process:
                def poll(self):
                    return 1
            stream = StringIO()
            with patch.object(cli.subprocess, 'Popen', return_value=Process()), patch.object(cli, 'decode_file') as decode, redirect_stdout(stream):
                with self.assertRaisesRegex(SnapshotError, 'before ready'):
                    cli.receive_uart('OFFLINE-NO-PORT', 1, 115200, Path(temp) / 'record', Abi(DEFAULT_ABI))
                decode.assert_not_called()
                self.assertNotIn('Recorder ready.', stream.getvalue())

    def test_startup_timeout_stops_child_without_decoding(self):
        with tempfile.TemporaryDirectory() as temp:
            class Process:
                stopped = False
                def poll(self): return -15 if self.stopped else None
                def terminate(self): self.stopped = True
                def wait(self, timeout): return -15
            process = Process()
            with patch.object(cli.subprocess, 'Popen', return_value=process), patch.object(cli.time, 'monotonic', side_effect=[0,31]), patch.object(cli, 'decode_file') as decode:
                with self.assertRaisesRegex(SnapshotError, '30 seconds'):
                    cli.receive_uart('OFFLINE-NO-PORT', 1, 115200, Path(temp) / 'record', Abi(DEFAULT_ABI))
                self.assertTrue(process.stopped)
                decode.assert_not_called()


if __name__ == '__main__':
    unittest.main()
