"""Check command-only uniform RGBA sideband without changing V/T math."""
import importlib.util
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest

ROOT = Path(__file__).resolve().parents[3]
PATH = ROOT/'ip/gpu-v2/examples/vliw_integration/transport.py'
spec = importlib.util.spec_from_file_location('command_transport', PATH)
transport = importlib.util.module_from_spec(spec)
spec.loader.exec_module(transport)


class ColorTransport(unittest.TestCase):
    def test_command_and_legacy_packets(self):
        for enabled, rgba in [(False, [17, 34, 51, 68]),
                              (True, [17, 34, 51, 68]), (True, None)]:
            with self.subTest(enabled=enabled, rgba=rgba), TemporaryDirectory() as temp:
                out = Path(temp)
                coefficients = list(range(10))
                attributes = [0x12345678, 0x87654321, 0x01020304]
                old = coefficients + [v for a in attributes for v in (0, 0, a)]
                marker = object()
                def prepare(instance, directory):
                    (Path(directory)/'vertices.hex').write_text('0\n'*6)
                    return {'marker': marker}, [{'words': old, 'triangle': marker}]
                oracle = SimpleNamespace(prepare=prepare,
                    M=SimpleNamespace(normal12_from18=lambda value: value))
                transport.adapt_oracle(oracle, mesh_uniform_color=enabled)
                instance = {'config': [0, 0, 0, 256]}
                if rgba is not None:
                    instance['mesh_uniform_color'] = rgba
                meta, packets = oracle.prepare(instance, out)
                words = packets[0]['words']
                self.assertIs(meta['marker'], marker)
                self.assertIs(packets[0]['triangle'], marker)
                self.assertEqual(list(words[:10]), coefficients)
                self.assertEqual([v & ((1 << 36)-1) for v in words[10:]],
                                 [v for a in attributes for v in (0, a)])
                color = int.from_bytes(bytes(rgba or [255]*4), 'little') if enabled else 0
                self.assertEqual(words[10] >> 36, color & ((1 << 18)-1))
                self.assertEqual(words[11] >> 36, color >> 18)
                self.assertTrue(all(v >> 36 == 0 for v in words[12:]))
                self.assertEqual([int(v, 16) for v in (out/'words.hex').read_text().splitlines()],
                                 list(words))


if __name__ == '__main__':
    unittest.main()
