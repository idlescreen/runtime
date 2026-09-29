use super::*;

/// xorshift64* deterministic stand-in for proptest's generators.
struct Xor(u64);
impl Xor {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// Same seed always yields the same sequence (determinism).
#[test]
fn same_seed_same_stream() {
    let mut g = Xor(0xB16D_1CE5_0001);
    for _ in 0..128 {
        let seed = g.next();
        let steps = 1 + (g.next() % 64) as usize;
        let mut a = LcgRng::new(seed);
        let mut b = LcgRng::new(seed);
        for _ in 0..steps {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }
}

/// `next_usize(max)` is always in `0..max` when max > 0.
#[test]
fn next_usize_in_range() {
    let mut g = Xor(0xB16D_1CE5_0002);
    for _ in 0..128 {
        let max = 1 + (g.next() % 10_000) as usize;
        let mut rng = LcgRng::new(g.next());
        for _ in 0..32 {
            let n = rng.next_usize(max);
            assert!(n < max, "n={n} max={max}");
        }
    }
}

/// `next_f32` stays in [0, 1).
#[test]
fn next_f32_unit_interval() {
    let mut g = Xor(0xB16D_1CE5_0003);
    for _ in 0..128 {
        let mut rng = LcgRng::new(g.next());
        for _ in 0..32 {
            let f = rng.next_f32();
            assert!((0.0..1.0).contains(&f), "f={f}");
        }
    }
}
