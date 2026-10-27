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


def validate_asset_range(address, length):
    bounds = sectors()
    first = bounds["GPU_ASSET_FIRST_SECTOR"] * 128
    end = bounds["GPU_ASSET_END_SECTOR"] * 128
    if not (length > 0 and first <= address < address + length <= end):
        raise ValueError(f"asset {address:#x}+{length:#x} overlaps reserved GPU storage")
