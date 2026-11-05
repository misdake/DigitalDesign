"""Read-only planar goal apertures and baked ordinary-floor material phases."""


def prepare(w, system, output):
    from gpu_texture_layout import pack_levels
    from PIL import Image
    import source_photos as source
    from atlas_borders import extrude

    binary = bytearray()
    for phase in range(4):
        # Build the planar material directly from original photo crops. Each
        # final cell is sampled once; no quantized main atlas is resampled.
        content=Image.new('RGB',(124,124))
        boundaries=[round(i*124/3) for i in range(4)]
        for row in range(3):
            for col in range(3):
                x0,x1=boundaries[col:col+2];y0,y1=boundaries[row:row+2]
                if row==1 and col==1:
                    tile=Image.new('RGB',(x1-x0,y1-y0),(224,88,88))
                else:
                    tile=source.stone((phase+(col-1)*3+(row-1)*5)%4,(x1-x0,y1-y0))
                    def lit(rgb):
                        result=[]
                        for value,tint in zip(rgb,[82,100,121]):
                            value=value*tint+128
                            result.append((((value+(value>>8))>>8)*232)>>8)
                        return tuple(result)
                    tile.putdata([lit(rgb) for rgb in tile.getdata()])
                content.paste(tile,(x0,y0))
        image=extrude(content,128,2)
        pixels = []
        for rgb in image.getdata():
            channels = [(v * maximum + 127) // 255 for v, maximum in zip(rgb, [31, 63, 31])]
            pixels.append((channels[0] << 11) | (channels[1] << 5) | channels[2])
        binary.extend(pack_levels({7: pixels}))
    assert len(binary) == 4 * 32768
    (output / 'goal_carrier.bin').write_bytes(binary)

    carriers, hatches = [], []
    corners = [(-3, -3), (3, -3), (3, 3), (-3, 3)]
    for index in range(48):
        n = round(index * .02 * 128) / 128
        inside = [(-n, -n), (n, -n), (n, n), (-n, n)]
        if index == 0:
            faces = [corners]
        else:
            faces = [[corners[i], corners[(i + 1) % 4], inside[(i + 1) % 4], inside[i]] for i in range(4)]
        quads = [([(x, 0, z) for x, z in face], (0, 511, 0), (0, 0, 1, 1)) for face in faces]
        mesh = w.mesh('goal-carrier-' + str(index), quads, cull_negative=True)
        for vertex in mesh['vertices']:
            xyz = [mesh['config'][axis] + (((vertex[0] >> (axis * 10)) & 1023) << 9) for axis in range(3)]
            uv = [round((2.5 + (xyz[axis] / 65536 + 3) / 6 * 123) * 65535 / 128) for axis in [0, 2]]
            vertex[2] = uv[0] | (uv[1] << 16)
        carriers.append(mesh)
        # Index0 is never a visible hatch; retain a valid nondegenerate record.
        h = max(n, 1 / 128)
        u, v, s, t = w.uv_rect(0)
        f = h / .995
        crop = (1 - f) / 2
        hatches.append(w.mesh('goal-hatch-' + str(index), [([(-h, 0, -h), (h, 0, -h), (h, 0, h), (-h, 0, h)], (0, 511, 0), (u + crop * s, v + crop * t, s * f, t * f))], cull_negative=True))
    return [carriers, hatches]


def applicable(level):
    width, depth = level['width'], level['depth']
    goal = next(i for i, cell in enumerate(level['cells']) if cell[0] == 6)
    gx, gz = goal % width, goal // width
    return all(0 <= gx + dx < width and 0 <= gz + dz < depth and level['cells'][(gz + dz) * width + gx + dx][0] == 1
               for dz in [-1, 0, 1] for dx in [-1, 0, 1] if dx or dz)
