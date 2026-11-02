//! The trainer's randomness, and the only randomness in this ticket.
//!
//! **The engine has none and must not grow one** (§5.3: "No RNG in the engine.
//! Visit order, tie-breaking and dedup domination are part of the observable
//! contract"). Everything a Solve does is a function of its arguments, so the
//! trainer's draws live here, in `bench`, and nothing under `engine/` imports
//! this file.
//!
//! Three decisions, each of them load-bearing for "reproducible from a committed
//! seed":
//!
//! * **splitmix64**, exactly §5.6's algorithm, so the repository has one PRNG
//!   with one set of published test vectors and the crate gains no dependency;
//! * **purpose-separated streams** (§5.7's habit): the tuning stream is
//!   `tune_seed ^ C_TUNE` and the bootstrap stream is `tune_seed ^ C_BOOTSTRAP`,
//!   so a future change that draws more indices cannot silently shift the
//!   bootstrap's resamples;
//! * **Irwin–Hall(12)** for the normal transform rather than Box–Muller: it uses
//!   only IEEE-754 `+`, `−` and `×` on exact powers of two, so it is
//!   bit-reproducible across targets. `ln`/`sqrt` from libm are not guaranteed
//!   correctly rounded, which would make "the same seed gives the same table"
//!   true on this box and false on the next one.

/// §5.6's increment: the golden-ratio odd constant.
const INCREMENT: u64 = 0x9E37_79B9_7F4A_7C15;
const MULTIPLIER_1: u64 = 0xBF58_476D_1CE4_E5B9;
const MULTIPLIER_2: u64 = 0x94D0_49BB_1331_11EB;

/// The tuning stream's separator: the leading 64 bits of the fractional part of
/// √2, in the same spirit as §5.7's π constants and deliberately not equal to
/// `C_fill` or `C_sequence`, so the trainer can never share a stream with the
/// generator even if a later ticket makes it generate games.
pub const C_TUNE: u64 = 0x6A09_E667_F3BC_C908;

/// The bootstrap stream's separator: the leading 64 bits of the fractional part
/// of √3. A paired bootstrap's resamples must not move when the CE search draws
/// one more number, so it gets its own stream.
pub const C_BOOTSTRAP: u64 = 0xBB67_AE85_84CA_A73B;

/// §5.6's splitmix64: one `u64` of state, one draw at a time.
#[derive(Clone, Debug)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// §5.6: the state is the seed, zero-extended, with no pre-scramble.
    pub const fn new(seed: u64) -> SplitMix64 {
        SplitMix64 { state: seed }
    }

    /// The tuning stream of a committed `tune_seed`.
    pub const fn tune_stream(tune_seed: u64) -> SplitMix64 {
        SplitMix64::new(tune_seed ^ C_TUNE)
    }

    /// The paired bootstrap's stream.
    pub const fn bootstrap_stream(tune_seed: u64) -> SplitMix64 {
        SplitMix64::new(tune_seed ^ C_BOOTSTRAP)
    }

    /// §5.6's four lines, verbatim.
    #[inline]
    pub fn draw(&mut self) -> u64 {
        self.state = self.state.wrapping_add(INCREMENT);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(MULTIPLIER_1);
        z = (z ^ (z >> 27)).wrapping_mul(MULTIPLIER_2);
        z ^ (z >> 31)
    }

    /// §5.6's unbiased bounded draw: Lemire's multiply-shift with rejection.
    ///
    /// `% n` would bias the low indices, and a biased minibatch draw is a
    /// reproducibility bug that only shows up as a slightly different table.
    #[inline]
    pub fn bounded(&mut self, n: u32) -> u32 {
        assert!(n >= 1, "bounded(n) needs n >= 1");
        let n = n as u64;
        let mut x = self.draw();
        let mut m = (x as u128) * (n as u128);
        let mut low = m as u64;
        if low < n {
            let threshold = (u64::MAX - n + 1) % n;
            while low < threshold {
                x = self.draw();
                m = (x as u128) * (n as u128);
                low = m as u64;
            }
        }
        let _ = x;
        (m >> 64) as u32
    }

    /// A standard normal, by Irwin–Hall(12): the sum of twelve uniforms minus 6.
    ///
    /// Variance is exactly 1 and the support is `[−6, 6]`, which is the whole
    /// point: no transcendental function, so the bits are the same on every
    /// IEEE-754 target. The truncation is irrelevant to a cross-entropy search
    /// whose samples are bounded at `|w| ≤ 1024` anyway.
    #[inline]
    pub fn normal(&mut self) -> f64 {
        let mut sum = 0.0f64;
        let mut k = 0;
        while k < 12 {
            // 53 bits, exactly the f64 mantissa: `(draw() >> 11) as f64` is exact
            // and `2^-53` is a power of two, so the product is exact too.
            let uniform = ((self.draw() >> 11) as f64) * (1.0 / 9007199254740992.0);
            sum += uniform;
            k += 1;
        }
        sum - 6.0
    }

    /// `k` distinct indices below `n`, in draw order (§5.6: no `% n`).
    ///
    /// The rejection of a repeat is what makes the draw order *the* draw order:
    /// a "shuffle then take k" would consume a different number of draws.
    pub fn distinct_below(&mut self, k: u32, n: u32) -> Vec<u32> {
        assert!(k <= n, "cannot draw {k} distinct indices below {n}");
        let mut picked: Vec<u32> = Vec::with_capacity(k as usize);
        while picked.len() < k as usize {
            let index = self.bounded(n);
            if !picked.contains(&index) {
                picked.push(index);
            }
        }
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.6's published vectors, so a port proves itself before it plays a game.
    #[test]
    fn the_published_vectors_hold() {
        let mut seed0 = SplitMix64::new(0);
        let draws: Vec<u64> = (0..4).map(|_| seed0.draw()).collect();
        assert_eq!(
            draws,
            vec![
                0xe220_a839_7b1d_cdaf,
                0x6e78_9e6a_a1b9_65f4,
                0x06c4_5d18_8009_454f,
                0xf88b_b8a8_724c_81ec,
            ]
        );
        let mut seed42 = SplitMix64::new(42);
        let draws: Vec<u64> = (0..4).map(|_| seed42.draw()).collect();
        assert_eq!(
            draws,
            vec![
                0xbdd7_3226_2feb_6e95,
                0x28ef_e333_b266_f103,
                0x4752_6757_130f_9f52,
                0x581c_e1ff_0e4a_e394,
            ]
        );
    }

    #[test]
    fn bounded_stays_inside_its_range() {
        let mut rng = SplitMix64::tune_stream(0);
        for n in [1u32, 2, 7, 10, 800, 1_000] {
            for _ in 0..500 {
                assert!(rng.bounded(n) < n);
            }
        }
    }

    #[test]
    fn the_normal_is_centred_and_unit_variance() {
        let mut rng = SplitMix64::bootstrap_stream(3);
        let n = 20_000;
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        for _ in 0..n {
            let z = rng.normal();
            assert!((-6.0..=6.0).contains(&z));
            sum += z;
            sum_sq += z * z;
        }
        let mean = sum / n as f64;
        let variance = sum_sq / n as f64 - mean * mean;
        assert!(mean.abs() < 0.05, "mean {mean}");
        assert!((variance - 1.0).abs() < 0.05, "variance {variance}");
    }

    #[test]
    fn distinct_below_draws_no_repeat() {
        let mut rng = SplitMix64::tune_stream(11);
        for _ in 0..200 {
            let picked = rng.distinct_below(8, 800);
            assert_eq!(picked.len(), 8);
            let mut sorted = picked.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), 8);
            assert!(picked.iter().all(|index| *index < 800));
        }
    }
}
