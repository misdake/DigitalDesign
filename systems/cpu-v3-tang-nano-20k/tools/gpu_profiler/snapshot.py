"""Frozen pre-submit inputs: bounded framing, integrity and byte-addressed storage.

The driver ABI is read from its one authoritative Rust constants file. This tool
does not discover or restore arbitrary GPU register/cache state.
"""
from dataclasses import dataclass
from enum import IntEnum
import hashlib
import json
from pathlib import Path
import re
import struct
import zlib
from layout import get_layout, header_layout


MAGIC = b"GPS1"
WIRE_VERSION = 1
FRAME = struct.Struct("<4sBBHIIII")
CRC = struct.Struct("<I")
DATA_HEADER = struct.Struct("<HHII")
RANGE_END = struct.Struct("<HHII")
END = struct.Struct("<IIIII")
MAX_FRAME_BYTES = 4096
MAX_CHUNK_BYTES = 1024
MEMORY_BYTES = 8 * 1024 * 1024
MAX_TOTAL_BYTES = MEMORY_BYTES * 2
DEFAULT_ABI = Path(__file__).resolve().parents[2] / "rcc/gpu_capture_abi.rs"


class SnapshotError(ValueError):
    pass


class Kind(IntEnum):
    BEGIN = 1
    RANGE = 2
    DATA = 3
    RANGE_END = 4
    END = 5
    ERROR = 6


def checksum(data, previous=0):
    return zlib.crc32(data, previous) & 0xffffffff


def words_to_bytes(words):
    if any(type(w) is not int or not 0 <= w <= 65535 for w in words):
        raise SnapshotError("Descriptor words must be u16")
    return struct.pack("<" + "H" * len(words), *words)


def bytes_to_words(data):
    if len(data) % 2:
        raise SnapshotError("Odd word payload")
    return list(struct.unpack("<" + "H" * (len(data) // 2), data))


class Abi:
    """Consume literal constants; never maintain a second index table."""
    def __init__(self, source=DEFAULT_ABI):
        self.source = Path(source).resolve()
        raw = self.source.read_bytes()
        self.sha256 = hashlib.sha256(raw).hexdigest()
        self.constants = {m.group(1): int(m.group(2).replace("_", ""), 0)
                          for m in re.finditer(
                              r"^pub const (GPU_CAPTURE_\w+): u16 = (0x[0-9a-fA-F_]+|[0-9_]+);$",
                              raw.decode("utf-8-sig").replace("\r\n", "\n"), re.M)}
        if (self["VERSION"] != 1 or self["WORDS"] != 64 or self["RANGE_WORDS"] != 8 or
                self['NATIVE_COMMAND_ABI'] != 1):
            raise SnapshotError("Unsupported driver capture ABI")

    def __getitem__(self, name):
        try:
            return self.constants["GPU_CAPTURE_" + name]
        except KeyError as error:
            raise SnapshotError("Missing authoritative ABI constant: " + name) from error

    def word(self, words, name):
        return words[self[name]]

    def pair(self, words, name):
        at = self[name]
        return words[at] | words[at + 1] << 16

    def header(self, words):
        if len(words) != self["WORDS"]:
            raise SnapshotError("Invalid driver descriptor length")
        words_to_bytes(words)
        if (self.word(words, "H_VERSION") != self["VERSION"] or
                self.word(words, "H_WORDS") != self["WORDS"] or
                self.word(words, "H_FLAGS") != self["PUBLISHED"] or
                self.word(words, "H_BINDINGS") != self["BINDINGS_SELF_CONTAINED"]):
            raise SnapshotError("Unpublished, unsupported or non-self-contained descriptor")
        if self.word(words, "H_GPU_STATUS") != 0:
            raise SnapshotError("GPU is busy or faulted at the alleged freeze point")
        try:
            layout = get_layout(self.word(words, "H_LAYOUT_ABI"))
        except ValueError as error:
            raise SnapshotError(str(error)) from error
        if (self.word(words, "H_COMMAND_ABI") != self["NATIVE_COMMAND_ABI"] or
                self.word(words, "H_RANGE_VERSION") != self["RANGE_VERSION"] or
                self.word(words, "H_RANGE_WORDS") != self["RANGE_WORDS"]):
            raise SnapshotError("Incompatible layout/command/range ABI")
        if self.word(words, "H_MANIFEST_WORDS") != self["MANIFEST_WORDS"]:
            raise SnapshotError("Invalid identity declaration length")
        if any(words[self["H_MANIFEST"] - 2:self["H_MANIFEST"]]):
            raise SnapshotError("Reserved descriptor words are nonzero")
        result = {name: self.pair(words, "H_" + field) for name, field in [
            ("token", "TOKEN"), ("submission", "SUBMISSION"),
            ("cb_base", "COMMAND_BASE"), ("cb_bytes", "COMMAND_BYTES"),
            ("cb_padded_bytes", "COMMAND_PADDED_BYTES"),
            ("object_base", "OBJECT_BASE"), ("object_bytes", "OBJECT_BYTES"),
            ("ui_base", "UI_BASE"), ("ui_bytes", "UI_BYTES"),
            ("color_base", "COLOR_BASE"), ("depth_base", "DEPTH_BASE"),
            ("front_base", "FRONT_BASE"), ("plane_bytes", "PLANE_BYTES")]}
        result.update(frame=self.word(words, "H_FRAME"), slot=self.word(words, "H_SLOT"),
                      count_minus_one=self.word(words, "H_COUNT"),
                      range_count=self.word(words, "H_RANGE_COUNT"),
                      front_format=self.word(words, "H_FRONT_FORMAT"),
                      target_format=self.word(words, "H_TARGET_FORMAT"),
                      display_status=self.word(words, "H_DISPLAY_STATUS"),
                      publication=(self.word(words, "H_PUBLICATION_LOW") |
                                   self.word(words, "H_PUBLICATION_HIGH") << 16))
        # Preserve the v1 manifest representation and input hash byte for byte.
        if layout.abi != 1:
            result['layout_abi'] = layout.abi
            result['command_abi'] = 1
        at = self["H_MANIFEST"]
        result["manifest_id"] = words_to_bytes(words[at:at + self["MANIFEST_WORDS"]]).hex()
        valid, padded, base = result["cb_bytes"], result["cb_padded_bytes"], result["cb_base"]
        if (not result["token"] or result["slot"] > 1 or not 1 <= result["range_count"] <= self["MAX_RANGES"] or
                base % 128 or not 0 < valid <= 65536 or valid % 4 or
                result["count_minus_one"] != valid // 4 - 1 or
                padded % 128 or not (valid + 127) // 128 * 128 <= padded <= 65536 or
                base + padded > MEMORY_BYTES):
            raise SnapshotError("Invalid CB, count, token or range bound")
        if result["plane_bytes"] != layout.plane_bytes:
            raise SnapshotError("Unsupported target plane geometry")
        for key in ["color_base", "depth_base"]:
            if result[key] % (65536 if layout.abi == 2 else 128) or result[key] + layout.allocation_bytes > MEMORY_BYTES:
                raise SnapshotError("Invalid target plane address")
        for prefix in ["object", "ui"]:
            address, size = result[prefix + "_base"], result[prefix + "_bytes"]
            if size and (address % 2 or size % 2 or address + size > MEMORY_BYTES):
                raise SnapshotError("Invalid optional input range")
        front = result["front_base"]
        if front and (front % (32 if layout.abi == 2 else 128) or front + layout.allocation_bytes > MEMORY_BYTES):
            raise SnapshotError("Invalid old Front address")
        if result["target_format"] not in [0, 1] or result["front_format"] not in [0, 1]:
            raise SnapshotError("Unknown Display encoding")
        if result["manifest_id"] == "0" * 64:
            raise SnapshotError("Missing build/catalog identity declaration")
        try:
            header_layout(result)
        except ValueError as error:
            raise SnapshotError(str(error)) from error
        return result

    def range(self, index, words):
        if len(words) != self["RANGE_WORDS"]:
            raise SnapshotError("Invalid range descriptor length")
        words_to_bytes(words)
        item = MemoryRange(index, self.word(words, "R_FLAGS"), self.word(words, "R_TAG"),
                           self.pair(words, "R_BASE"), self.pair(words, "R_BYTES"),
                           self.pair(words, "R_GENERATION"), tuple(words))
        if (not 0 <= index < self["MAX_RANGES"] or not item.length or
                item.address % 2 or item.length % 2 or item.address + item.length > MEMORY_BYTES or
                item.flags & ~self["FLAGS_ALL"] or not item.flags & (self["READ"] | self["WRITE"]) or
                (item.flags & self["IMMUTABLE"] and item.flags & self["WRITE"])):
            raise SnapshotError("Invalid range flags/address/length")
        return item


@dataclass(frozen=True)
class MemoryRange:
    index: int
    flags: int
    tag: int
    address: int
    length: int
    generation: int
    words: tuple


@dataclass(frozen=True)
class Frame:
    kind: Kind
    snapshot: int
    attempt: int
    sequence: int
    payload: bytes

    def encode(self):
        if (len(self.payload) > MAX_FRAME_BYTES or
                any(type(n) is not int or not 0 <= n <= 0xffffffff
                    for n in [self.snapshot, self.attempt, self.sequence])):
            raise SnapshotError("Frame payload too large")
        header = FRAME.pack(MAGIC, WIRE_VERSION, int(self.kind), 0,
                            self.snapshot, self.attempt, self.sequence, len(self.payload))
        body = header + self.payload
        return body + CRC.pack(checksum(body))


def rle16_encode(data):
    """Literal or repeat word runs; raw fallback is selected by the sender."""
    words = bytes_to_words(data)
    out, at = bytearray(), 0
    while at < len(words):
        run = 1
        while at + run < len(words) and words[at + run] == words[at] and run < 32768:
            run += 1
        if run >= 3:
            out += struct.pack("<HH", 0x8000 | (run - 1), words[at])
            at += run
        else:
            begin = at
            at += run
            while at < len(words) and at - begin < 32768:
                if at + 2 < len(words) and words[at] == words[at + 1] == words[at + 2]:
                    break
                at += 1
            out += struct.pack("<H", at - begin - 1) + words_to_bytes(words[begin:at])
    return bytes(out)


def rle16_decode(data, expected):
    if not 0 < expected <= MAX_CHUNK_BYTES or expected % 2:
        raise SnapshotError("Invalid decoded chunk bound")
    words, at, out = bytes_to_words(data), 0, bytearray()
    while at < len(words):
        token = words[at]
        at += 1
        count = (token & 32767) + 1
        if len(out) + count * 2 > expected:
            raise SnapshotError("RLE expansion exceeds declared size")
        if token & 32768:
            if at == len(words):
                raise SnapshotError("Truncated RLE repeat")
            out += struct.pack("<H", words[at]) * count
            at += 1
        else:
            if at + count > len(words):
                raise SnapshotError("Truncated RLE literal")
            out += words_to_bytes(words[at:at + count])
            at += count
    if len(out) != expected:
        raise SnapshotError("RLE decoded length mismatch")
    return bytes(out)


class Attempt:
    """One attempt owns its bytes; an incomplete attempt cannot donate chunks."""
    def __init__(self, first, abi):
        self.abi = abi
        self.snapshot, self.attempt = first.snapshot, first.attempt
        self.sequence = 0
        self.descriptor = bytes_to_words(first.payload)
        self.header = abi.header(self.descriptor)
        if self.snapshot != self.header["token"] or not self.attempt:
            raise SnapshotError("Envelope and prepared token differ")
        self.ranges, self.data, self.closed = [], {}, set()
        self.chunk_count, self.total, self.global_crc = 0, 0, checksum(first.payload)
        self.complete = False
        self.elapsed_ms = None

    def feed(self, frame):
        if self.complete or (frame.snapshot, frame.attempt, frame.sequence) != (
                self.snapshot, self.attempt, self.sequence + 1):
            raise SnapshotError("Out-of-order, duplicated or mixed-attempt frame")
        self.sequence += 1
        if frame.kind == Kind.RANGE:
            if self.chunk_count or len(frame.payload) != 2 + self.abi["RANGE_WORDS"] * 2:
                raise SnapshotError("Range table changed after data began")
            index, = struct.unpack_from("<H", frame.payload)
            if index != len(self.ranges) or index >= self.header["range_count"]:
                raise SnapshotError("Invalid range-table order")
            item = self.abi.range(index, bytes_to_words(frame.payload[2:]))
            if sum(r.length for r in self.ranges) + item.length > MAX_TOTAL_BYTES:
                raise SnapshotError("Aggregate range limit exceeded")
            self.ranges.append(item)
            self.data[index] = bytearray()
            self.global_crc = checksum(frame.payload, self.global_crc)
        elif frame.kind == Kind.DATA:
            if len(self.ranges) != self.header["range_count"] or len(frame.payload) < DATA_HEADER.size:
                raise SnapshotError("Data precedes complete range table")
            index, codec, offset, decoded = DATA_HEADER.unpack_from(frame.payload)
            if index not in self.data or index in self.closed or not 0 < decoded <= MAX_CHUNK_BYTES or decoded % 2:
                raise SnapshotError("Invalid data chunk identity/size")
            if index != len(self.closed) or offset != len(self.data[index]) or offset + decoded > self.ranges[index].length:
                raise SnapshotError("Data gap/overlap or nonsequential range")
            encoded = frame.payload[DATA_HEADER.size:]
            if codec == 0:
                if len(encoded) != decoded:
                    raise SnapshotError("Raw chunk length mismatch")
                raw = encoded
            elif codec == 1:
                raw = rle16_decode(encoded, decoded)
            else:
                raise SnapshotError("Unknown lossless chunk codec")
            self.data[index] += raw
            self.global_crc = checksum(raw, self.global_crc)
            self.total += decoded
            self.chunk_count += 1
        elif frame.kind == Kind.RANGE_END:
            if len(frame.payload) != RANGE_END.size:
                raise SnapshotError("Invalid range-end record")
            index, reserved, count, crc = RANGE_END.unpack(frame.payload)
            if (reserved or index != len(self.closed) or index not in self.data or
                    count != len(self.data[index]) or count != self.ranges[index].length or
                    crc != checksum(self.data[index])):
                raise SnapshotError("Incomplete range or range CRC mismatch")
            self.closed.add(index)
        elif frame.kind == Kind.END:
            if len(frame.payload) != END.size:
                raise SnapshotError("Invalid complete-END record")
            ranges, total, chunks, crc, elapsed = END.unpack(frame.payload)
            if (ranges != self.header["range_count"] or len(self.closed) != ranges or
                    total != self.total or chunks != self.chunk_count or crc != self.global_crc):
                raise SnapshotError("Missing input, incomplete END or aggregate CRC mismatch")
            self.complete, self.elapsed_ms = True, elapsed
            self.validate_memory()
        elif frame.kind == Kind.ERROR:
            raise SnapshotError("Board reported snapshot failure: " + frame.payload.hex())
        else:
            raise SnapshotError("Unexpected frame kind in attempt")

    def read(self, address, length):
        if (type(address) is not int or type(length) is not int or
                not 0 <= address < MEMORY_BYTES or not 0 < length <= MEMORY_BYTES or
                address + length > MEMORY_BYTES):
            raise SnapshotError("Invalid bounded memory read")
        result, covered = bytearray(length), bytearray(length)
        for item in self.ranges:
            if len(self.data[item.index]) != item.length:
                raise SnapshotError('Captured range bytes do not match their declared extent')
            lo, hi = max(address, item.address), min(address + length, item.address + item.length)
            if lo < hi:
                result[lo - address:hi - address] = self.data[item.index][lo - item.address:hi - item.address]
                covered[lo - address:hi - address] = b"\1" * (hi - lo)
        if not all(covered):
            raise SnapshotError(f"Missing input bytes at 0x{address:x}+0x{length:x}")
        return bytes(result)

    def validate_memory(self):
        if self.header.get('layout_abi', 1) == 2:
            layout=header_layout(self.header)
            for item in self.ranges:
                if item.tag in (self.abi['TAG_COLOR'],self.abi['TAG_DEPTH'],self.abi['TAG_FRONT']):
                    base=self.header[{self.abi['TAG_COLOR']:'color_base',self.abi['TAG_DEPTH']:'depth_base',
                                      self.abi['TAG_FRONT']:'front_base'}[item.tag]]
                    if item.flags & self.abi['FULL_CLEAR'] and not base <= item.address < item.address+item.length <= base+layout.plane_bytes:
                        raise SnapshotError('FULL_CLEAR cannot declare allocation guard bytes initialized')
                    if item.flags & self.abi['WRITE'] and not base <= item.address < item.address+item.length <= base+layout.plane_bytes:
                        raise SnapshotError('A column allocation guard is not a writable plane extent')
        for i, a in enumerate(self.ranges):
            for b in self.ranges[i + 1:]:
                lo, hi = max(a.address, b.address), min(a.address + a.length, b.address + b.length)
                if lo < hi and self.data[a.index][lo - a.address:hi - a.address] != self.data[b.index][lo - b.address:hi - b.address]:
                    raise SnapshotError("Aliased captured ranges contain conflicting bytes")
        cb = self.read(self.header["cb_base"], self.header["cb_padded_bytes"])
        if any(cb[self.header["cb_bytes"]:]):
            raise SnapshotError("CB fetch padding is not zero")
        instructions = struct.unpack("<" + "I" * (self.header["cb_bytes"] // 4), cb[:self.header["cb_bytes"]])
        if instructions[-1] != 0xf0000000 or any(w >> 28 == 15 for w in instructions[:-1]):
            raise SnapshotError("END is missing, malformed or before valid end")

    def manifest(self):
        if not self.complete:
            raise SnapshotError("Incomplete snapshots cannot be materialized")
        identity = hashlib.sha256(words_to_bytes(self.descriptor))
        ranges = []
        for item in self.ranges:
            raw = bytes(self.data[item.index])
            identity.update(words_to_bytes(item.words))
            identity.update(raw)
            ranges.append({"index": item.index, "flags": item.flags, "tag": item.tag,
                           "base_bytes": item.address, "length_bytes": item.length,
                           "generation": item.generation, "descriptor_words": list(item.words),
                           "file": f"range-{item.index:03d}.bin", "sha256": hashlib.sha256(raw).hexdigest(),
                           "crc32": f"{checksum(raw):08x}"})
        return {"schema": "digitaldesign.gpu-snapshot", "version": 1,
                "snapshot": self.snapshot, "attempt": self.attempt, "complete": True,
                "cache_state": "not_captured", "replay_policy": "cold_only",
                "descriptor_words": self.descriptor, "header": self.header,
                "abi_sha256": self.abi.sha256, "input_sha256": identity.hexdigest(),
                "ranges": ranges, "chunks": self.chunk_count, "decoded_bytes": self.total,
                "board_dump_ms": self.elapsed_ms,
                "identity_note": "Header ID is a declaration; actual firmware/bitstream require a host programming receipt."}

    def save(self, output):
        output = Path(output)
        if output.exists() and any(output.iterdir()):
            raise SnapshotError("Snapshot output must be new/empty")
        manifest = self.manifest()
        output.mkdir(parents=True, exist_ok=True)
        for item in manifest["ranges"]:
            (output / item["file"]).write_bytes(self.data[item["index"]])
        (output / "snapshot.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        return manifest


def decode_stream(raw, abi):
    """Return independent complete attempts and visible failures, never partial inputs."""
    at, active, completed, failures, ignored = 0, None, [], [], 0
    while at < len(raw):
        if raw[at:at + 4] != MAGIC:
            if active is not None:
                failures.append({"offset": at, "error": "Unexpected bytes inside attempt"})
                active = None
            next_at = raw.find(MAGIC, at + 1)
            if next_at < 0:
                ignored += len(raw) - at
                break
            ignored += next_at - at
            at = next_at
        if at + FRAME.size > len(raw):
            failures.append({"offset": at, "error": "Truncated frame header"})
            break
        magic, version, kind, flags, snapshot, attempt, seq, size = FRAME.unpack_from(raw, at)
        if version != WIRE_VERSION or flags or size > MAX_FRAME_BYTES:
            failures.append({"offset": at, "error": "Unknown wire version/flags or excessive length"})
            active = None
            at += 4
            continue
        end = at + FRAME.size + size + CRC.size
        if end > len(raw):
            failures.append({"offset": at, "error": "Truncated frame payload"})
            active = None
            next_at = raw.find(MAGIC, at + 4)
            if next_at < 0:
                break
            at = next_at
            continue
        body = raw[at:end - 4]
        expected, = CRC.unpack_from(raw, end - 4)
        try:
            if checksum(body) != expected:
                raise SnapshotError("Frame CRC mismatch")
            frame = Frame(Kind(kind), snapshot, attempt, seq, raw[at + FRAME.size:end - 4])
            if frame.kind == Kind.BEGIN:
                if active is not None:
                    failures.append({"offset": at, "error": "New BEGIN interrupted incomplete attempt"})
                if seq:
                    raise SnapshotError("BEGIN sequence must be zero")
                active = Attempt(frame, abi)
            elif active is None:
                raise SnapshotError("Frame outside an explicit BEGIN")
            else:
                active.feed(frame)
            if active is not None and active.complete:
                completed.append(active)
                active = None
        except (SnapshotError, ValueError) as error:
            failures.append({"offset": at, "error": str(error)})
            active = None
            # A truncated transmission may have consumed the next BEGIN while
            # satisfying its alleged length. Resume searching, never reuse data.
            next_at = raw.find(MAGIC, at + 4)
            if next_at >= 0:
                at = next_at
                continue
        at = end
    if active is not None:
        failures.append({"offset": at, "error": "Missing complete END"})
    return completed, {"version": WIRE_VERSION, "complete_attempts": len(completed),
                       "failures": failures, "ignored_bytes": ignored,
                       "raw_bytes": len(raw), "raw_sha256": hashlib.sha256(raw).hexdigest()}


def encode_reference(descriptor, range_words, blobs, abi, attempt=1, chunk_bytes=512,
                     compress=True, elapsed_ms=0):
    """Reference encoder for firmware cross-checks; not a board measurement."""
    if chunk_bytes % 2 or not 0 < chunk_bytes <= MAX_CHUNK_BYTES:
        raise SnapshotError("Invalid bounded encoder chunk")
    header = abi.header(descriptor)
    if len(range_words) != header["range_count"] or len(blobs) != len(range_words):
        raise SnapshotError("Missing encoder ranges")
    payload = words_to_bytes(descriptor)
    global_crc, chunks, total, sequence = checksum(payload), 0, 0, 0
    frames = [Frame(Kind.BEGIN, header["token"], attempt, 0, payload).encode()]

    def emit(kind, body):
        nonlocal sequence
        sequence += 1
        frames.append(Frame(kind, header["token"], attempt, sequence, body).encode())

    for index, row in enumerate(range_words):
        item = abi.range(index, row)
        if len(blobs[index]) != item.length:
            raise SnapshotError("Encoder range length mismatch")
        body = struct.pack("<H", index) + words_to_bytes(row)
        emit(Kind.RANGE, body)
        global_crc = checksum(body, global_crc)
    for index, blob in enumerate(blobs):
        for offset in range(0, len(blob), chunk_bytes):
            data = blob[offset:offset + chunk_bytes]
            encoded = rle16_encode(data) if compress else data
            codec = int(compress and len(encoded) < len(data))
            if not codec:
                encoded = data
            emit(Kind.DATA, DATA_HEADER.pack(index, codec, offset, len(data)) + encoded)
            global_crc = checksum(data, global_crc)
            total += len(data)
            chunks += 1
        emit(Kind.RANGE_END, RANGE_END.pack(index, 0, len(blob), checksum(blob)))
    emit(Kind.END, END.pack(len(blobs), total, chunks, global_crc, elapsed_ms))
    return b"".join(frames)


def load_snapshot(path, abi):
    """Recheck every materialized byte and descriptor; cached JSON is not trusted."""
    path = Path(path).resolve()
    if path.is_dir():
        path /= "snapshot.json"
    metadata = json.loads(path.read_text(encoding="utf-8"))
    if (metadata.get("schema") != "digitaldesign.gpu-snapshot" or
            metadata.get("version") != 1 or metadata.get("complete") is not True or
            metadata.get("abi_sha256") != abi.sha256 or
            metadata.get("cache_state") != "not_captured" or
            metadata.get("replay_policy") != "cold_only"):
        raise SnapshotError("Unknown, partial or ABI-mismatched snapshot")
    first = Frame(Kind.BEGIN, metadata["snapshot"], metadata["attempt"], 0,
                  words_to_bytes(metadata["descriptor_words"]))
    result = Attempt(first, abi)
    ranges = metadata["ranges"]
    if len(ranges) != result.header["range_count"]:
        raise SnapshotError("Incomplete saved range table")
    for index, item in enumerate(ranges):
        if item["index"] != index or item["file"] != f"range-{index:03d}.bin":
            raise SnapshotError("Unsafe or inconsistent range filename/index")
        candidate = path.parent / item["file"]
        filename = candidate.resolve()
        if filename.parent != path.parent or candidate.is_symlink():
            raise SnapshotError("Range escapes the snapshot directory")
        region = abi.range(index, item["descriptor_words"])
        if result.total + region.length > MAX_TOTAL_BYTES:
            raise SnapshotError("Saved snapshot exceeds the aggregate bound")
        if filename.stat().st_size != region.length:
            raise SnapshotError("Saved range file length mismatch")
        blob = filename.read_bytes()
        if hashlib.sha256(blob).hexdigest() != item["sha256"] or f"{checksum(blob):08x}" != item["crc32"]:
            raise SnapshotError("Saved range digest mismatch")
        result.ranges.append(region)
        result.data[index] = bytearray(blob)
        result.closed.add(index)
        result.total += len(blob)
    if result.total > MAX_TOTAL_BYTES:
        raise SnapshotError("Saved snapshot exceeds the aggregate bound")
    result.complete = True
    result.chunk_count = metadata["chunks"]
    result.elapsed_ms = metadata["board_dump_ms"]
    result.validate_memory()
    fresh = result.manifest()
    if (fresh["input_sha256"] != metadata["input_sha256"] or fresh["header"] != metadata["header"] or
            fresh["ranges"] != metadata["ranges"] or result.total != metadata["decoded_bytes"]):
        raise SnapshotError("Saved metadata does not describe its verified bytes")
    return result
