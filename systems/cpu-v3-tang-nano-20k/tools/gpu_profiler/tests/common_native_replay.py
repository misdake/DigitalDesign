"""Finite real GPU+MC replay of the saved tiny RGB fixture, no application run."""
import json
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from test_snapshot import fixture
import snapshot as s
import replay


def main():
    source,rtl,out=map(Path,sys.argv[1:4]);out.mkdir(parents=True,exist_ok=False)
    backing=source.read_bytes();assert len(backing)==8388608
    abi=s.Abi(s.DEFAULT_ABI);header,_,_=fixture(abi)
    header[abi['H_VERSION']]=abi['EXPLICIT_VERSION']
    header[abi['H_FLAGS']]=abi['PUBLISHED']|abi['HAS_TARGET']|abi['HAS_FRONT']
    header[abi['H_FRAME']]=0
    def pair(name,value):
        at=abi['H_'+name];header[at:at+2]=[value&65535,value>>16]
    for name,value in [('COMMAND_BASE',0x40000),('COMMAND_BYTES',60),('COMMAND_PADDED_BYTES',128),
                       ('COLOR_BASE',0x100000),('DEPTH_BASE',0x140000)]:pair(name,value)
    header[abi['H_COUNT']]=14
    rows=[];blobs=[]
    def region(tag,address,length,flags):
        row=[0]*abi['RANGE_WORDS'];row[abi['R_TAG']]=abi['TAG_'+tag];row[abi['R_FLAGS']]=flags
        for name,value in [('BASE',address),('BYTES',length),('GENERATION',1)]:
            at=abi['R_'+name];row[at:at+2]=[value&65535,value>>16]
        rows.append(row);blobs.append(backing[address:address+length])
    region('COMMANDS',0x40000,128,abi['READ'])
    region('OBJECTS',0x20000,256,abi['READ'])
    region('OBJECTS',0x21000,256,abi['READ'])
    region('MESH',0x22000,2048,abi['READ']|abi['IMMUTABLE'])
    for tag,base in [('COLOR',0x100000),('DEPTH',0x140000)]:
        region(tag,base,204800,abi['READ']|abi['WRITE']|abi['PREIMAGE'])
        region(tag,base+204800,262144-204800,abi['READ']|abi['PREIMAGE'])
    region('FRONT',0x400000,262144,abi['READ']|abi['PREIMAGE'])
    header[abi['H_RANGE_COUNT']]=len(rows)
    captures,status=s.decode_stream(s.encode_reference(header,rows,blobs,abi),abi)
    assert not status['failures'],status
    capture=captures[0]
    manifest=replay.prepare(capture,rtl,out/'replay',200000,60)
    result=replay.execute(capture,manifest,require_images=True)
    print(json.dumps(result))


if __name__=='__main__':main()
