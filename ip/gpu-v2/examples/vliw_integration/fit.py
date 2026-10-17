"""同一注册探针、器件与54MHz约束，拟合当前VLIW集成导出。"""
from pathlib import Path
import argparse
import hashlib
import json
import re
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('placement', type=int, choices=(0, 1))
    args = parser.parse_args()
    source, output = args.source.resolve(), args.output.resolve()
    assert not output.exists(), 'Use a fresh fit directory'
    output.parent.mkdir(parents=True, exist_ok=True)
    text = source.read_text()
    top = re.search(r'^module\s+(\w+)\s*\(', text, re.M)
    assert top, 'Export the composition top first'
    normalized = output.with_name(output.name + '-input.v')
    assert not normalized.exists()
    normalized_text = text[:top.start(1)] + 'gpu_v2_postquad' + text[top.end(1):]
    # The retained probe parser expects whitespace after input/output. Without
    # this lexical normalization it silently omits compact vector declarations.
    normalized_text = re.sub(r'\b(input|output)(?=\[)', r'\1 ', normalized_text)
    normalized.write_text(normalized_text)
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'postquad_ii4'))
    from wrapper import wrapper
    _, boundary = wrapper(normalized_text)
    assert boundary['input_bits'] > 300 and boundary['output_bits'] > 100, 'Incomplete physical boundary'
    fit = Path(__file__).resolve().parents[1] / 'postquad_ii4/fit.py'
    with output.with_name(output.name + '.log').open('w') as log:
        result = subprocess.run([sys.executable, str(fit), str(normalized), str(output), str(args.placement)],
                                stdout=log, stderr=subprocess.STDOUT, timeout=600)
    assert result.returncode == 0, 'See fit log'
    receipt_path = output / 'receipt.json'
    receipt = json.loads(receipt_path.read_text())
    receipt.update(export_top=top[1], export_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                   scope='完整导出顶层的注册探针；包含探针FF，排除CPU、显示和板测。')
    receipt_path.write_text(json.dumps(receipt, indent=2, ensure_ascii=False))
    print(json.dumps({k: receipt[k] for k in ('export_top', 'logic', 'logic_ff', 'ssram', 'bsram',
                                            'primitives', 'fmax', 'setup_violations', 'hold_violations')}))
    assert receipt['setup_violations'] == 0 and receipt['hold_violations'] == 0


if __name__ == '__main__':
    main()
