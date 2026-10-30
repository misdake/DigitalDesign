"""Native ABI3/RTT4 raw-copy contract and bounded, mutable reference memory."""
import struct

MEMORY_BYTES = 8 * 1024 * 1024


def descriptor(command, scratch, command_abi):
    if command_abi not in (3, 4):
        raise ValueError('MEMCPY requires native command ABI3 or RTT4')
    slot = command & 127
    if command >> 28 != 12 or command & 0x0fffff80 or not 66 <= slot < 80:
        raise ValueError('Invalid MEMCPY command/staging slot')
    value, = struct.unpack_from('<Q', scratch, slot*32)
    source = (value & 0x3fff)*512
    destination = ((value >> 14) & 0x3fff)*512
    size = (((value >> 28) & 0x7ff)+1)*512
    if value >> 39 or source+size > MEMORY_BYTES or destination+size > MEMORY_BYTES:
        raise ValueError('Invalid MEMCPY reserved bits/extent')
    if source < destination+size and destination < source+size:
        raise ValueError('Overlapping MEMCPY extents')
    return source, destination, size


class MemoryImage:
    """A write becomes a later command's input; frozen bytes are not reread."""
    def __init__(self, capture):
        self.data = bytearray(MEMORY_BYTES)
        self.valid = bytearray(MEMORY_BYTES)
        self.writable = bytearray(MEMORY_BYTES)
        self.abi = capture.abi
        self.header = capture.header
        for region in capture.ranges:
            lo, hi = region.address, region.address+region.length
            blob = capture.data[region.index]
            if lo < 0 or hi > MEMORY_BYTES or len(blob) != region.length:
                raise ValueError('Invalid captured raw-copy memory')
            self.data[lo:hi] = blob
            self.valid[lo:hi] = b'\1'*region.length
            if region.flags & self.abi['WRITE']:
                self.writable[lo:hi] = b'\1'*region.length

    def read(self, address, size):
        if not 0 <= address < address+size <= MEMORY_BYTES or not all(self.valid[address:address+size]):
            raise ValueError('Missing/uninitialized raw-copy input bytes')
        return bytes(self.data[address:address+size])

    def copy(self, job):
        source, destination, size = job
        if not 0 <= destination < destination+size <= MEMORY_BYTES or (source < destination+size and destination < source+size):
            raise ValueError('Invalid raw-copy destination/overlap')
        value = self.read(source, size)
        if not all(self.writable[destination:destination+size]):
            raise ValueError('MEMCPY writes outside declared mutable ranges')
        self.data[destination:destination+size] = value
        self.valid[destination:destination+size] = b'\1'*size
