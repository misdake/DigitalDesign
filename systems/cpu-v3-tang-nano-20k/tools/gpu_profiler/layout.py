"""Versioned storage contracts; capture wire/header indices remain driver-owned."""
from dataclasses import dataclass
import struct

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
                    macro_order='row' if self.abi == 1 else 'column',
                    macro_pitch=25 if self.abi == 1 else 16,
                    texture_layout=self.texture_layout)
        if self.abi == 2:
            value['memory_map'] = {k: list(v) for k, v in COLUMN_MAP.items()}
        return value

    def offset(self, x, y):
        if not 0 <= x < 400 or not 0 <= y < 240:
            raise ValueError('Pixel is outside the logical 400x240 surface')
        macro = (y // 16) * 25 + x // 16 if self.abi == 1 else (x // 16) * 16 + y // 16
        return macro * 512 + (y % 16) * 32 + (x % 16) * 2

    def coordinates(self, offset):
        if offset % 2 or not 0 <= offset < self.plane_bytes:
            raise ValueError('Offset is outside the physical word extent')
        macro, local = divmod(offset // 2, 256)
        mx, my = (macro % 25, macro // 25) if self.abi == 1 else (macro // 16, macro % 16)
        x, y = mx * 16 + local % 16, my * 16 + local // 16
        return (x, y) if x < 400 and y < 240 else None

    def words(self, plane):
        if len(plane) != self.plane_bytes:
            raise ValueError('Plane length does not match the declared physical layout')
        return [struct.unpack_from('<H', plane, self.offset(x, y))[0]
                for y in range(240) for x in range(400)]


ROW = Layout(1, 'rgb565-macro16x16-row-pitch25', 'production', 192000, 192000,
             'pot-row-macro8x8')
COLUMN = Layout(2, 'rgb565-macro16x16-column-pad16', 'texture16x8-column16',
                204800, 262144, 'pot-column-macro16x16-min512-cache16x8')


def get_layout(abi):
    if type(abi) is not int or abi not in (1, 2):
        raise ValueError('Unknown capture storage layout ABI')
    return ROW if abi == 1 else COLUMN


def declared_layout(description):
    if not isinstance(description, dict):
        raise ValueError('Explicit layout description is required')
    layout = get_layout(description.get('abi'))
    if description != layout.description():
        raise ValueError('Mixed or unsupported layout description')
    return layout


def header_layout(header):
    # Older in-memory v1 metadata did not retain this derived field.
    layout = get_layout(header.get('layout_abi', 1))
    if header.get('command_abi',1) != 1:
        raise ValueError('Unsupported storage/command combination; paused RTT ABI2 is not pure column')
    if header['plane_bytes'] != layout.plane_bytes:
        raise ValueError('Header plane span does not match its layout ABI')
    if layout.abi == 2:
        if (header['color_base'] not in (0x400000, 0x440000) or header['depth_base'] != 0x480000 or
                header['front_base'] not in (0, 0x400000, 0x440000)):
            raise ValueError('Column target bases disagree with the declared memory map')
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
