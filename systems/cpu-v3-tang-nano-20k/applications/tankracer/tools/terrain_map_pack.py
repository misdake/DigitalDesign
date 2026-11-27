"""Place complete collision cells at sector boundaries inside resident pages."""
import json
import struct


def pack_map(raw, output):
    if len(raw) != 64 * 32768:
        raise ValueError('64 complete source cell pages required')
    cells = []
    for cell in range(64):
        source = raw[cell * 32768:(cell + 1) * 32768]
        magic, version, identity, words = struct.unpack_from('<4H', source)
        if magic != 0x5443 or version != 1 or identity != cell or not 16 <= words <= 16384:
            raise ValueError(f'invalid source cell {cell}')
        data = source[:words * 2]
        cells.append(dict(cell=cell, data=data, size=(len(data) + 127) // 128 * 128))
    pages = []
    directory = [65535] * 64
    rows = []
    for row in sorted(cells, key=lambda row: (-row['size'], row['cell'])):
        page = next((page for page in pages if page['used'] + row['size'] <= 32768), None)
        if page is None:
            page = dict(index=len(pages), used=0, data=bytearray(32768))
            pages.append(page)
        offset = page['used']
        sector = page['index'] * 256 + offset // 128
        if sector >= 32767:
            raise ValueError('collision sector directory overflow')
        directory[row['cell']] = 32768 | sector
        page['data'][offset:offset + len(row['data'])] = row['data']
        rows.append(dict(cell=row['cell'], page=page['index'], sector=sector,
                         word_offset=offset // 2, bytes=len(row['data']), reserved_bytes=row['size']))
        page['used'] += row['size']
    packed = b''.join(page['data'] for page in pages)
    for row in cells:
        offset = (directory[row['cell']] & 32767) * 128
        if packed[offset:offset + len(row['data'])] != row['data']:
            raise ValueError(f'collision data changed: {row["cell"]}')
    (output / 'cells.bin').write_bytes(packed)
    (output / 'directory.bin').write_bytes(struct.pack('<64H', *directory))
    report = dict(version=1, cells=sorted(rows, key=lambda row: row['cell']),
                  used_bytes=sum(len(row['data']) for row in cells),
                  packed_bytes=len(packed), page_count=len(pages), page_bytes=32768,
                  directory_bytes=128, alignment_bytes=128,
                  scope='All source cells, preserving every used byte and original local offset. '
                        'Each cell is contained in one resident page; runtime queries directly map it.')
    (output / 'terrain-map.json').write_text(json.dumps(report, indent=2) + '\n')
    return report
