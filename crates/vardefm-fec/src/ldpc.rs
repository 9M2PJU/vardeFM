//! Systematic IRA (irregular repeat-accumulate) LDPC codes.
//!
//! The parity-check matrix is `H = [Hu | Hp]`:
//! - `Hp` (M x M): a staircase. Check `i` contains parity bits `i` and `i-1`.
//! - `Hu` (M x K): built by progressive edge growth (PEG) over the whole graph,
//!   staircase included, so short cycles are avoided. Information bits have one of
//!   two degrees ([`info_degrees`]). See [`Ldpc::new`] for the exact algorithm.
//!
//! Encoding is a running XOR. Codeword = information bits followed by parity bits.
//! Decoding is layered normalized min-sum.

use vardefm_dsp::prng::Prng;

const ALPHA: f32 = 0.75;
const LLR_CLAMP: f32 = 40.0;
/// Degree of the high-degree information bits.
const HIGH_DEGREE: usize = 8;

#[derive(Clone, Debug)]
pub struct Ldpc {
    pub n: usize,
    pub k: usize,
    /// For each check: the information-bit variables it contains.
    info_vars: Vec<Vec<u32>>,
    /// Flattened adjacency of every check (information and parity variables).
    check_ptr: Vec<usize>,
    check_vars: Vec<u32>,
}

impl Ldpc {
    /// Build the (n, k) code from `seed`.
    ///
    /// Construction (normative, both ends must match; docs/PROTOCOL.md section 2.4):
    /// 1. `m = n - k`. Information bit `j` gets degree `d_j` from [`info_degrees`].
    /// 2. Start from the staircase alone: parity bit `p` is in checks `p` and `p + 1`
    ///    (the last parity bit only in check `m - 1`). A check's degree counts both
    ///    kinds of edges, so check 0 starts at 1 and every other check at 2.
    /// 3. With `rng = Prng::new(seed)`, for `j` in `0..k`, add `d_j` edges to bit `j`,
    ///    one at a time. For each edge, let `S` be the checks at the greatest distance
    ///    from bit `j` in the current graph if every check can be reached from `j`,
    ///    else the checks that cannot be reached (all `m` checks for the first edge).
    ///    Keep the checks of `S` with the lowest current degree, in ascending order,
    ///    and connect `j` to the one at index `rng.next_u32() mod (number kept)`.
    pub fn new(n: usize, k: usize, seed: u32) -> Self {
        assert!(k > 0 && n > k);
        let m = n - k;
        let mut info_vars = peg(m, &info_degrees(n, k), seed);
        for v in &mut info_vars {
            v.sort_unstable();
        }
        let mut check_ptr = Vec::with_capacity(m + 1);
        let mut check_vars = Vec::new();
        check_ptr.push(0);
        for (c, iv) in info_vars.iter().enumerate() {
            check_vars.extend_from_slice(iv);
            check_vars.push((k + c) as u32);
            if c > 0 {
                check_vars.push((k + c - 1) as u32);
            }
            check_ptr.push(check_vars.len());
        }
        Self { n, k, info_vars, check_ptr, check_vars }
    }

    pub fn m(&self) -> usize {
        self.n - self.k
    }

    /// Encode `k` information bits into an `n`-bit codeword.
    pub fn encode(&self, info: &[u8]) -> Vec<u8> {
        assert_eq!(info.len(), self.k);
        let mut cw = Vec::with_capacity(self.n);
        cw.extend_from_slice(info);
        let mut prev = 0u8;
        for iv in &self.info_vars {
            let s = iv.iter().fold(0u8, |acc, &v| acc ^ info[v as usize]);
            prev ^= s;
            cw.push(prev);
        }
        cw
    }

    /// True if `bits` (length n) satisfies every parity check.
    pub fn check(&self, bits: &[u8]) -> bool {
        (0..self.m()).all(|c| {
            self.check_vars[self.check_ptr[c]..self.check_ptr[c + 1]].iter().fold(0u8, |acc, &v| acc ^ bits[v as usize])
                == 0
        })
    }

    /// Decode channel LLRs (positive = bit 0). Returns the `k` information bits
    /// if a valid codeword was found within `max_iter` iterations.
    pub fn decode(&self, llr: &[f32], max_iter: usize) -> Option<Vec<u8>> {
        assert_eq!(llr.len(), self.n);
        let mut total: Vec<f32> = llr.iter().map(|l| l.clamp(-LLR_CLAMP, LLR_CLAMP)).collect();
        let mut r = vec![0f32; self.check_vars.len()];
        let mut q = Vec::with_capacity(64);
        let mut hard = vec![0u8; self.n];

        for _ in 0..max_iter {
            for c in 0..self.m() {
                let (a, b) = (self.check_ptr[c], self.check_ptr[c + 1]);
                q.clear();
                let (mut min1, mut min2, mut imin) = (f32::INFINITY, f32::INFINITY, 0);
                let mut neg = false;
                for e in a..b {
                    let v = total[self.check_vars[e] as usize] - r[e];
                    let mag = v.abs();
                    if mag < min1 {
                        min2 = min1;
                        min1 = mag;
                        imin = e;
                    } else if mag < min2 {
                        min2 = mag;
                    }
                    neg ^= v < 0.0;
                    q.push(v);
                }
                for (i, e) in (a..b).enumerate() {
                    let mag = ALPHA * if e == imin { min2 } else { min1 };
                    let sign_neg = neg ^ (q[i] < 0.0);
                    let new = if sign_neg { -mag } else { mag };
                    r[e] = new;
                    total[self.check_vars[e] as usize] = q[i] + new;
                }
            }
            for (h, t) in hard.iter_mut().zip(&total) {
                *h = (*t < 0.0) as u8;
            }
            if self.check(&hard) {
                return Some(hard[..self.k].to_vec());
            }
        }
        None
    }
}

/// Degree of every information bit of the (n, k) code, `m = n - k`:
/// the first `k - h` bits have degree `max(3, ceil(3m / 2k))`, the last
/// `h = min(floor((4k + 5m) / 20), floor(k / 2))` bits have degree 8. Both are capped
/// at `m`. Payload codes (n = 2400) get `h` = 540, 520, 510 and 500 bits of degree 8
/// for k = 1200, 1600, 1800 and 2000, the rest degree 3; the header code (400, 96)
/// gets 48 bits of degree 5 and 48 of degree 8.
pub fn info_degrees(n: usize, k: usize) -> Vec<usize> {
    let m = n - k;
    let high = ((4 * k + 5 * m) / 20).min(k / 2);
    let low = (3 * m).div_ceil(2 * k).max(3);
    let mut d = vec![low.min(m); k - high];
    d.resize(k, HIGH_DEGREE.min(m));
    d
}

/// Progressive edge growth (see [`Ldpc::new`], step 3). Returns, for each check, its
/// information-bit neighbours (unsorted).
///
/// Distances are found by a breadth-first search over checks only: two checks are
/// neighbours when they share a variable node (information or parity bit), so a check
/// at depth `d` is `2d + 1` edges from bit `j`. Sets of checks are bit sets;
/// when fewer checks are left unreached than the current level holds, each of them is
/// tested against the level instead of expanding the level (same result, faster).
fn peg(m: usize, degrees: &[usize], seed: u32) -> Vec<Vec<u32>> {
    let k = degrees.len();
    let mut rng = Prng::new(seed);
    let mut var_checks: Vec<Vec<u32>> = degrees.iter().map(|&d| Vec::with_capacity(d.min(m))).collect();
    let mut check_info: Vec<Vec<u32>> = vec![Vec::new(); m];
    let mut cdeg: Vec<u32> = (0..m).map(|c| if c == 0 { 1 } else { 2 }).collect();
    // The search runs on checks only, with sets of checks as bit sets of `w` words.
    // nb[c] holds the checks that share a variable node (information or parity) with c.
    let w = m.div_ceil(64);
    let mut nb = vec![0u64; m * w];
    let set = |bits: &mut [u64], c: usize| bits[c / 64] |= 1 << (c % 64);
    for c in 1..m {
        set(&mut nb[c * w..], c - 1);
        set(&mut nb[(c - 1) * w..], c);
    }
    // Mask of the bits of word i that stand for checks.
    let valid = |i: usize| if i + 1 == w && !m.is_multiple_of(64) { (1u64 << (m % 64)) - 1 } else { u64::MAX };
    let (mut reached, mut level, mut next) = (vec![0u64; w], vec![0u64; w], vec![0u64; w]);
    let mut cand = Vec::new();
    for j in 0..k {
        for _ in 0..degrees[j].min(m) {
            // Breadth-first search from bit j, one depth at a time. Afterwards `next`
            // holds the deepest level if `deepest`, else `reached` misses some checks.
            reached.fill(0);
            for &c in &var_checks[j] {
                set(&mut reached, c as usize);
            }
            level.copy_from_slice(&reached);
            let mut size = var_checks[j].len();
            let mut count = size;
            let mut deepest = false;
            while size > 0 {
                next.fill(0);
                if m - count < size {
                    // Few checks left: test each of them against the current level.
                    for (i, (x, &r)) in next.iter_mut().zip(&reached).enumerate() {
                        let mut left = !r & valid(i);
                        while left != 0 {
                            let u = i * 64 + left.trailing_zeros() as usize;
                            left &= left - 1;
                            if nb[u * w..(u + 1) * w].iter().zip(&level).any(|(a, b)| a & b != 0) {
                                *x |= 1 << (u % 64);
                            }
                        }
                    }
                } else {
                    for (i, &lv) in level.iter().enumerate() {
                        let mut bits = lv;
                        while bits != 0 {
                            let c = i * 64 + bits.trailing_zeros() as usize;
                            bits &= bits - 1;
                            next.iter_mut().zip(&nb[c * w..(c + 1) * w]).for_each(|(x, &y)| *x |= y);
                        }
                    }
                    next.iter_mut().zip(&reached).for_each(|(x, &r)| *x &= !r);
                }
                size = next.iter().map(|x| x.count_ones() as usize).sum();
                reached.iter_mut().zip(&next).for_each(|(r, &x)| *r |= x);
                count += size;
                if count == m {
                    deepest = true;
                    break;
                }
                std::mem::swap(&mut level, &mut next);
            }
            // Candidates, in ascending order: the deepest level, or the checks not reached.
            cand.clear();
            let mut best = u32::MAX;
            for (i, (&x, &r)) in next.iter().zip(&reached).enumerate() {
                let mut bits = if deepest { x } else { !r & valid(i) };
                while bits != 0 {
                    let c = i * 64 + bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    if cdeg[c] < best {
                        best = cdeg[c];
                        cand.clear();
                    }
                    if cdeg[c] == best {
                        cand.push(c);
                    }
                }
            }
            let c = cand[rng.next_u32() as usize % cand.len()];
            for &c2 in &var_checks[j] {
                set(&mut nb[c * w..], c2 as usize);
                set(&mut nb[c2 as usize * w..], c);
            }
            var_checks[j].push(c as u32);
            check_info[c].push(j as u32);
            cdeg[c] += 1;
        }
    }
    check_info
}

/// The codes the modem uses (`vardefm-modem`): header, then payload k = 1200..2000.
#[cfg(test)]
const CODES: [(usize, usize, u32); 5] = [
    (400, 96, 0x1d9c_1000),
    (2400, 1200, 0x1d9c_0000 + 1200),
    (2400, 1600, 0x1d9c_0000 + 1600),
    (2400, 1800, 0x1d9c_0000 + 1800),
    (2400, 2000, 0x1d9c_0000 + 2000),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// BPSK through AWGN at the given Es/N0; returns LLRs = 2y/sigma^2.
    fn awgn_bpsk(cw: &[u8], esn0_db: f32, seed: u32) -> Vec<f32> {
        let sigma = (0.5 * 10f32.powf(-esn0_db / 10.0)).sqrt();
        let mut rng = Prng::new(seed);
        let mut u = || (rng.next_u32() as f32 + 1.0) / (u32::MAX as f32 + 2.0);
        cw.iter()
            .map(|&b| {
                let n = (-2.0 * u().ln()).sqrt() * (2.0 * std::f32::consts::PI * u()).cos();
                let y = if b == 0 { 1.0 } else { -1.0 } + sigma * n;
                2.0 * y / (sigma * sigma)
            })
            .collect()
    }

    #[test]
    fn encode_satisfies_checks() {
        for (n, k) in [(2400, 1200), (2400, 1600), (2400, 1800), (2400, 2000), (400, 96)] {
            let code = Ldpc::new(n, k, 1);
            let info = Prng::new(9).bits(k);
            let cw = code.encode(&info);
            assert!(code.check(&cw), "({n},{k})");
            let llr: Vec<f32> = cw.iter().map(|&b| if b == 0 { 4.0 } else { -4.0 }).collect();
            assert_eq!(code.decode(&llr, 5).unwrap(), info);
        }
    }

    #[test]
    fn corrects_noise() {
        // Rate 1/2 BPSK at Eb/N0 = 2.0 dB (Es/N0 = -1.0 dB): FER is about 0.5%.
        let code = Ldpc::new(2400, 1200, 0x1d9c_0000 + 1200);
        let mut ok = 0;
        for t in 0..20 {
            let info = Prng::new(100 + t).bits(1200);
            let cw = code.encode(&info);
            let llr = awgn_bpsk(&cw, 2.0 + 10.0 * 0.5f32.log10(), 7 + t);
            if code.decode(&llr, 50).as_deref() == Some(&info[..]) {
                ok += 1;
            }
        }
        assert!(ok >= 19, "decoded {ok}/20");
    }

    #[test]
    fn graph_has_profile_and_no_4_cycles() {
        for (n, k, seed) in CODES {
            let code = Ldpc::new(n, k, seed);
            let m = code.m();
            let mut checks_of = vec![Vec::new(); k];
            for (c, iv) in code.info_vars.iter().enumerate() {
                for &v in iv {
                    checks_of[v as usize].push(c);
                }
            }
            let deg: Vec<usize> = checks_of.iter().map(Vec::len).collect();
            assert_eq!(deg, info_degrees(n, k), "({n},{k})");
            // An information bit in checks c and c + 1 would close a 4-cycle with parity bit c.
            for cs in &checks_of {
                assert!(cs.windows(2).all(|w| w[1] > w[0] + 1), "({n},{k}): adjacent checks");
            }
            // Two information bits may share at most one check.
            let mut pairs = std::collections::HashSet::new();
            for iv in &code.info_vars {
                for (i, &a) in iv.iter().enumerate() {
                    for &b in &iv[i + 1..] {
                        assert!(pairs.insert((a, b)), "({n},{k}): bits {a} and {b} share two checks");
                    }
                }
            }
            let cdeg: Vec<usize> = (1..m).map(|c| code.check_ptr[c + 1] - code.check_ptr[c]).collect();
            let (lo, hi) = (cdeg.iter().min().unwrap(), cdeg.iter().max().unwrap());
            assert!(hi - lo <= 2, "({n},{k}): check degrees {lo}..{hi}");
        }
    }

    /// The construction is part of the protocol: any change to it breaks
    /// interoperability, so pin a hash of every code the modem uses.
    #[test]
    fn construction_is_stable() {
        let mut got = Vec::new();
        for (n, k, seed) in CODES {
            let code = Ldpc::new(n, k, seed);
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for v in code.check_ptr.iter().map(|&p| p as u32).chain(code.check_vars.iter().copied()) {
                h = (h ^ v as u64).wrapping_mul(0x0100_0000_01b3);
            }
            got.push(h);
        }
        let want = [
            0xe396_c10e_22fe_3079,
            0x6a2b_d917_f607_4504,
            0x5b2b_815c_a332_d0a6,
            0x949f_456c_abe3_8f31,
            0xcfb5_fd93_5694_16b2,
        ];
        assert_eq!(got, want, "{got:#018x?}");
    }
}

#[cfg(test)]
mod curve {
    use super::*;
    use std::time::Instant;

    /// Frame error rate of `code` at `ebn0_db` (BPSK, AWGN, 50 iterations).
    fn fer(code: &Ldpc, ebn0_db: f32, trials: u32) -> f32 {
        let rate = code.k as f32 / code.n as f32;
        let esn0 = ebn0_db + 10.0 * rate.log10();
        let sigma = (0.5 * 10f32.powf(-esn0 / 10.0)).sqrt();
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()) as u32;
        let fails: u32 = std::thread::scope(|s| {
            let workers: Vec<_> = (0..threads)
                .map(|th| {
                    s.spawn(move || {
                        let mut fail = 0;
                        for t in (th..trials).step_by(threads as usize) {
                            let info = Prng::new(1 + t).bits(code.k);
                            let cw = code.encode(&info);
                            let mut rng = Prng::new(1000 + t);
                            let mut u = || (rng.next_u32() as f32 + 1.0) / (u32::MAX as f32 + 2.0);
                            let llr: Vec<f32> = cw
                                .iter()
                                .map(|&b| {
                                    let n = (-2.0 * u().ln()).sqrt() * (2.0 * std::f32::consts::PI * u()).cos();
                                    let y = if b == 0 { 1.0 } else { -1.0 } + sigma * n;
                                    2.0 * y / (sigma * sigma)
                                })
                                .collect();
                            if code.decode(&llr, 50).as_deref() != Some(&info[..]) {
                                fail += 1;
                            }
                        }
                        fail
                    })
                })
                .collect();
            workers.into_iter().map(|w| w.join().unwrap()).sum()
        });
        fails as f32 / trials as f32
    }

    /// Prints build time and frame error rate vs Eb/N0 for every code (BPSK, AWGN),
    /// 200 frames per point (or `TRIALS`) in 0.25 dB steps from 0.5 dB; a curve ends
    /// after two error-free points in a row.
    /// `cargo test --release -p vardefm-fec fer_curve -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn fer_curve() {
        let trials = std::env::var("TRIALS").map_or(200, |t| t.parse().unwrap());
        let mut total = 0.0;
        for (n, k, seed) in CODES {
            let t0 = Instant::now();
            let code = Ldpc::new(n, k, seed);
            let built = t0.elapsed().as_secs_f64() * 1e3;
            total += built;
            println!("({n},{k}) rate {:.2}, built in {built:.1} ms:", k as f32 / n as f32);
            let mut line = String::new();
            let mut clean = 0;
            for i in 0..=16 {
                let ebn0 = 0.5 + 0.25 * i as f32;
                let f = fer(&code, ebn0, trials);
                line += &format!(" {ebn0:.2}:{f:.3}");
                clean = if f == 0.0 { clean + 1 } else { 0 };
                if clean == 2 {
                    break;
                }
            }
            println!("  {line}");
        }
        println!("all codes built in {total:.1} ms");
    }
}
