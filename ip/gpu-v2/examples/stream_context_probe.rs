use gpu_v2::framebuffer::ports::bank_address;
use gpu_v2::system::pixel::postquad::stream_context::{Context, TokenOracle};
fn main() {
    let p = std::env::args().nth(1).unwrap();
    let s = std::fs::read_to_string(p).unwrap();
    let mut oracle = TokenOracle::default();
    let mut walls = 0;
    let mut pixels = 0;
    let mut commits = 0;
    let mut ce0_row0 = 0;
    for line in s.lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        assert_eq!(words.len(), 34);
        // Decode only controls and payloads owned by an asserted operation.
        // Unknown inactive payload is harmless; unknown active data is not zero.
        let v = |i: usize| {
            words[i].parse::<usize>().unwrap_or_else(|_| {
                panic!("unknown active field {i} at wall {walls}: {}", words[i])
            })
        };
        if v(29) != 0 {
            let c = oracle.pending_commit.take().expect("unpaid quad commit");
            assert_eq!(
                (v(30), v(31), v(32)),
                (usize::from(c.x), usize::from(c.y), usize::from(c.mask))
            );
            commits += 1;
        } else {
            assert!(oracle.pending_commit.is_none(), "missing immediate commit");
        }
        if v(2) == 0 {
            assert_eq!(v(3) | v(10) | v(12), 0, "compute accepted under CE0/fault");
            if v(4) == 0 {
                ce0_row0 += 1;
            }
        }
        if v(10) != 0 {
            let t = oracle.color(v(11) as u8);
            let c = t.context;
            let (b, a) = bank_address(
                0,
                usize::from(c.line),
                (usize::from(c.x) & 15) + usize::from(t.lane & 1),
                (usize::from(c.y) & 15) + usize::from(t.lane >> 1),
            );
            assert_eq!(v(15), b);
            assert_eq!(v(19 + b), a, "Color owner address wall {walls}");
            assert_eq!(
                v(33),
                if v(17) != 0 { 1 << b } else { 0 },
                "Color write-bank owner"
            );
            pixels += 1;
        }
        if v(12) != 0 {
            let t = oracle.depth(v(13) as u8);
            let c = t.context;
            let (b, a) = bank_address(
                1,
                usize::from(c.line),
                (usize::from(c.x) & 15) + usize::from(t.lane & 1),
                (usize::from(c.y) & 15) + usize::from(t.lane >> 1),
            );
            assert_eq!(v(16), b);
            assert_eq!(v(19 + b), a, "Depth owner address wall {walls}");
            assert_eq!(
                v(33),
                if v(18) != 0 { 1 << b } else { 0 },
                "Depth write-bank owner"
            );
        }
        if v(3) != 0 {
            let c = Context {
                slot: v(5) as u8,
                line: v(6) as u8,
                x: v(7) as u16,
                y: v(8) as u8,
                mask: v(9) as u8,
            };
            if v(27) != 0 || v(28) != 0 {
                for lane in 0..4 {
                    let (b, a) = bank_address(
                        v(28),
                        usize::from(c.line),
                        (usize::from(c.x) & 15) + (lane & 1),
                        (usize::from(c.y) & 15) + (lane >> 1),
                    );
                    assert_eq!(v(23 + b), a, "input group address");
                }
            }
            oracle.accept_row(v(4) as u8, c);
        }
        walls += 1;
        assert!(walls < 200000);
    }
    assert!(
        walls > 0 && pixels > 0 && commits > 0,
        "trace exercised no complete work"
    );
    assert!(
        oracle.issued.is_empty()
            && oracle.pending_depth.is_none()
            && oracle.pending_commit.is_none()
    );
    println!("PASS independent accepted-lane FIFO: {walls} walls, {pixels} Color/Depth pairs, {commits} exact commits, {ce0_row0} CE0 row0 views");
}
