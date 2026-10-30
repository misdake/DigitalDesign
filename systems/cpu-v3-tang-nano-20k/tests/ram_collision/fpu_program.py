"""Fixed legal CPU program and an independent wrapping Q16.16 write oracle."""
import json


def build_program():
    words, writes, registers, memory = [], [], {}, {}

    def immediate(reg, value):
        words.extend([0xF000 | (value >> 4), 0xA300 | (reg << 4) | (value & 15)])

    def auxiliary(fa, fd, subop, mode=0):
        words.extend([0xE100 | (fa << 2), (fd << 10) | (subop << 4) | mode])

    def commit(reg, value):
        value &= 0xFFFFFFFF
        registers[reg] = value
        writes.append((reg, value))

    def signed(value):
        return value if value < 0x80000000 else value - 0x100000000

    def vector(fa, fb, fd, op):
        # Sources are snapshotted before any destination write, including aliases.
        words.extend([0xC000 | (fa << 6) | fb, (fd << 10) | 0x200 | (op << 3)])
        a = [registers[fa + i] for i in range(4)]
        if op == 12:  # VMOV
            values = a
        else:
            b = [registers[fb + (0 if op == 3 else i)] for i in range(4)]
            if op == 0:  # VADD
                values = [x + y for x, y in zip(a, b)]
            else:  # VMUL or broadcast VMULS
                assert op in (2, 3)
                values = [(signed(x) * signed(y)) >> 16 for x, y in zip(a, b)]
        for i, value in enumerate(values):
            commit(fd + i, value)

    def scalar(fa, fb, fd, op):
        words.extend([0xD000 | (fa << 6) | fb, (fd << 10) | (op << 4)])
        a, b = registers[fa], registers[fb]
        assert op in (0, 2)
        commit(fd, a + b if op == 0 else (signed(a) * signed(b)) >> 16)

    for base, fd, values in [
        (0x1000, 0, [0x10000, -0x20000, 0x18000, -0x8000]),
        (0x1020, 8, [0x20000, 0x30000, -0x10000, 0x8000]),
    ]:
        for i, value in enumerate(values):
            memory[base + 2 * i] = value & 0xFFFF
            memory[base + 2 * i + 1] = (value >> 16) & 0xFFFF
        immediate(1, base)
        auxiliary(0, fd, 0, 3)  # FLD4 through the actual CPU data port.
        for i, value in enumerate(values):
            commit(fd + i, value)
    vector(0, 8, 0, 0)
    vector(0, 8, 8, 2)
    scalar(8, 9, 0, 0)
    scalar(0, 10, 0, 2)
    vector(0, 8, 8, 3)  # Destination overwrites the retained broadcast source.
    vector(8, 0, 0, 12)
    scalar(0, 1, 0, 0)
    immediate(1, 0x1100)
    auxiliary(0, 0, 1, 3)  # FST4, then HALT.
    words.append(0x6C00)
    stores = {0x1100 + 2 * i: registers[i] & 0xFFFF for i in range(4)}
    stores.update({0x1101 + 2 * i: registers[i] >> 16 for i in range(4)})
    return dict(program_words=words, memory=memory, expected_writes=writes,
                expected_store=stores, seed="directed-0x434f4c4c")


def render(template, output):
    data = build_program()
    writes = data["expected_writes"]
    init = dict(enumerate(data["program_words"])) | data["memory"]
    replacements = {
        "__LAST_WRITE__": str(len(writes) - 1),
        "__WRITE_COUNT__": str(len(writes)),
        "__PROGRAM_INIT__": "\n".join(
            f"memory[{address}]=16'h{value:04x};" for address, value in init.items()),
        "__ORACLE_INIT__": "\n".join(
            f"gold_address[{i}]=9'd{address};gold_value[{i}]=32'h{value:08x};"
            for i, (address, value) in enumerate(writes)),
        "__STORE_CHECKS__": "\n".join(
            f"if(memory[{address}]!==16'h{value:04x})"
            f'$fatal(1,"AUDIT_LIVE_STORE addr={address} got=%h",memory[{address}]);'
            for address, value in data["expected_store"].items()),
    }
    for marker, value in replacements.items():
        assert marker in template, marker
        template = template.replace(marker, value)
    assert "__" not in template
    (output / "program.json").write_text(json.dumps(data, indent=2) + "\n")
    return template
