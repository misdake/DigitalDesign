"""Matched row/column full-scene replay; canonical pixels plus physical padding guards."""
from pathlib import Path
import hashlib,json,re,struct,sys

sys.dont_write_bytecode=True
HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE))
import texture_layout_replay as base

GUARD=0x5aa5c33c


def offset(canonical,column):
    tile,local=divmod(canonical,512)
    return ((tile%25)*16+tile//25)*512+local if column else canonical


def live(physical,column):
    tile=physical//512
    return tile<400 and tile%16<15 if column else physical<192000


def memory_column(original,texture):
    memory=original(texture)
    row=base_memory16(original,texture)
    packed=bytearray();start=0
    for n in range(7):
        side=max(1,(1<<n)//16);size=side*side*512
        for x in range(side):
            for y in range(side):
                tile=(y*side+x)*512
                packed.extend(row[start+tile:start+tile+512])
        start+=size
    assert len(packed)==100*128
    memory[:16384]=packed+bytes(16384-len(packed))
    return memory


DECLARATIONS=r'''
// LAYOUT_ADAPTER: only this localparam differs between matched observations.
localparam integer COLUMN_MAJOR=@COLUMN@;
integer layout_j,layout_pad_words;reg[31:0]layout_word;
function integer framebuffer_offset;
 input integer canonical;integer tile;
 begin
  tile=canonical/512;
  framebuffer_offset=COLUMN_MAJOR?(((tile%25)*16+tile/25)*512+canonical%512):canonical;
 end
endfunction
function integer physical_live;
 input integer physical;integer tile;
 begin tile=physical/512;physical_live=COLUMN_MAJOR?(tile<400&&tile%16<15):(physical<192000);end
endfunction
'''

PADDING=r'''
 layout_pad_words=0;
 for(layout_j=0;layout_j<262144;layout_j=layout_j+4)if(!physical_live(layout_j))begin
  layout_word=pin.physical_word(native_word(32'h100000+layout_j));
  if(layout_word!==((COLUMN_MAJOR&&layout_j<204800)?32'd0:32'h5aa5c33c))$fatal(1,"color padding changed physical%h value%h",layout_j,layout_word);
  layout_word=pin.physical_word(native_word(32'h140000+layout_j));
  if(layout_word!==((COLUMN_MAJOR&&layout_j<204800)?32'hffffffff:32'h5aa5c33c))$fatal(1,"depth padding changed physical%h value%h",layout_j,layout_word);
  layout_pad_words=layout_pad_words+2;
 end
 $display("PASS PADDING layout=%0d words=%0d",COLUMN_MAJOR,layout_pad_words);
'''


def main():
    global base_memory16
    column='--column-major' in sys.argv
    if column:sys.argv.remove('--column-major')
    assert '--layout' in sys.argv and sys.argv[sys.argv.index('--layout')+1]=='16x8'
    harness=Path(__file__).read_bytes()
    base_memory16=base.memory16
    if column:base.memory16=memory_column
    original_bench=base.command.command_bench
    def bench(text,bunny,metas,out):
        text=original_bench(text,bunny,metas,out)
        (out/'layout-harness.py').write_bytes(harness)
        seed={int(a,16):int(v,16) for a,v in (line.split() for line in (out/'seed.txt').read_text().splitlines())}
        mapped={}
        for a,v in seed.items():
            for plane in (0x100000,0x140000):
                if plane<=a<plane+192000:
                    a=plane+offset(a-plane,column);break
            assert a not in mapped
            mapped[a]=v
        for plane in (0x100000,0x140000):
            for off in range(0,262144,4):
                if not live(off,column):
                    assert plane+off not in mapped
                    mapped[plane+off]=GUARD
        (out/'seed.txt').write_text(''.join(f'{a:08x} {v:08x}\n' for a,v in sorted(mapped.items())))
        text=text.replace('module tb;','module tb;\n'+DECLARATIONS.replace('@COLUMN@',str(int(column))),1)
        old="if(j<TEX)raw_seed_addr=32'h1000+j;else if(j<TEX+FB)raw_seed_addr=32'h100000+j-TEX;else raw_seed_addr=32'h140000+j-TEX-FB;"
        new="if(j<TEX)raw_seed_addr=32'h1000+j;else if(j<TEX+FB)raw_seed_addr=32'h100000+framebuffer_offset(j-TEX);else raw_seed_addr=32'h140000+framebuffer_offset(j-TEX-FB);"
        assert text.count(old)==1;text=text.replace(old,new)
        marker=' end $fclose(ff);'
        assert text.count(marker)==1;text=text.replace(marker,marker+PADDING)
        return text
    base.command.command_bench=bench
    out=Path(sys.argv[sys.argv.index('--output')+1]).resolve()
    base.main()
    counts_path=out/'texture-counts.json';counts=json.loads(counts_path.read_text())
    log=(out/'run.log').read_text();assert f'PASS PADDING layout={int(column)}' in log
    text=(out/'tb.v').read_text();normalized=re.sub(r'localparam integer COLUMN_MAJOR=[01];','localparam integer COLUMN_MAJOR=LAYOUT;',text)
    normalized_readback=re.sub(r' layout_pad_words=0;.*?\$display\("PASS PADDING.*?;\n','',normalized,flags=re.S)
    normalized_readback='\n'.join(line.strip() for line in normalized_readback.splitlines() if line.strip())
    counts.update(layout='16x8-column16' if column else '16x8-row',
        layout_identity='rgb565-macro16x16-column-major-pad16-cache16x8-8KiB' if column else 'rgb565-macro16x16-row-major-pitch25-cache16x8-8KiB',
        framebuffer_dump='Canonical row-major16x16 logical pixels; physical padded tiles must contain CLEAR values0/ffff, allocation slack must retain seeded guards.',
        boundary='Full GPU with the same real MC/pin model at approximately49.5/99MHz, CL2 as emitted by the frozen TB; no CPU/Display contention or board-frequency claim.',
        normalized_tb_without_padding_guards_sha256=hashlib.sha256(normalized_readback.encode()).hexdigest(),
        framebuffer_allocation_bytes=262144,framebuffer_layout_bytes=204800 if column else 192000,
        layout_harness_sha256=hashlib.sha256(harness).hexdigest(),
        normalized_tb_sha256=hashlib.sha256(normalized.encode()).hexdigest(),
        padding_summary=next(l for l in log.splitlines() if l.startswith('PASS PADDING')))
    counts_path.write_text(json.dumps(counts,indent=2))
    print(json.dumps({k:counts[k] for k in ('layout_identity','counts','padding_summary','normalized_tb_sha256')},indent=2))


if __name__=='__main__':main()
