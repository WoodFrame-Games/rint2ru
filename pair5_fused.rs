// Fused-ядро: генерация и 4-байтный SIMD-фильтр в регистрах, без буфера и
// повторного чтения. Блок = 64 цифры (16 x u32). Фильтр для блока prev
// использует cur (следующий блок) для позиций 61..63 через vpermi2b.
//
// Ограничение: зазор <= 49 цифр (проверка пары читает prev+cur = 128 байт).
// Для больших зазоров используйте pair5_gen4_best64.

use std::arch::x86_64::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

const BLOCK: usize = 64;
const MAX_GAP: usize = 49;
// filt3: 3-байтный фильтр (2 vpermi2b вместо 3), больше кандидатов, меньше permute
const FILT3: bool = cfg!(filt3);
// wordfilter: искать "1718" по упакованным 32-битным словам (vpermi2d + сдвиги),
// а не тремя vpermi2b по байтам.
const WORD: bool = cfg!(wordfilter);
// pipe4: 4 блока за итерацию (больше ILP, но выше давление на регистры)
const PIPE4: bool = cfg!(pipe4);
const PIPE8: bool = cfg!(pipe8);
const PIPE6: bool = cfg!(pipe6);
// aesrng: counter-based VAES вместо xorshift64.
// ВАЖНО: 1-2 раунда AES статистически плохи (chi2/df 28917 и 1.72),
// 3 раунда дают chi2/df=0.986 и проходят проверки.
const AESRNG: bool = cfg!(aesrng);
const AES_ROUNDS: usize = if cfg!(aes4) {
    4
} else if cfg!(aes3) {
    3
} else if cfg!(aes2) {
    2
} else {
    1
};

static DIGITS: AtomicU64 = AtomicU64::new(0);
static P1HITS: AtomicU64 = AtomicU64::new(0);
static FOUND: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
static GAP: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
struct CpuSet {
    bits: [u64; 16],
}
extern "C" {
    fn sched_setaffinity(pid: i32, cpusetsize: usize, mask: *const CpuSet) -> i32;
}
#[inline]
fn pin_cpu(cpu: usize) {
    unsafe {
        let mut set = CpuSet { bits: [0u64; 16] };
        set.bits[cpu / 64] |= 1u64 << (cpu % 64);
        sched_setaffinity(0, std::mem::size_of::<CpuSet>(), &set);
    }
}

struct Rng(u64);
impl Rng {
    #[inline(always)]
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
}

const fn make_idx(off: usize) -> [i8; 64] {
    let mut a = [0i8; 64];
    let mut i = 0;
    while i < 64 {
        a[i] = (i + off) as i8;
        i += 1;
    }
    a
}
static IDX1: [i8; 64] = make_idx(1);
static IDX2: [i8; 64] = make_idx(2);
static IDX3: [i8; 64] = make_idx(3);

// следующие 32-битные лейны: [1..15, 16] (16 = lane 0 из cur)
static IDXW: [i32; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
const PW: i32 = 0x38313731u32 as i32; // "1718" little-endian

#[inline(always)]
unsafe fn mulhi32(a: __m512i, b: __m512i) -> __m512i {
    let e = _mm512_mul_epu32(a, b);
    let o = _mm512_mul_epu32(_mm512_srli_epi64::<32>(a), _mm512_srli_epi64::<32>(b));
    let he = _mm512_srli_epi64::<32>(e);
    let ho = _mm512_slli_epi64::<32>(_mm512_srli_epi64::<32>(o));
    _mm512_mask_blend_epi32(0xAAAA, he, ho)
}

// 16 x u32 -> 64 десятичных цифр в одном ZMM (без store).
#[inline(always)]
unsafe fn digits256(x: __m512i) -> __m512i {
    let q = _mm512_srli_epi32::<13>(mulhi32(x, _mm512_set1_epi32(3518437209u32 as i32)));
    let r = _mm512_sub_epi32(x, _mm512_mullo_epi32(q, _mm512_set1_epi32(10000)));
    let h2 = _mm512_srli_epi32::<19>(_mm512_mullo_epi32(r, _mm512_set1_epi32(5243)));
    let l2 = _mm512_sub_epi32(r, _mm512_mullo_epi32(h2, _mm512_set1_epi32(100)));

    let d1 = _mm512_srli_epi32::<11>(_mm512_mullo_epi32(h2, _mm512_set1_epi32(205)));
    let d2 = _mm512_sub_epi32(h2, _mm512_mullo_epi32(d1, _mm512_set1_epi32(10)));
    let d3 = _mm512_srli_epi32::<11>(_mm512_mullo_epi32(l2, _mm512_set1_epi32(205)));
    let d4 = _mm512_sub_epi32(l2, _mm512_mullo_epi32(d3, _mm512_set1_epi32(10)));

    let w = _mm512_or_si512(
        _mm512_or_si512(d1, _mm512_slli_epi32::<8>(d2)),
        _mm512_or_si512(_mm512_slli_epi32::<16>(d3), _mm512_slli_epi32::<24>(d4)),
    );
    _mm512_add_epi32(w, _mm512_set1_epi32(0x30303030))
}

// xorshift64 на 8 лейнах (период 2^64-1)
#[inline(always)]
unsafe fn gen_block(st: &mut __m512i) -> __m512i {
    let mut x = *st;
    x = _mm512_xor_si512(x, _mm512_slli_epi64::<13>(x));
    x = _mm512_xor_si512(x, _mm512_srli_epi64::<7>(x));
    x = _mm512_xor_si512(x, _mm512_slli_epi64::<17>(x));
    *st = x;
    digits256(x)
}

// counter-based VAES: ctr += 8 на все 64-битные лейны, затем AES_ROUNDS раундов
#[inline(always)]
unsafe fn gen_block_aes(ctr: &mut __m512i, key: __m512i) -> __m512i {
    *ctr = _mm512_add_epi64(*ctr, _mm512_set1_epi64(8));
    let mut x = *ctr;
    let mut r = 0;
    while r < AES_ROUNDS {
        x = _mm512_aesenc_epi128(x, key);
        r += 1;
    }
    digits256(x)
}

#[inline(always)]
unsafe fn ld64(p: *const u8) -> u64 {
    std::ptr::read_unaligned(p as *const u64)
}
#[inline(always)]
unsafe fn ld32(p: *const u8) -> u32 {
    std::ptr::read_unaligned(p as *const u32)
}
#[inline(always)]
unsafe fn ld16(p: *const u8) -> u16 {
    std::ptr::read_unaligned(p as *const u16)
}

#[inline(always)]
unsafe fn verify_at(
    base: *const u8,
    p: usize,
    p1lo: u64,
    p1hi: u16,
    p2lo: u32,
    p2hi: u16,
    gapmax: usize,
) {
    if ld64(base.add(p)) != p1lo {
        return;
    }
    if ld16(base.add(p + 8)) != p1hi {
        return;
    }
    P1HITS.fetch_add(1, Ordering::Relaxed);
    for g in 0..=gapmax {
        let off = p + 10 + g;
        if ld32(base.add(off)) != p2lo {
            continue;
        }
        if ld16(base.add(off + 4)) != p2hi {
            continue;
        }
        if !FOUND.swap(true, Ordering::SeqCst) {
            GAP.store(g as u64, Ordering::Relaxed);
            STOP.store(true, Ordering::Relaxed);
        }
        break;
    }
}

#[inline(always)]
unsafe fn scan(
    prev: __m512i,
    cur: __m512i,
    c0: __m512i,
    c1: __m512i,
    c2: __m512i,
    c3: __m512i,
    idx1: __m512i,
    idx2: __m512i,
    idx3: __m512i,
    p1lo: u64,
    p1hi: u16,
    p2lo: u32,
    p2hi: u16,
    gapmax: usize,
) {
    let s1 = _mm512_permutex2var_epi8(prev, idx1, cur);
    let s2 = _mm512_permutex2var_epi8(prev, idx2, cur);
    let m = _mm512_cmpeq_epi8_mask(prev, c0)
        & _mm512_cmpeq_epi8_mask(s1, c1)
        & _mm512_cmpeq_epi8_mask(s2, c2);
    let m = if FILT3 {
        m
    } else {
        let s3 = _mm512_permutex2var_epi8(prev, idx3, cur);
        m & _mm512_cmpeq_epi8_mask(s3, c3)
    };
    if m == 0 {
        return;
    }

    let mut scratch = [0u8; 128];
    let sp = scratch.as_mut_ptr();
    _mm512_storeu_si512(sp as *mut __m512i, prev);
    _mm512_storeu_si512(sp.add(64) as *mut __m512i, cur);

    let mut mm = m;
    while mm != 0 {
        let p = mm.trailing_zeros() as usize;
        mm &= mm - 1;
        if ld64(sp.add(p)) != p1lo {
            continue;
        }
        if ld16(sp.add(p + 8)) != p1hi {
            continue;
        }
        P1HITS.fetch_add(1, Ordering::Relaxed);
        for g in 0..=gapmax {
            let off = p + 10 + g;
            if ld32(sp.add(off)) != p2lo {
                continue;
            }
            if ld16(sp.add(off + 4)) != p2hi {
                continue;
            }
            if !FOUND.swap(true, Ordering::SeqCst) {
                GAP.store(g as u64, Ordering::Relaxed);
                STOP.store(true, Ordering::Relaxed);
            }
            break;
        }
        if STOP.load(Ordering::Relaxed) {
            break;
        }
    }
}

// Фильтр по упакованным словам: каждое 32-битное слово = 4 ASCII-цифры.
// 4 возможных старта внутри слова собираются сдвигами + одним vpermi2d
// (для lane15 берётся lane0 из cur). Покрывает все позиции 0..63.
#[inline(always)]
unsafe fn scan_word(
    prev: __m512i,
    cur: __m512i,
    p1lo: u64,
    p1hi: u16,
    p2lo: u32,
    p2hi: u16,
    gapmax: usize,
) {
    let idx = _mm512_loadu_si512(IDXW.as_ptr() as *const __m512i);
    let pat = _mm512_set1_epi32(PW);
    let wn = _mm512_permutex2var_epi32(prev, idx, cur);
    let v1 = _mm512_or_si512(
        _mm512_srli_epi32::<8>(prev),
        _mm512_slli_epi32::<24>(wn),
    );
    let v2 = _mm512_or_si512(
        _mm512_srli_epi32::<16>(prev),
        _mm512_slli_epi32::<16>(wn),
    );
    let v3 = _mm512_or_si512(
        _mm512_srli_epi32::<24>(prev),
        _mm512_slli_epi32::<8>(wn),
    );
    let m0 = _mm512_cmpeq_epi32_mask(prev, pat);
    let m1 = _mm512_cmpeq_epi32_mask(v1, pat);
    let m2 = _mm512_cmpeq_epi32_mask(v2, pat);
    let m3 = _mm512_cmpeq_epi32_mask(v3, pat);
    if (m0 | m1 | m2 | m3) == 0 {
        return;
    }

    let mut scratch = [0u8; 128];
    let sp = scratch.as_mut_ptr();
    _mm512_storeu_si512(sp as *mut __m512i, prev);
    _mm512_storeu_si512(sp.add(64) as *mut __m512i, cur);

    for (m, start) in [(m0, 0usize), (m1, 1), (m2, 2), (m3, 3)] {
        let mut mm = m;
        while mm != 0 {
            let lane = mm.trailing_zeros() as usize;
            mm &= mm - 1;
            let p = 4 * lane + start;
            verify_at(sp, p, p1lo, p1hi, p2lo, p2hi, gapmax);
            if STOP.load(Ordering::Relaxed) {
                break;
            }
        }
    }
}

unsafe fn worker(seed: u64, p1lo: u64, p1hi: u16, p2lo: u32, p2hi: u16, gapmax: usize) {
    let mut rng = Rng(seed);
    let mut raw0 = [0u64; 8];
    let mut raw1 = [0u64; 8];
    if AESRNG {
        let b0 = rng.next();
        let b1 = rng.next();
        for i in 0..8 {
            raw0[i] = b0.wrapping_add(i as u64);
            raw1[i] = b1.wrapping_add(i as u64);
        }
    } else {
        for v in raw0.iter_mut() {
            *v = rng.next() | 1;
        }
        for v in raw1.iter_mut() {
            *v = rng.next() | 1;
        }
    }
    let key0 = _mm512_set1_epi64(rng.next() as i64);
    let key1 = _mm512_set1_epi64(rng.next() as i64);
    let mut st0 = _mm512_loadu_si512(raw0.as_ptr() as *const __m512i);
    let mut st1 = _mm512_loadu_si512(raw1.as_ptr() as *const __m512i);

    let c0 = _mm512_set1_epi8(b'1' as i8);
    let c1 = _mm512_set1_epi8(b'7' as i8);
    let c2 = _mm512_set1_epi8(b'1' as i8);
    let c3 = _mm512_set1_epi8(b'8' as i8);
    let idx1 = _mm512_loadu_si512(IDX1.as_ptr() as *const __m512i);
    let idx2 = _mm512_loadu_si512(IDX2.as_ptr() as *const __m512i);
    let idx3 = _mm512_loadu_si512(IDX3.as_ptr() as *const __m512i);
    let genonly = std::env::var("GENONLY").is_ok();

    // block0: scan later together with block1
    let mut prev = gen_block(&mut st0);
    let mut local: u64 = 0;
    let mut iters: u32 = 0;

    if PIPE8 {
        loop {
            let a = gen_block(&mut st1);
            let b = gen_block(&mut st0);
            let c = gen_block(&mut st1);
            let d = gen_block(&mut st0);
            let e = gen_block(&mut st1);
            let f = gen_block(&mut st0);
            let g = gen_block(&mut st1);
            let h = gen_block(&mut st0);
            if genonly {
                let _ = std::hint::black_box(a);
                let _ = std::hint::black_box(b);
                let _ = std::hint::black_box(c);
                let _ = std::hint::black_box(d);
                let _ = std::hint::black_box(e);
                let _ = std::hint::black_box(f);
                let _ = std::hint::black_box(g);
                let _ = std::hint::black_box(h);
            } else {
                scan(
                    prev, a, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    a, b, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    b, c, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    c, d, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    d, e, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    e, f, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    f, g, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    g, h, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
            }
            prev = h;
            local += (8 * BLOCK) as u64;
            iters += 1;
            if iters == 16 {
                DIGITS.fetch_add(local, Ordering::Relaxed);
                local = 0;
                iters = 0;
                if STOP.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    } else if PIPE6 {
        loop {
            let a = gen_block(&mut st1);
            let b = gen_block(&mut st0);
            let c = gen_block(&mut st1);
            let d = gen_block(&mut st0);
            let e = gen_block(&mut st1);
            let f = gen_block(&mut st0);
            if genonly {
                let _ = std::hint::black_box(a);
                let _ = std::hint::black_box(b);
                let _ = std::hint::black_box(c);
                let _ = std::hint::black_box(d);
                let _ = std::hint::black_box(e);
                let _ = std::hint::black_box(f);
            } else {
                scan(
                    prev, a, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    a, b, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    b, c, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    c, d, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    d, e, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    e, f, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
            }
            prev = f;
            local += (6 * BLOCK) as u64;
            iters += 1;
            if iters == 21 {
                DIGITS.fetch_add(local, Ordering::Relaxed);
                local = 0;
                iters = 0;
                if STOP.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    } else if PIPE4 {
        loop {
            let a = if AESRNG {
                gen_block_aes(&mut st1, key1)
            } else {
                gen_block(&mut st1)
            };
            let b = if AESRNG {
                gen_block_aes(&mut st0, key0)
            } else {
                gen_block(&mut st0)
            };
            let c = if AESRNG {
                gen_block_aes(&mut st1, key1)
            } else {
                gen_block(&mut st1)
            };
            let d = if AESRNG {
                gen_block_aes(&mut st0, key0)
            } else {
                gen_block(&mut st0)
            };
            if genonly {
                let _ = std::hint::black_box(a);
                let _ = std::hint::black_box(b);
                let _ = std::hint::black_box(c);
                let _ = std::hint::black_box(d);
            } else {
                scan(
                    prev, a, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    a, b, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    b, c, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    c, d, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
            }
            prev = d;
            local += (4 * BLOCK) as u64;
            iters += 1;
            if iters == 32 {
                DIGITS.fetch_add(local, Ordering::Relaxed);
                local = 0;
                iters = 0;
                if STOP.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    } else {
        loop {
            let a = gen_block(&mut st1);
            let b = gen_block(&mut st0);
            if genonly {
                // не даём компилятору выкинуть конверсию
                let _ = std::hint::black_box(a);
                let _ = std::hint::black_box(b);
            } else if WORD {
                scan_word(prev, a, p1lo, p1hi, p2lo, p2hi, gapmax);
                scan_word(a, b, p1lo, p1hi, p2lo, p2hi, gapmax);
            } else {
                scan(
                    prev, a, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
                scan(
                    a, b, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
                );
            }
            prev = b;

            local += (2 * BLOCK) as u64;
            iters += 1;
            if iters == 64 {
                DIGITS.fetch_add(local, Ordering::Relaxed);
                local = 0;
                iters = 0;
                if STOP.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }
    DIGITS.fetch_add(local, Ordering::Relaxed);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let runtime: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(300.0);
    let gapmax: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    if gapmax > MAX_GAP {
        eprintln!(
            "fused-ядро поддерживает зазор <= {} (нужно prev+cur = 128 байт); для больших зазоров используйте pair5_gen4_best64",
            MAX_GAP
        );
        std::process::exit(2);
    }

    let p1: [u8; 10] = *b"1718103620";
    let p1lo = u64::from_le_bytes(p1[0..8].try_into().unwrap());
    let p1hi = u16::from_le_bytes(p1[8..10].try_into().unwrap());
    let p2: [u8; 6] = *b"518214";
    let p2lo = u32::from_le_bytes(p2[0..4].try_into().unwrap());
    let p2hi = u16::from_le_bytes(p2[4..6].try_into().unwrap());

    let threads = std::env::var("THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2)
        });
    let rngname = if AESRNG {
        match AES_ROUNDS {
            2 => "VAES r2",
            3 => "VAES r3",
            4 => "VAES r4",
            _ => "VAES r1",
        }
    } else {
        "xorshift64"
    };
    println!(
        "RUST AVX-512 fused: {} + скан в регистрах ({}), блок {} | пара привет+друг, зазор <= {} | {} потока | {:.0} сек",
        rngname,
        if WORD {
            "vpermi2d/word"
        } else if FILT3 {
            "vpermi2b 3B"
        } else {
            "vpermi2b 4B"
        },
        BLOCK,
        gapmax,
        threads,
        runtime
    );
    let t0 = Instant::now();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let no_pin = std::env::var("NOPIN").is_ok();
    let mut handles = Vec::new();
    for t in 0..threads {
        let s = seed ^ 0x9E3779B97F4A7C15u64.wrapping_mul(t as u64 + 1);
        handles.push(std::thread::spawn(move || unsafe {
            if !no_pin {
                pin_cpu(t);
            }
            worker(s, p1lo, p1hi, p2lo, p2hi, gapmax);
        }));
    }

    let mut nxt = 10.0;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let el = t0.elapsed().as_secs_f64();
        if el >= runtime {
            STOP.store(true, Ordering::Relaxed);
            break;
        }
        if el >= nxt {
            let d = DIGITS.load(Ordering::Relaxed);
            println!(
                "... {:.0} сек | {:.1} млрд цифр | {:.0} млн цифр/сек | приветов: {}",
                el,
                d as f64 / 1e9,
                d as f64 / el / 1e6,
                P1HITS.load(Ordering::Relaxed)
            );
            nxt += 10.0;
        }
    }
    for h in handles {
        h.join().unwrap();
    }
    let el = t0.elapsed().as_secs_f64();
    let d = DIGITS.load(Ordering::Relaxed);
    println!("\n========== РЕЗУЛЬТАТ (Rust AVX-512 fused) ==========");
    println!(
        "цифр: {:.1} млрд | время: {:.0} сек | скорость: {:.0} млн цифр/сек",
        d as f64 / 1e9,
        el,
        d as f64 / el / 1e6
    );
    println!("приветов: {}", P1HITS.load(Ordering::Relaxed));
    if FOUND.load(Ordering::Relaxed) {
        println!("НАЙДЕНО! зазор {} цифр", GAP.load(Ordering::Relaxed));
    }
}