"""Synthetic native-ABI3 inputs; not an on-board producer qualification."""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[5]/'ip/gpu-v2/examples/vliw_integration'))
from replay_command import CommandBuffer, memcpy_params
from test_snapshot import fixture
import snapshot as s


def make(abi, command_abi=3):
    header, _, _ = fixture(abi)
    cb = CommandBuffer()
    cb.emit('WAIT', a=3)
    cb.emit('FLUSH')
    cb.emit('DMA', 272, 0x51000, 128)
    cb.emit('WAIT', a=1)
    for _ in range(22): cb.emit('NOP')
    cb.emit('MEMCPY', 272)
    for _ in range(4): cb.emit('NOP')
    cb.emit('MEMCPY', 276)
    cb.emit('MEMCPY', 280)
    cb.emit('END')
    commands = cb.encode()
    jobs = [(0x200000,0x300000,1536),(0x300000,0x430000,512),(0x430000,0x7ffe00,512)]
    original = bytes(((i*73)^(i>>3)^(i>>9)^0xa5)&255 for i in range(1536))
    declarations = [
        ('COMMANDS',abi['READ']|abi['IMMUTABLE'],0x490000,commands+bytes(-len(commands)%128)),
        ('OBJECTS',abi['READ']|abi['IMMUTABLE'],0x51000,b''.join(memcpy_params(*j) for j in jobs)+bytes(32)),
        ('BUFFER',abi['READ']|abi['IMMUTABLE'],0x200000,original),
        ('BUFFER',abi['READ']|abi['WRITE']|abi['PREIMAGE'],0x2ffffe,b'\x69\x69'+bytes([0x3c])*1536+b'\x96\x96'),
        ('BUFFER',abi['READ']|abi['WRITE']|abi['PREIMAGE'],0x7ffe00,bytes([0x96])*512),
        ('COLOR',abi['READ']|abi['WRITE']|abi['PREIMAGE'],0x430000,b'\x34\x12'*96000),
        ('DEPTH',abi['READ']|abi['PREIMAGE'],0x460000,b'\xff\xff'*96000),
        ('FRONT',abi['READ']|abi['PREIMAGE'],0x400000,bytes(192000)),
    ]
    rows, blobs = [], []
    def pair(words, at, value): words[at:at+2] = [value&65535,value>>16]
    for tag, flags, address, blob in declarations:
        row = [0]*abi['RANGE_WORDS']
        row[abi['R_TAG']], row[abi['R_FLAGS']] = abi['TAG_'+tag], flags
        pair(row,abi['R_BASE'],address); pair(row,abi['R_BYTES'],len(blob))
        rows.append(row); blobs.append(blob)
    header[abi['H_COMMAND_ABI']] = command_abi
    header[abi['H_COUNT']] = len(commands)//4-1
    header[abi['H_RANGE_COUNT']] = len(rows)
    pair(header,abi['H_COMMAND_BYTES'],len(commands))
    pair(header,abi['H_COMMAND_PADDED_BYTES'],(len(commands)+127)//128*128)
    pair(header,abi['H_OBJECT_BASE'],0x51000);pair(header,abi['H_OBJECT_BYTES'],128)
    captures, report = s.decode_stream(s.encode_reference(header,rows,blobs,abi),abi)
    assert len(captures)==1 and not report['failures']
    return captures[0], original
