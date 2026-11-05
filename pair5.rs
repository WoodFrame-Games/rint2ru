use std::arch::x86_64::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

const CHUNKS: usize = 128;
const LEN: usize = 60;
const NEWD: usize = CHUNKS * LEN;
const S: usize = 32 + NEWD;
const PMAX: usize = S - 24;

static DIGITS: AtomicU64 = AtomicU64::new(0);
static P1HITS: AtomicU64 = AtomicU64::new(0);
static FOUND: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
static GAP: AtomicU64 = AtomicU64::new(0);
static STOP_AT: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn splitmix(mut s: u64) -> u64 {
    s = s.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = s;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
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

#[inline(always)]
unsafe fn mulhi32(a: __m256i, b: __m256i) -> __m256i {
    let e = _mm256_mul_epu32(a, b);
    let o = _mm256_mul_epu32(_mm256_srli_epi64::<32>(a), _mm256_srli_epi64::<32>(b));
    let he = _mm256_srli_epi64::<32>(e);
    let ho = _mm256_slli_epi64::<32>(_mm256_srli_epi64::<32>(o));
    _mm256_blend_epi32::<0xAA>(he, ho)
}

#[inline(always)]
unsafe fn gen16(st: &mut __m256i, out: *mut u8) {
    let mut x = *st;
    x = _mm256_xor_si256(x, _mm256_srli_epi32::<13>(x));
    x = _mm256_xor_si256(x, _mm256_slli_epi32::<17>(x));
    x = _mm256_xor_si256(x, _mm256_srli_epi32::<5>(x));
    *st = x;
    let q100 = _mm256_srli_epi32::<5>(mulhi32(x, _mm256_set1_epi32(0x51EB851F)));
    let r = _mm256_sub_epi32(x, _mm256_mullo_epi32(q100, _mm256_set1_epi32(100)));
    let t = _mm256_srli_epi32::<11>(_mm256_mullo_epi32(r, _mm256_set1_epi32(205)));
    let u = _mm256_sub_epi32(r, _mm256_mullo_epi32(t, _mm256_set1_epi32(10)));
    let v = _mm256_or_si256(
        _mm256_add_epi32(t, _mm256_set1_epi32(b'0' as i32)),
        _mm256_slli_epi32::<8>(_mm256_add_epi32(u, _mm256_set1_epi32(b'0' as i32))),
    );
    let sh = _mm256_setr_epi8(
        0, 1, 4, 5, 8, 9, 12, 13, -1, -1, -1, -1, -1, -1, -1, -1,
        0, 1, 4, 5, 8, 9, 12, 13, -1, -1, -1, -1, -1, -1, -1, -1,
    );
    let p = _mm256_shuffle_epi8(v, sh);
    let g = _mm256_permute4x64_epi64::<0x08>(p);
    _mm_storeu_si128(out as *mut __m128i, _mm256_castsi256_si128(g));
}

#[inline(always)]
unsafe fn ld64(p: *const u8) -> u64 {
    std::ptr::read_unaligned(p as *const u64)
}
#[inline(always)]
unsafe fn ld16(p: *const u8) -> u16 {
    std::ptr::read_unaligned(p as *const u16)
}

unsafe fn worker(seed: u64, p1lo: u64, p1hi: u16, p2: u64, gapmax: usize) {
    let mut rng = Rng(seed);
    let mut st = _mm256_setr_epi32(
        rng.next() as i32, rng.next() as i32, rng.next() as i32, rng.next() as i32,
        rng.next() as i32, rng.next() as i32, rng.next() as i32, rng.next() as i32,
    );
    let mut buf = [0u8; S + 32];
    for i in 0..32 {
        buf[i] = b'0';
    }
    let ones = _mm256_set1_epi8(b'1' as i8);
    let base_ptr = buf.as_mut_ptr();

    loop {
        let mut k = 0;
        while k < NEWD {
            gen16(&mut st, base_ptr.add(32 + k));
            gen16(&mut st, base_ptr.add(32 + k + 16));
            gen16(&mut st, base_ptr.add(32 + k + 32));
            gen16(&mut st, base_ptr.add(32 + k + 48));
            buf[32 + k] = b'0' + 1 + (rng.next() % 9) as u8;
            k += LEN;
        }
        DIGITS.fetch_add(NEWD as u64, Ordering::Relaxed);
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        let mut p0 = 11;
        while p0 + 32 <= S {
            let v = _mm256_loadu_si256(base_ptr.add(p0) as *const __m256i);
            let mut m = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v, ones)) as u32;
            while m != 0 {
                let bit = m.trailing_zeros() as usize;
                m &= m - 1;
                let p = p0 + bit;
                if p > PMAX {
                    break;
                }
                if ld64(base_ptr.add(p)) != p1lo {
                    continue;
                }
                if ld16(base_ptr.add(p + 8)) != p1hi {
                    continue;
                }
                P1HITS.fetch_add(1, Ordering::Relaxed);
                for g in 0..=gapmax {
                    if (ld64(base_ptr.add(p + 10 + g)) & 0xFFFFFFFFFFFF) != p2 {
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
            p0 += 32;
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

    let threads = std::env::var("THREADS").ok().and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2));
    println!("RUST: пара привет+друг, зазор <= {} | {} потока | {:.0} сек", gapmax, threads, runtime);
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
    println!("\n========== РЕЗУЛЬТАТ (Rust) ==========");
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
    let _ = STOP_AT;
}