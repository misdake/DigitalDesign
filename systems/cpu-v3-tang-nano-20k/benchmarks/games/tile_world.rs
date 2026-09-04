use crate::dsl_rt::*;

const CELLS: u16 = 1200;
static TILES: [u16; 1200] = [0; 1200];

fn main() {
    let mut tiles = TILES.as_array();
    let mut i: u16 = 0;
    while i < CELLS {
        tiles[i] = if (i & 31) == 0 || (i & 31) == 31 { 1 } else { 0 };
        i = i + 1;
    }
    let mut player: u16 = 41;
    let mut score: u16 = 0;
    let mut frame: u16 = 0;
    while frame < 30 {
        let direction = frame & 3;
        let mut candidate = player;
        if direction == 0 { candidate = player + 1; }
        if direction == 1 { candidate = player + 32; }
        if direction == 2 { candidate = player - 1; }
        if direction == 3 { candidate = player - 32; }
        if tiles[candidate] == 0 { player = candidate; }
        i = 0;
        while i < CELLS {
            score = score + (tiles[i] ^ (i & 7));
            i = i + 1;
        }
        frame = frame + 1;
    }
    if player == 74 && score == 60494 { halt(1); } else { halt(0); }
}
