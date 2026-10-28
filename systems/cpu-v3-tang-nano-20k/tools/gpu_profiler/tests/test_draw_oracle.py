"""Analytic host-only tests; synthetic inputs are never CPU producer evidence."""
from pathlib import Path
from types import SimpleNamespace
import struct
import sys
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import draw_oracle as o
from layout import ROW, COLUMN


class SyntheticCapture:
    def __init__(self, layout=ROW, tint=0xffffffff, rgb=False):
        self.header = dict(layout_abi=layout.abi, command_abi=1, cb_base=0x4c0000,
                           color_base=0x440000, depth_base=0x480000, plane_bytes=layout.plane_bytes,
                           front_base=0x400000, slot=0, cb_padded_bytes=128,
                           object_base=0x4d0000, object_bytes=256, ui_base=0, ui_bytes=0)
        self.memory = {}
        self.ranges = []
        objects = bytearray(256)
        target = (self.header['color_base']//512) | ((self.header['depth_base']//512) << 14)
        struct.pack_into('<I', objects, 0, target)
        struct.pack_into('<3I', objects, 32, 0x4f0000//128, 0, 32 | 8 | 1)
        struct.pack_into('<I', objects, 64, 100*65536)
        struct.pack_into('<4I', objects, 68, 0, 16384 | (8192 << 16),
                         4915 | (10486 << 16), 256)
        matrix = [0]*32
        for at in (0, 5, 10, 12, 16, 20, 21):
            matrix[at] = 65536
        matrix[22] = tint
        struct.pack_into('<32I', objects, 128, *matrix)
        self.put(0x4d0000, objects)
        mesh = bytearray(256)
        struct.pack_into('<4Q', mesh, 0, 3 | (1 << 8) | (20 << 16) | (5 << 24),
                         16 if rgb else 0, 0xffff0000 | (0xffff8000 << 32), 4*65536 | (8 << 32))
        for i, (x, y) in enumerate(((0, 0), (512, 0), (256, 256))):
            attr = (255 << (i*8)) if rgb else 0
            struct.pack_into('<3I', mesh, 128+i*12, x | (y << 10), 511 << 20, attr)
        struct.pack_into('<Q', mesh, 128+40, 0 | (1 << 6) | (2 << 12))
        self.put(0x4e0000, mesh)
        self.put(0x4f0000, b'\xff\xff'*256)
        def dma(source, destination, code):
            return 0x10000000 | ((source//32) << 10) | ((destination//32) << 3) | (code << 1)
        self.commands = [0x40000003, 0x70000000,
                         dma(0x4d0080, 2560, 2), 0x40000001,
                         dma(0x4d0040, 2112, 1), 0x40000001, 0x50000042,
                         dma(0x4d0000, 2176, 1), 0x40000001, 0xa0000044, 0x60000045,
                         0x80001234, 0x8001ffff,
                         0x20000000 | ((0x4e0000//128) << 12), 0x40000001, 0x30000000,
                         0x40000003, 0x70000000, 0xf0000000]
        self.update_commands()
        self.put(self.header['color_base'], b'\x0b\x0b'*(layout.plane_bytes//2))
        self.put(self.header['depth_base'], b'\xff\xff'*(layout.plane_bytes//2))

    def update_commands(self):
        data = struct.pack('<'+'I'*len(self.commands), *self.commands)
        self.header['cb_bytes'] = len(data)
        self.put(self.header['cb_base'], data)

    def put(self, address, data):
        self.memory[address] = bytes(data)
        self.ranges = [SimpleNamespace(address=a, length=len(b)) for a, b in self.memory.items()]

    def read(self, address, length):
        for base, data in self.memory.items():
            if base <= address and address+length <= base+len(data):
                return data[address-base:address-base+length]
        raise ValueError(f'Uncaptured bytes {address:x}+{length:x}')

    def manifest(self):
        import hashlib
        return dict(input_sha256=hashlib.sha256(b''.join(
            address.to_bytes(4, 'little')+data for address, data in sorted(self.memory.items()))).hexdigest())


class DrawOracleTests(unittest.TestCase):
    def test_analytic_constant_depth_and_uniform_color(self):
        result = o.evaluate(SyntheticCapture(tint=0xff4080ff))
        i = 110*400+199
        self.assertTrue(result.stable[i])
        self.assertEqual(result.planes['depth'][i], 1024)
        self.assertEqual(result.planes['color'][i], o.rgb565((255, 128, 64)))
        self.assertEqual(result.planes['color'][0], 0x1234)
        self.assertEqual(result.planes['depth'][0], 65535)
        self.assertGreater(result.caller['drawn_pixel_count'], 3000)
        self.assertEqual(result.caller['draw_commands'], 1)

    def test_row_column_logical_equivalence(self):
        a = o.evaluate(SyntheticCapture(ROW, rgb=True))
        b = o.evaluate(SyntheticCapture(COLUMN, rgb=True))
        self.assertEqual(a.planes, b.planes)
        self.assertEqual(a.stable, b.stable)
        # At (199,110), the screen barycentrics are 0.328125, 0.3203125,
        # 0.3515625. Equal Z makes them also perspective weights.
        self.assertEqual(a.planes['color'][110*400+199], o.rgb565((84, 82, 90)))

    def test_nonboundary_color_and_depth_faults_are_visible(self):
        result = o.evaluate(SyntheticCapture())
        actual = {k: list(v) for k, v in result.planes.items()}
        self.assertTrue(result.compare(actual)['pass'])
        actual['color'][110*400+199] = 0
        actual['depth'][110*400+199] += 9
        report = result.compare(actual)
        self.assertEqual(report['mismatches']['color']['count'], 1)
        self.assertEqual(report['mismatches']['depth']['count'], 1)
        actual['depth'][0] -= 1
        self.assertEqual(result.compare(actual)['mismatches']['depth']['count'], 2)

    def test_perspective_against_hand_solved_weights(self):
        fixture = SyntheticCapture(rgb=True)
        mesh = bytearray(fixture.memory[0x4e0000])
        position, = struct.unpack_from('<I', mesh, 128+24)
        struct.pack_into('<I', mesh, 128+24, position | (512 << 20))
        fixture.put(0x4e0000, mesh)
        result = o.evaluate(fixture)
        # Screen weights at (199,110) are 75/256,73/256,108/256.
        # Divide by vertex Z=(4,4,6) and renormalize: 75/220,73/220,72/220.
        # z=256/55, hence floor(z*256)=1191; RGB nearest=(87,85,83).
        self.assertEqual(result.planes['depth'][110*400+199], 1191)
        self.assertEqual(result.planes['color'][110*400+199], o.rgb565((87, 85, 83)))

    def test_backface_cull_and_two_sided_winding(self):
        fixture = SyntheticCapture()
        expected = o.evaluate(fixture)
        mesh = bytearray(fixture.memory[0x4e0000])
        struct.pack_into('<Q', mesh, 168, (2 << 6) | (1 << 12))
        fixture.put(0x4e0000, mesh)
        self.assertEqual(o.evaluate(fixture).planes, expected.planes)
        q0, = struct.unpack_from('<Q', mesh, 0)
        struct.pack_into('<Q', mesh, 0, q0 | (1 << 31))
        fixture.put(0x4e0000, mesh)
        result = o.evaluate(fixture)
        self.assertEqual(result.caller['drawn_pixel_count'], 0)
        self.assertEqual(result.caller['culled_triangles'], 1)

    def test_unsupported_shader_and_missing_draw_fail_closed(self):
        fixture = SyntheticCapture()
        objects = bytearray(fixture.memory[0x4d0000]); struct.pack_into('<I', objects, 40, 9)
        fixture.put(0x4d0000, objects)
        with self.assertRaisesRegex(ValueError, 'unlit'):
            o.evaluate(fixture)
        fixture = SyntheticCapture()
        fixture.commands.remove(0x30000000); fixture.update_commands()
        with self.assertRaisesRegex(ValueError, 'No real DRAW'):
            o.evaluate(fixture)

    def test_cb_address_reuse_does_not_reuse_reference_outputs(self):
        first = SyntheticCapture(tint=0xff0000ff)
        second = SyntheticCapture(tint=0xff00ff00)
        self.assertEqual(first.header['cb_base'], second.header['cb_base'])
        self.assertEqual(first.memory[first.header['cb_base']], second.memory[second.header['cb_base']])
        a = o.evaluate(first); b = o.evaluate(second); a2 = o.evaluate(first)
        self.assertEqual(a.planes, a2.planes)
        self.assertNotEqual(a.planes['color'], b.planes['color'])
        self.assertFalse(a.compare(b.planes)['pass'])

    def test_three_distinct_leases_with_reused_address(self):
        from producer_sequence import verify_reuse
        fixtures = [SyntheticCapture(tint=tint) for tint in (0xff0000ff, 0xff00ff00, 0xffff0000)]
        for i, fixture in enumerate(fixtures):
            fixture.header.update(token=i+1, submission=i+11)
        middle = fixtures[1]
        middle.header['cb_base'] = 0x7e0000
        middle.put(0x7e0000, middle.memory.pop(0x4c0000))
        report = verify_reuse(fixtures)
        self.assertGreater(report['reused_address_changed_stable_pixels'], 3000)
        fixtures[2].header['token'] = 1
        with self.assertRaisesRegex(ValueError, 'distinct lease'):
            verify_reuse(fixtures)


if __name__ == '__main__':
    unittest.main()
