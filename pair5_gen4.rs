use std::arch::x86_64::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

const CHUNKS: usize = 128;
const LEN: usize = if cfg!(len64) { 64 } else { 60 };
const NEWD: usize = CHUNKS * LEN;
const S: usize = 32 + NEWD;
const PMAX: usize = S - 24;

// Варианты собираются через --cfg:
//   nstate4 — 4 независимых состояния вместо 2;
//   filter4 — 4-байтный SIMD-фильтр вместо 3-байтного;
//   nolead  — не навязывать ведущую цифру 1..9 (поток становится равномерным).
const NSTATE: usize = if cfg!(nstate4) { 4 } else { 2 };
const FILTER4: bool = cfg!(filter4);
const FILTER16: bool = cfg!(filter16);
const NOLEAD: bool = cfg!(nolead);
const PAD: usize = if FILTER4 { 67 } else { 66 };

static DIGITS: AtomicU64 = AtomicU64::new(0);
static P1HITS: AtomicU64 = AtomicU64::new(0);
static FOUND: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
static GAP: AtomicU64 = AtomicU64::new(0);

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

#[inline(always)]
unsafe fn mulhi32(a: __m512i, b: __m512i) -> __m512i {
    let e = _mm512_mul_epu32(a, b);
    let o = _mm512_mul_epu32(_mm512_srli_epi64::<32>(a), _mm512_srli_epi64::<32>(b));
    let he = _mm512_srli_epi64::<32>(e);
    let ho = _mm512_slli_epi64::<32>(_mm512_srli_epi64::<32>(o));
    _mm512_mask_blend_epi32(0xAAAA, he, ho)
}

// Один вызов: xorshift64 на 8 лейнах (период 2^64-1) -> 16 x u32 ->
// по 4 десятичных цифры на u32 -> 64 байта подряд. Один store, без shuffle.
#[inline(always)]
unsafe fn gen64(st: &mut __m512i, out: *mut u8) {
    let mut x = *st;
    x = _mm512_xor_si512(x, _mm512_slli_epi64::<13>(x));
    x = _mm512_xor_si512(x, _mm512_srli_epi64::<7>(x));
    x = _mm512_xor_si512(x, _mm512_slli_epi64::<17>(x));
    *st = x;

    // x как 16 x u32: [lo0, hi0, lo1, hi1, ...]
    // q = x / 10000 (точная магия, 32-битные лейны)
    let q = _mm512_srli_epi32::<13>(mulhi32(x, _mm512_set1_epi32(3518437209u32 as i32)));
    let r = _mm512_sub_epi32(x, _mm512_mullo_epi32(q, _mm512_set1_epi32(10000)));

    // r = h2*100 + l2; r < 10000 => h2 = (r*5243)>>19 точно и дешевле mulhi
    let h2 = _mm512_srli_epi32::<19>(_mm512_mullo_epi32(r, _mm512_set1_epi32(5243)));
    let l2 = _mm512_sub_epi32(r, _mm512_mullo_epi32(h2, _mm512_set1_epi32(100)));

    let d1 = _mm512_srli_epi32::<11>(_mm512_mullo_epi32(h2, _mm512_set1_epi32(205)));
    let d2 = _mm512_sub_epi32(h2, _mm512_mullo_epi32(d1, _mm512_set1_epi32(10)));
    let d3 = _mm512_srli_epi32::<11>(_mm512_mullo_epi32(l2, _mm512_set1_epi32(205)));
    let d4 = _mm512_sub_epi32(l2, _mm512_mullo_epi32(d3, _mm512_set1_epi32(10)));

    // little-endian: d1 d2 d3 d4 в одном 32-битном слове
    let w = _mm512_or_si512(
        _mm512_or_si512(d1, _mm512_slli_epi32::<8>(d2)),
        _mm512_or_si512(_mm512_slli_epi32::<16>(d3), _mm512_slli_epi32::<24>(d4)),
    );
    let w = _mm512_add_epi32(w, _mm512_set1_epi32(0x30303030));
    _mm512_storeu_si512(out as *mut __m512i, w);
}

#[inline(always)]
unsafe fn ld64(p: *const u8) -> u64 {
    std::ptr::read_unaligned(p as *const u64)
}
#[inline(always)]
unsafe fn ld16(p: *const u8) -> u16 {
    std::ptr::read_unaligned(p as *const u16)
}
#[inline(always)]
unsafe fn ld32(p: *const u8) -> u32 {
    std::ptr::read_unaligned(p as *const u32)
}

// Проверка кандидата: привет + друг (6 цифр читаем как u32+u16).
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

unsafe fn worker(seed: u64, p1lo: u64, p1hi: u16, p2: u64, gapmax: usize) {
    let mut rng = Rng(seed);
    let mut raw = [[0u64; 8]; 4];
    for s in 0..NSTATE {
        for v in raw[s].iter_mut() {
            *v = rng.next() | 1;
        }
    }
    let mut st = [_mm512_setzero_si512(); 4];
    for s in 0..NSTATE {
        st[s] = _mm512_loadu_si512(raw[s].as_ptr() as *const __m512i);
    }

    let mut buf = [0u8; S + 32];
    for i in 0..32 {
        buf[i] = b'0';
    }
    let c0 = _mm512_set1_epi8(b'1' as i8);
    let c1 = _mm512_set1_epi8(b'7' as i8);
    let c2 = _mm512_set1_epi8(b'1' as i8);
    let c3 = _mm512_set1_epi8(b'8' as i8);
    let base_ptr = buf.as_mut_ptr();
    let genonly = std::env::var("GENONLY").is_ok();
    let p2lo = p2 as u32;
    let p2hi = (p2 >> 32) as u16;

    loop {
        let mut k = 0;
        while k < NEWD {
            if NSTATE == 2 {
                gen64(&mut st[0], base_ptr.add(32 + k));
                if !NOLEAD {
                    buf[32 + k] = b'0' + 1 + (rng.next() % 9) as u8;
                }
                gen64(&mut st[1], base_ptr.add(32 + k + LEN));
                if !NOLEAD {
                    buf[32 + k + LEN] = b'0' + 1 + (rng.next() % 9) as u8;
                }
            } else {
                gen64(&mut st[0], base_ptr.add(32 + k));
                if !NOLEAD {
                    buf[32 + k] = b'0' + 1 + (rng.next() % 9) as u8;
                }
                gen64(&mut st[1], base_ptr.add(32 + k + LEN));
                if !NOLEAD {
                    buf[32 + k + LEN] = b'0' + 1 + (rng.next() % 9) as u8;
                }
                gen64(&mut st[2], base_ptr.add(32 + k + 2 * LEN));
                if !NOLEAD {
                    buf[32 + k + 2 * LEN] = b'0' + 1 + (rng.next() % 9) as u8;
                }
                gen64(&mut st[3], base_ptr.add(32 + k + 3 * LEN));
                if !NOLEAD {
                    buf[32 + k + 3 * LEN] = b'0' + 1 + (rng.next() % 9) as u8;
                }
            }
            k += NSTATE * LEN;
        }
        DIGITS.fetch_add(NEWD as u64, Ordering::Relaxed);
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        if genonly {
            continue;
        }

        let mut p0 = 11;
        if FILTER16 {
            // 2 загрузки на 62 позиции вместо 4 на 64:
            // первая пара (17) и вторая (18) проверяются 16-битными сравнениями,
            // чётные и нечётные старты — отдельными масками (без pdep).
            let p17 = _mm512_set1_epi16(0x3731u16 as i16);
            let p18 = _mm512_set1_epi16(0x3831u16 as i16);
            while p0 + 65 <= S {
                let v0 = _mm512_loadu_si512(base_ptr.add(p0) as *const __m512i);
                let v1 = _mm512_loadu_si512(base_ptr.add(p0 + 1) as *const __m512i);
                let e17 = _mm512_cmpeq_epi16_mask(v0, p17);
                let e18 = _mm512_cmpeq_epi16_mask(v0, p18) >> 1;
                let o17 = _mm512_cmpeq_epi16_mask(v1, p17);
                let o18 = _mm512_cmpeq_epi16_mask(v1, p18) >> 1;
                let mut e = e17 & e18;
                let mut o = o17 & o18;
                while e != 0 {
                    let k = e.trailing_zeros() as usize;
                    e &= e - 1;
                    let p = p0 + 2 * k;
                    if p > PMAX {
                        break;
                    }
                    verify_at(base_ptr, p, p1lo, p1hi, p2lo, p2hi, gapmax);
                    if STOP.load(Ordering::Relaxed) {
                        break;
                    }
                }
                while o != 0 {
                    let k = o.trailing_zeros() as usize;
                    o &= o - 1;
                    let p = p0 + 2 * k + 1;
                    if p > PMAX {
                        break;
                    }
                    verify_at(base_ptr, p, p1lo, p1hi, p2lo, p2hi, gapmax);
                    if STOP.load(Ordering::Relaxed) {
                        break;
                    }
                }
                if STOP.load(Ordering::Relaxed) {
                    break;
                }
                p0 += 62;
            }
        } else {
            while p0 + PAD <= S {
                let v = _mm512_loadu_si512(base_ptr.add(p0) as *const __m512i);
                let w1 = _mm512_loadu_si512(base_ptr.add(p0 + 1) as *const __m512i);
                let w2 = _mm512_loadu_si512(base_ptr.add(p0 + 2) as *const __m512i);
                let mut m = _mm512_cmpeq_epi8_mask(v, c0)
                    & _mm512_cmpeq_epi8_mask(w1, c1)
                    & _mm512_cmpeq_epi8_mask(w2, c2);
                if FILTER4 {
                    let w3 = _mm512_loadu_si512(base_ptr.add(p0 + 3) as *const __m512i);
                    m &= _mm512_cmpeq_epi8_mask(w3, c3);
                }
                while m != 0 {
                    let bit = m.trailing_zeros() as usize;
                    m &= m - 1;
                    let p = p0 + bit;
                    if p > PMAX {
                        break;
                    }
                    verify_at(base_ptr, p, p1lo, p1hi, p2lo, p2hi, gapmax);
                    if STOP.load(Ordering::Relaxed) {
                        break;
                    }
                }
                p0 += 64;
            }
        }
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        std::ptr::copy(base_ptr.add(NEWD), base_ptr, 32);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let runtime: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(300.0);
    let gapmax: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);

    let p1: [u8; 10] = *b"1718103620";
    let p1lo = u64::from_le_bytes(p1[0..8].try_into().unwrap());
    let p1hi = u16::from_le_bytes(p1[8..10].try_into().unwrap());
    let mut p2b = [0u8; 8];
    p2b[..6].copy_from_slice(b"518214");
    let p2 = u64::from_le_bytes(p2b);

    let threads = std::env::var("THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2)
        });
    println!(
        "RUST AVX-512 gen4: 4 цифры/лейн, xorshift64, {} состояния, {}-байтный фильтр{}, чанк {} | пара привет+друг, зазор <= {} | {} потока | {:.0} сек",
        NSTATE,
        if FILTER4 { 4 } else { 3 },
        if NOLEAD { ", без ведущей" } else { "" },
        LEN,
        gapmax,
        threads,
        runtime
    );
    let t0 = Instant::now();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let mut handles = Vec::new();
    for t in 0..threads {
        let s = seed ^ 0x9E3779B97F4A7C15u64.wrapping_mul(t as u64 + 1);
        handles.push(std::thread::spawn(move || unsafe {
            worker(s, p1lo, p1hi, p2, gapmax);
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
    println!("\n========== РЕЗУЛЬТАТ (Rust AVX-512 gen4) ==========");
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