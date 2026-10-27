"""Manifest layout descriptions derived from the selected system allocation source."""
from pathlib import Path
import re
from gpu_layout import sectors


def description(profile='default'):
    b=sectors(profile)
    column=profile=='column16'
    abi_source=Path(__file__).resolve().parents[1]/'rcc/gpu_capture_abi.rs'
    ids={k:int(v) for k,v in re.findall(r'pub const (GPU_CAPTURE_LAYOUT_\w+_ABI): u16 = ([0-9]+);',abi_source.read_text())}
    value=dict(abi=ids['GPU_CAPTURE_LAYOUT_COLUMN_ABI' if column else 'GPU_CAPTURE_LAYOUT_ROW_ABI'],
        name='rgb565-macro16x16-column-pad16' if column else 'rgb565-macro16x16-row-pitch25',
        gpu_profile='texture16x8-column16' if column else 'production',
        width=400,height=240,logical_pixels=96000,physical_bytes=b['GPU_FRAMEBUFFER_PHYSICAL_SECTORS']*128,
        allocation_bytes=b['GPU_PLANE_SECTORS']*128 if column else b['GPU_FRAMEBUFFER_PHYSICAL_SECTORS']*128,
        pixel_bytes=2,macro_width=16,macro_height=16,macro_order='column' if column else 'row',macro_pitch=16 if column else 25,
        texture_layout='pot-column-macro16x16-min512-cache16x8' if column else 'pot-row-macro8x8')
    if column:
        starts=[b[k]*128 for k in ['GPU_COLOR_A_SECTOR','GPU_COLOR_B_SECTOR','GPU_DEPTH_SECTOR','GPU_COMMAND_FIRST_SECTOR','GPU_OBJECT_FIRST_SECTOR','GPU_ASSET_FIRST_SECTOR','GPU_UI_PAGE0_SECTOR','GPU_UI_PAGE1_SECTOR','GPU_SLOT1_COMMAND_FIRST_SECTOR','GPU_SLOT1_OBJECT_FIRST_SECTOR']]
        names=['colorA','colorB','depth','slot0CB','slot0obj','assets','UI0','UI1','slot1CB','slot1obj']
        value['memory_map']={n:[a,z] for n,a,z in zip(names,starts,starts[1:]+[0x800000])}
    return value
