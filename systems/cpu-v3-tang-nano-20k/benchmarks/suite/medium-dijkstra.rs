// bench-max-cycles: 400000
// bench-expected-halt: 2898
// bench-tier: medium
use crate::dsl_rt::*;

// Dijkstra shortest paths from node 0 over a 96-node graph with on-the-fly
// deterministic weights; exact distance-vector checksum.
const N: u16 = 96;
static DIST: [u16; 96] = [0; 96];
static VISITED: [u16; 96] = [0; 96];

fn weight(i: u16, j: u16) -> u16 {
    // edge exists when the low two bits of a hash are zero
    let h = (i << 3) ^ (j << 1) ^ (i + j);
    if h & 3 == 0 { ((h >> 2) & 15) + 1 } else { 0 }
}

fn main() {
    let mut dist = DIST.as_array();
    let mut visited = VISITED.as_array();
    let mut i: u16 = 0;
    while i < N {
        dist[i] = 0xffff;
        visited[i] = 0;
        i = i + 1;
    }
    dist[0u16] = 0;
    let mut round: u16 = 0;
    while round < N {
        // extract the unvisited minimum
        let mut best: u16 = 0xffff;
        let mut u: u16 = 0;
        i = 0;
        while i < N {
            if visited[i] == 0 && dist[i] < best {
                best = dist[i];
                u = i;
            }
            i = i + 1;
        }
        if best == 0xffff {
            round = N;
        } else {
            visited[u] = 1;
            let mut j: u16 = 0;
            while j < N {
                let w = weight(u, j);
                if w != 0 && visited[j] == 0 {
                    let cand = dist[u] + w;
                    if cand < dist[j] {
                        dist[j] = cand;
                    }
                }
                j = j + 1;
            }
            round = round + 1;
        }
    }
    let mut cs: u16 = 0;
    i = 0;
    while i < N {
        cs = cs ^ dist[i];
        cs = (cs << 1) | (cs >> 15);
        i = i + 1;
    }
    halt(cs);
}
