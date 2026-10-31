"""Sparse record-v3 catalog closure; synthetic envelopes, not GPU evidence."""
import hashlib
from pathlib import Path
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import identity
import snapshot as s
from test_recorded import recorded
from test_snapshot import fixture

class RecordedCatalogTests(unittest.TestCase):
    def make(self, *, writable=False):
        abi=s.Abi();header=recorded(abi,False);_,rows,blobs=fixture(abi)
        rows[0][2:4]=[0,0x38]
        data=bytes((n*37+11)&255 for n in range(512))
        rows[1][2:6]=[128,0x4e,128,0]
        blobs[1]=data[128:256]
        if writable:
            rows[1][0]=abi['WRITE']|abi['PREIMAGE'];rows[1][1]=abi['TAG_BUFFER']
        captures,report=s.decode_stream(s.encode_reference(header,rows,blobs,abi),abi)
        self.assertFalse(report['failures']);self.assertEqual(len(captures),1)
        resource=dict(name='mesh.bin',kind='mesh',address=0x4e0000,bytes=512,sha256=hashlib.sha256(data).hexdigest())
        return captures[0],resource,data

    def test_unused_catalog_allocation_is_not_a_missing_input(self):
        capture,resource,_=self.make();resource['address']=0x500000
        identity.input_extents(capture,204800,[resource])

    def test_sparse_asset_bytes_are_compared_to_the_bound_full_asset(self):
        capture,resource,data=self.make()
        with tempfile.TemporaryDirectory() as temp:
            path=Path(temp)/'mesh.bin';path.write_bytes(data)
            bindings=[dict(path=str(path),bytes=len(data),sha256=resource['sha256'])]
            result=identity.input_extents(capture,204800,[resource],asset_files=bindings)
            selected=[r for r in result if r['role'].startswith('mesh.bin')]
            self.assertEqual([(r['base_bytes'],r['length_bytes']) for r in selected],[(0x4e0080,128)])
            capture.data[1][0]^=1
            with self.assertRaises(s.SnapshotError):identity.input_extents(capture,204800,[resource],asset_files=bindings)

    def test_sparse_hashed_asset_without_bound_bytes_is_rejected(self):
        capture,resource,_=self.make()
        with self.assertRaises(s.SnapshotError):identity.input_extents(capture,204800,[resource])

    def test_bound_asset_hash_and_immutable_write_rejections_remain_strict(self):
        capture,resource,data=self.make()
        with tempfile.TemporaryDirectory() as temp:
            path=Path(temp)/'mesh.bin';path.write_bytes(data)
            bindings=[dict(path=str(path),bytes=len(data),sha256=resource['sha256'])]
            path.write_bytes(bytes([data[0]^1])+data[1:])
            with self.assertRaises(s.SnapshotError):identity.input_extents(capture,204800,[resource],asset_files=bindings)
            path.write_bytes(data)
            # Transport-valid buffer write cannot alias an immutable catalog.
            capture,resource,_=self.make(writable=True)
            with self.assertRaises(s.SnapshotError):identity.input_extents(capture,204800,[resource],asset_files=bindings)

    def test_missing_recorded_bytes_and_legacy_whole_catalog_rules_remain_rejected(self):
        capture,resource,_=self.make();capture.header['cb_padded_bytes']=256
        with self.assertRaises(s.SnapshotError):identity.input_extents(capture,204800,[])
        capture,resource,_=self.make();capture.header['capture_version']=1
        with self.assertRaises(s.SnapshotError):identity.input_extents(capture,204800,[resource])

if __name__=='__main__':unittest.main()
