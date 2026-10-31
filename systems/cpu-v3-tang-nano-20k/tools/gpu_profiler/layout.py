"""Versioned storage contracts; capture wire/header indices remain driver-owned."""
from dataclasses import dataclass
import struct
import re
from pathlib import Path

COMMAND_ABI = int(re.search(r"pub const GPU_CAPTURE_RTT400_COMMAND_ABI: u16 = ([0-9]+);",
    (Path(__file__).resolve().parents[2] / "rcc/gpu_capture_abi.rs").read_text()).group(1))

COLUMN_MAP = dict(colorA=(0x400000, 0x440000), colorB=(0x440000, 0x480000),
                  depth=(0x480000, 0x4c0000), slot0CB=(0x4c0000, 0x4d0000),
                  slot0obj=(0x4d0000, 0x4e0000), assets=(0x4e0000, 0x7df800),
                  UI0=(0x7df800, 0x7dfc00), UI1=(0x7dfc00, 0x7e0000),
                  slot1CB=(0x7e0000, 0x7f0000), slot1obj=(0x7f0000, 0x800000))


@dataclass(frozen=True)
class Layout:
    abi: int
    name: str
    profile: str
    plane_bytes: int
    allocation_bytes: int
    texture_layout: str

    def description(self):
        value = dict(abi=self.abi, name=self.name, gpu_profile=self.profile,
                    width=400, height=240, logical_pixels=96000,
                    physical_bytes=self.plane_bytes, allocation_bytes=self.allocation_bytes,
                    pixel_bytes=2, macro_width=16, macro_height=16,
                    macro_order='column',
                    macro_pitch=16,
                    texture_layout=self.texture_layout)
        value['memory_map'] = {k: list(v) for k, v in COLUMN_MAP.items()}
        return value

    def offset(self, x, y):
        if not 0 <= x < 400 or not 0 <= y < 240:
            raise ValueError('Pixel is outside the logical 400x240 surface')
        macro = (x // 16) * 16 + y // 16
        return macro * 512 + (y % 16) * 32 + (x % 16) * 2

    def recorded_description(self):
        """Same physical packing, without legacy allocation or address policy."""
        value = self.description()
        del value['memory_map']
        del value['allocation_bytes']
        value['address_policy'] = 'record-ranges-v1'
        return value

    def coordinates(self, offset):
        if offset % 2 or not 0 <= offset < self.plane_bytes:
            raise ValueError('Offset is outside the physical word extent')
        macro, local = divmod(offset // 2, 256)
        mx, my = (macro // 16, macro % 16)
        x, y = mx * 16 + local % 16, my * 16 + local // 16
        return (x, y) if x < 400 and y < 240 else None

    def words(self, plane):
        if len(plane) != self.plane_bytes:
            raise ValueError('Plane length does not match the declared physical layout')
        return [struct.unpack_from('<H', plane, self.offset(x, y))[0]
                for y in range(240) for x in range(400)]


COLUMN = Layout(2, 'rgb565-macro16x16-column-pad16', 'production',
                204800, 262144, 'pot-column-macro16x16-min512-cache16x8')


def get_layout(abi):
    if type(abi) is not int or abi != COLUMN.abi:
        raise ValueError('Production requires explicit column storage ABI2; row captures are unsupported')
    return COLUMN


def declared_layout(description):
    if not isinstance(description, dict):
        raise ValueError('Explicit layout description is required')
    layout = get_layout(description.get('abi'))
    if description not in (layout.description(), layout.recorded_description()):
        raise ValueError('Mixed or unsupported layout description')
    return layout


def header_layout(header):
    layout = get_layout(header.get('layout_abi'))
    if header.get('command_abi') != COMMAND_ABI:
        raise ValueError('Unsupported production column/RTT command ABI')
    recorded = header.get('capture_version', 1) == 2
    has_target = header.get('has_target', True) if recorded else True
    if header['plane_bytes'] != (layout.plane_bytes if has_target else 0):
        raise ValueError('Header plane span does not match its layout ABI')
    if recorded:
        # Header validation supplies presence and physical bounds. Allocation
        # extents belong to the range table, never the old slot/asset map.
        return layout
    if layout.abi == 2:
        color, depth = header['color_base'], header['depth_base']
        fixed = color in (0x400000, 0x440000) and depth == 0x480000
        lo, hi = COLUMN_MAP['assets']
        borrowed = all(base % 65536 == 0 and lo <= base < base + layout.allocation_bytes <= hi
                       for base in (color, depth)) and abs(color-depth) >= layout.allocation_bytes
        if not (fixed or borrowed) or header['front_base'] not in (0, 0x400000, 0x440000):
            raise ValueError('Column target bases disagree with fixed or borrowed plane ownership')
        slot = header['slot']
        for role, base, size in [('CB', header['cb_base'], header['cb_padded_bytes']),
                                 ('obj', header['object_base'], header['object_bytes'])]:
            lo, hi = COLUMN_MAP[f'slot{slot}{role}']
            if size and not lo <= base < base + size <= hi:
                raise ValueError('Column input allocation exceeds its slot memory map')
        if header['ui_bytes'] and not any(lo <= header['ui_base'] < header['ui_base'] + header['ui_bytes'] <= hi
                                        for lo, hi in (COLUMN_MAP['UI0'], COLUMN_MAP['UI1'])):
            raise ValueError('Column UI range exceeds its declared page')
    return layout


def capture_description(header):
    layout = header_layout(header)
    return layout.recorded_description() if header.get('capture_version', 1) == 2 else layout.description()
