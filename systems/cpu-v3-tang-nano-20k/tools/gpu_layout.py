"""Read the system's sole GPU layout authority; do not duplicate pool bounds."""
import ast
import operator
from pathlib import Path
import re

LAYOUT = Path(__file__).resolve().parents[1] / "rcc/gpu_layout.rs"


def sectors():
    values = {}
    operations = {ast.Add: operator.add, ast.Sub: operator.sub}

    def value(node):
        if isinstance(node, ast.Constant) and type(node.value) is int:
            return node.value
        if isinstance(node, ast.Name):
            return values[node.id]
        if isinstance(node, ast.BinOp) and type(node.op) in operations:
            return operations[type(node.op)](value(node.left), value(node.right))
        raise ValueError("unsupported GPU layout expression")

    for name, expression in re.findall(r"pub const (\w+): u16\s*=\s*([^;]+);", LAYOUT.read_text()):
        result = value(ast.parse(expression.strip(), mode="eval").body)
        if not 0 <= result <= 65535:
            raise ValueError(f"GPU sector constant does not fit u16: {name}")
        values[name] = result
    return values


def asset_regions():
    """Prefer the original high bank; each allocation must fit one whole bank."""
    bounds = sectors()
    return [(bounds[first] * 128, bounds[end] * 128) for first, end in [
        ('GPU_ASSET_FIRST_SECTOR', 'GPU_ASSET_END_SECTOR'),
        ('GPU_ASSET_LOW_FIRST_SECTOR', 'GPU_ASSET_LOW_END_SECTOR')]]


def validate_asset_range(address, length):
    if not any(length > 0 and first <= address < address + length <= end
               for first, end in asset_regions()):
        raise ValueError(f"asset {address:#x}+{length:#x} overlaps reserved GPU storage")


def description():
    b=sectors()
    value=dict(abi=2,
        name='rgb565-macro16x16-column-pad16',
        gpu_profile='production',
        width=400,height=240,logical_pixels=96000,physical_bytes=b['GPU_FRAMEBUFFER_PHYSICAL_SECTORS']*128,
        allocation_bytes=b['GPU_PLANE_SECTORS']*128,
        pixel_bytes=2,macro_width=16,macro_height=16,macro_order='column',macro_pitch=16,
        texture_layout='pot-column-macro16x16-min512-cache16x8')
    starts=[b[k]*128 for k in ['GPU_COLOR_A_SECTOR','GPU_COLOR_B_SECTOR','GPU_DEPTH_SECTOR','GPU_COMMAND_FIRST_SECTOR','GPU_OBJECT_FIRST_SECTOR','GPU_ASSET_FIRST_SECTOR','GPU_UI_PAGE0_SECTOR','GPU_UI_PAGE1_SECTOR','GPU_SLOT1_COMMAND_FIRST_SECTOR','GPU_SLOT1_OBJECT_FIRST_SECTOR']]
    names=['colorA','colorB','depth','slot0CB','slot0obj','assets','UI0','UI1','slot1CB','slot1obj']
    value['memory_map']={n:[a,z] for n,a,z in zip(names,starts,starts[1:]+[0x800000])}
    return value
