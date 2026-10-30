"""Find a generated module instance for canonical native replay observers."""
import re


def hierarchy(source, target):
    modules = dict(re.findall(r'\bmodule\s+(\w+)\b(.*?)\bendmodule\b', source, re.S))
    def walk(name, prefix, seen):
        if name == target:
            return prefix
        if name in seen:
            return None
        body = modules[name]
        for child in modules:
            match = re.search(r'\b'+re.escape(child)+r'\s*(?:#\s*\([^;]*?\))?\s+(\w+)\s*\(', body, re.S)
            if match:
                result = walk(child, prefix+'.'+match[1], seen | {name})
                if result:
                    return result
        return None
    result = walk('gpu_v2_vliw_command', 'dut', set())
    assert result, target
    return result
