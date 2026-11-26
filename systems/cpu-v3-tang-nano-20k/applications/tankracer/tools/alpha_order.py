"""View-direction priors for exact runtime transparent depth sorting."""
import math
import struct

YAW_SECTORS = 16
PITCH_DEGREES = (-60, -30, -10, -1, 0, 1, 10, 30, 60)


def build(packed, opaque_count):
    total = len(packed)//11
    count = total-opaque_count
    stride = max(64, ((count+63)//64)*64)
    centers = {}
    for at in range(0, len(packed), 11):
        identity = packed[at+10]
        if identity >= opaque_count:
            centers[identity] = [(v-65536 if v & 32768 else v)*512 for v in packed[at:at+3]]
    if sorted(centers) != list(range(opaque_count, total)):
        raise ValueError('alpha rank input must preserve contiguous stable IDs')
    tables = []
    for yaw in range(YAW_SECTORS):
        angle = yaw*2*math.pi/YAW_SECTORS
        for degrees in PITCH_DEGREES:
            pitch = math.radians(degrees)
            axis = [round(v*65536) for v in (math.sin(angle)*math.cos(pitch), math.sin(pitch), math.cos(angle)*math.cos(pitch))]
            ordered = sorted(centers, key=lambda i: (sum(a*b for a, b in zip(axis, centers[i])) >> 16, i))
            ranks = [65535]*stride
            for rank, identity in enumerate(ordered):
                ranks[identity-opaque_count] = rank
            tables.extend(ranks)
    thresholds = []
    for a, b in zip(PITCH_DEGREES, PITCH_DEGREES[1:]):
        raw = round((math.sin(math.radians(a))+math.sin(math.radians(b)))*32768)
        thresholds.extend((raw & 65535, (raw >> 16) & 65535))
    return struct.pack(f'<{len(tables)}H', *tables), stride//64, thresholds
