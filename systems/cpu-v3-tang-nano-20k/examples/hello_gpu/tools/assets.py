"""The complete first-frame producer: source vertices and selected base-font glyphs."""
import argparse
from pathlib import Path
from native_asset_source import Sources, Vertex


def build(output):
    assets = Sources(output)
    assets.font('HELLO_ATLAS', 'HELLO GPU')
    assets.geometry('HELLO_TRIANGLE', [
        Vertex((-1,-0.8,0), color=(255,64,64)),
        Vertex((1,-0.8,0), color=(64,255,64)),
        Vertex((0,1,0), color=(64,64,255)),
    ], [[0,1,2]])
    assets.write_manifest('hello_assets.rs', sources=[__file__])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', type=Path, required=True)
    build(parser.parse_args().output)
