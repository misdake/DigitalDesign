"""Bounded column storage/MC fixture; never a real producer or board receipt."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from capture import run as capture_run
from identity import digest
from layout import COLUMN
from replay import execute, physical, prepare
import snapshot as s
from test_snapshot import fixture
from test_layout import oracle_plane


def stage_column(row_export, frozen, contract, output):
    declaration=json.loads(contract.read_text())
    if (declaration['hardware_profile'] != COLUMN.profile or
            declaration['gpu_export_sha256'] != digest(frozen) or
            declaration['framebuffer']['physical_bytes'] != COLUMN.plane_bytes):
        raise ValueError('Frozen GPU bytes do not match the explicitly supplied column contract')
    output.mkdir(parents=True,exist_ok=False)
    sources=json.loads((row_export/'rtl.json').read_text()); selected=[]; replaced=0
    for name in sources:
        source=row_export/name
        if 'module gpu_v2_vliw_command' in source.read_text():
            replaced+=1
            destination=output/'gpu-column-frozen.v'; shutil.copyfile(frozen,destination)
            selected.append(destination.name)
        else:
            destination=output/name; destination.parent.mkdir(parents=True,exist_ok=True)
            shutil.copyfile(source,destination); selected.append(name)
    if replaced != 1: raise ValueError('Expected one explicit GPU export replacement')
    peer=json.loads((row_export/'peer/stream-contract.json').read_text())
    for name in [*peer['files'],'stream-contract.json']:
        destination=output/'peer'/name; destination.parent.mkdir(parents=True,exist_ok=True)
        shutil.copyfile(row_export/'peer'/name,destination)
    (output/'rtl.json').write_text(json.dumps(selected))
    (output/'layout.json').write_text(json.dumps(dict(schema=1,layout_abi=2,gpu_profile=COLUMN.profile)))
    (output/'fixture-source.json').write_text(json.dumps(dict(
        scope='Explicit frozen column GPU plus current 54/108 CL3 stream peer; no firmware or board qualification',
        frozen_gpu_sha256=digest(frozen), contract_sha256=digest(contract),
        original_gpu=str(frozen.resolve()), current_peer_sha256=digest(row_export/'peer/stream-contract.json')),
        indent=2)+'\n')


def column_capture(abi, clear=False, guards=True):
    header,_,_=fixture(abi)
    header[abi['H_LAYOUT_ABI']]=2
    commands=[0,0x40000003,0x70000000,0xf0000000]
    if clear:
        dma=0x10000000|((0x4d0000//32)<<10)|((276//4)<<3)
        commands=[0x40000003,0x70000000,dma,0x40000001,0xa0000045,0x80001234,0x8001abcd,
                  0x40000003,0x70000000,0xf0000000]
    for name,value in [('COMMAND_BASE',0x4c0000),('COMMAND_BYTES',len(commands)*4),
                       ('COMMAND_PADDED_BYTES',128),('COLOR_BASE',0x440000),('DEPTH_BASE',0x480000),
                       ('PLANE_BYTES',204800),('OBJECT_BASE',0x4d0000 if clear else 0),
                       ('OBJECT_BYTES',32 if clear else 0)]:
        at=abi['H_'+name]; header[at:at+2]=[value&65535,value>>16]
    header[abi['H_COUNT']]=len(commands)-1
    rows,blobs=[],[]
    def add(tag,address,data,flags):
        row=[0]*8; row[abi['R_FLAGS']]=flags; row[abi['R_TAG']]=abi['TAG_'+tag]
        for name,value in [('BASE',address),('BYTES',len(data)),('GENERATION',1)]:
            at=abi['R_'+name]; row[at:at+2]=[value&65535,value>>16]
        rows.append(row); blobs.append(data)
    add('COMMANDS',0x4c0000,struct.pack('<'+'I'*len(commands),*commands)+bytes(128-len(commands)*4),abi['READ'])
    if clear:
        target=(0x440000//512)|((0x480000//512)<<14)
        add('OBJECTS',0x4d0000,struct.pack('<8I',target,*([0]*7)),abi['READ'])
    for tag,base,depth in [('COLOR',0x440000,False),('DEPTH',0x480000,True),('FRONT',0x400000,False)]:
        flags=abi['PREIMAGE']|abi['READ']
        if tag!='FRONT': flags|=abi['WRITE']
        if clear and tag!='FRONT': flags|=abi['FULL_CLEAR']
        add(tag,base,oracle_plane(depth),flags)
        if guards: add(tag,base+204800,bytes([0xa5])*57344,abi['PREIMAGE']|abi['READ'])
    header[abi['H_RANGE_COUNT']]=len(rows)
    captures,report=s.decode_stream(s.encode_reference(header,rows,blobs,abi),abi)
    if len(captures)!=1 or report['failures']: raise ValueError(report)
    return captures[0]


def main():
    p=argparse.ArgumentParser(__doc__)
    p.add_argument('--row-rtl',type=Path,required=True)
    p.add_argument('--frozen-column',type=Path,required=True)
    p.add_argument('--column-contract',type=Path,required=True)
    p.add_argument('--out',type=Path,required=True)
    p.add_argument('--clear',action='store_true')
    p.add_argument('--missing-read',action='store_true')
    p.add_argument('--forbidden-write',action='store_true')
    p.add_argument('--physical-only',action='store_true',help='Omit the unreferenced allocation tails')
    args=p.parse_args(); args.out.mkdir(parents=True,exist_ok=False)
    rtl=args.out/'rtl'; stage_column(args.row_rtl,args.frozen_column,args.column_contract,rtl)
    capture=column_capture(s.Abi(),args.clear or args.forbidden_write,not args.physical_only)
    capture.save(args.out/'snapshot')
    span=204800 if args.physical_only else 262144
    manifest=prepare(capture,rtl,args.out/'replay',max_cycles=180000,timeout=180,
                     preflight_result=dict(fixture_only=True,captured_plane_bytes=span))
    if args.missing_read or args.forbidden_write:
        guard_file='valid.hex' if args.missing_read else 'writable.hex'
        address=0x4c0000 if args.missing_read else 0x440000
        mask='e' if args.missing_read else '0'
        with (manifest.parent/guard_file).open('a') as stream:
            stream.write(f'@{physical(address//4):x}\n{mask}\n')
        try: capture_run(manifest,manifest.parent/'capture')
        except RuntimeError:
            log=(manifest.parent/'capture/run.log').read_text(errors='replace')
            required='Uncaptured/uninitialized read' if args.missing_read else 'Write outside the captured writable extent'
            if required not in log: raise
            (args.out/'result.json').write_text(json.dumps(dict(
                missing_read_rejected=args.missing_read,forbidden_write_rejected=args.forbidden_write,board=False)))
            print('PASS guarded negative: '+required); return
        raise AssertionError('Invalid input/write unexpectedly accepted')
    result=execute(capture,manifest)
    for role,depth,value in [('color',False,0x1234),('depth',True,0xabcd)]:
        raw=(manifest.parent/'capture'/(role+'.bin')).read_bytes()
        expected=struct.pack('<H',value)*102400 if args.clear else oracle_plane(depth)
        if raw != expected: raise AssertionError(role+' physical image mismatch')
        expected_logical=([value]*96000 if args.clear else [
            (((y*400+x)*17+12345)&65535 if depth else
             ((x+y)%32)*2048+((x*3+y*5)%64)*32+(x*7+y)%32)
            for y in range(240) for x in range(400)])
        if COLUMN.words(raw)!=expected_logical: raise AssertionError(role+' independent logical oracle mismatch')
        allocation=(manifest.parent/'capture'/(role+'-allocation.bin')).read_bytes()
        if allocation[204800:]!=bytes([0xa5])*(span-204800): raise AssertionError(role+' allocation guard changed')
    (args.out/'result.json').write_text(json.dumps(dict(
        scope='Synthetic snapshot/native CL3 pin replay only; no real producer or board qualification',
        clear=args.clear,layout=COLUMN.description(),replay=result,logical_color_pixels_checked=96000,
        logical_depth_pixels_checked=96000,padding_words_per_plane=6400,guard_bytes_per_plane=span-204800),indent=2)+'\n')
    print('PASS column physical/pixel/padding/guard oracle; clear='+str(args.clear))


if __name__=='__main__': main()
