"""Resolve recorded sidecar formats to their application-owned codecs.

Runner version 1 was published as 'ordinary', but only describes Bloxorz.
Keep this explicit legacy-format binding; do not infer a game from event data
or import application rules into the common collector/model.
"""
from functools import lru_cache
from importlib.util import module_from_spec, spec_from_file_location
from pathlib import Path


@lru_cache(maxsize=1, typed=True)
def codec(version):
    if type(version) is not int or version != 1:
        raise ValueError('Unsupported application sidecar version: '+str(version))
    path = Path(__file__).resolve().parents[2]/'applications/bloxorz/tools/profile.py'
    spec = spec_from_file_location('bloxorz_profile_v1', path)
    module = module_from_spec(spec)
    spec.loader.exec_module(module)
    return module
