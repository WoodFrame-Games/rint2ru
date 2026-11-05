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
unsafe fn gen32(st: &mut __m512i, out: *mut u8) {
    let mut x = *st;
    x = _mm512_xor_si512(x, _mm512_srli_epi32::<13>(x));
    x = _mm512_xor_si512(x, _mm512_slli_epi32::<17>(x));
    x = _mm512_xor_si512(x, _mm512_srli_epi32::<5>(x));
    *st = x;

    let m = _mm512_set1_epi32(0x51EB851Fu32 as i32);
    let e = _mm512_mul_epu32(x, m);
    let o = _mm512_mul_epu32(_mm512_srli_epi64::<32>(x), _mm512_srli_epi64::<32>(m));
    let he = _mm512_srli_epi64::<32>(e);
    let ho = _mm512_slli_epi64::<32>(_mm512_srli_epi64::<32>(o));
    let hi = _mm512_mask_blend_epi32(0xAAAA, he, ho);

    let q100 = _mm512_srli_epi32::<5>(hi);
    let r = _mm512_sub_epi32(x, _mm512_mullo_epi32(q100, _mm512_set1_epi32(100)));
    let t = _mm512_srli_epi32::<11>(_mm512_mullo_epi32(r, _mm512_set1_epi32(205)));
    let u = _mm512_sub_epi32(r, _mm512_mullo_epi32(t, _mm512_set1_epi32(10)));
    let v = _mm512_or_si512(
        _mm512_add_epi32(t, _mm512_set1_epi32(b'0' as i32)),
        _mm512_slli_epi32::<8>(_mm512_add_epi32(u, _mm512_set1_epi32(b'0' as i32))),
    );
    static SHB: [i8; 64] = [
        0, 1, 4, 5, 8, 9, 12, 13, -1, -1, -1, -1, -1, -1, -1, -1,
        0, 1, 4, 5, 8, 9, 12, 13, -1, -1, -1, -1, -1, -1, -1, -1,
        0, 1, 4, 5, 8, 9, 12, 13, -1, -1, -1, -1, -1, -1, -1, -1,
        0, 1, 4, 5, 8, 9, 12, 13, -1, -1, -1, -1, -1, -1, -1, -1,
    ];
    static IDXB: [i64; 8] = [0, 2, 4, 6, 0, 2, 4, 6];
    let sh = _mm512_loadu_si512(SHB.as_ptr() as *const __m512i);
    let p = _mm512_shuffle_epi8(v, sh);
    let idx = _mm512_loadu_si512(IDXB.as_ptr() as *const __m512i);
    let g = _mm512_permutexvar_epi64(idx, p);
    _mm256_storeu_si256(out as *mut __m256i, _mm512_castsi512_si256(g));
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
    let mut init = [0i32; 16];
    for v in init.iter_mut() {
        *v = rng.next() as i32;
    }
    let mut st = _mm512_loadu_si512(init.as_ptr() as *const __m512i);
    let mut buf = [0u8; S + 32];
    for i in 0..32 {
        buf[i] = b'0';
    }
    let ones = _mm512_set1_epi8(b'1' as i8);
    let c0 = _mm512_set1_epi8(b'1' as i8);
    let c1 = _mm512_set1_epi8(b'7' as i8);
    let c2 = _mm512_set1_epi8(b'1' as i8);
    let base_ptr = buf.as_mut_ptr();

    loop {
        let mut k = 0;
        while k < NEWD {
            gen32(&mut st, base_ptr.add(32 + k));
            gen32(&mut st, base_ptr.add(32 + k + 32));
            buf[32 + k] = b'0' + 1 + (rng.next() % 9) as u8;
            k += LEN;
        }
        DIGITS.fetch_add(NEWD as u64, Ordering::Relaxed);
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        let mut p0 = 11;
        while p0 + 66 <= S {
            let v = _mm512_loadu_si512(base_ptr.add(p0) as *const __m512i);
            
            let w1 = _mm512_loadu_si512(base_ptr.add(p0 + 1) as *const __m512i);
            let w2 = _mm512_loadu_si512(base_ptr.add(p0 + 2) as *const __m512i);
            let mut m = _mm512_cmpeq_epi8_mask(v, c0) & _mm512_cmpeq_epi8_mask(w1, c1) & _mm512_cmpeq_epi8_mask(w2, c2);
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
            p0 += 64;
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

    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    println!("RUST AVX-512: пара привет+друг, зазор <= {} | {} потока | {:.0} сек",
             gapmax, threads, runtime);
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
            println!("... {:.0} сек | {:.1} млрд цифр | {:.0} млн цифр/сек | приветов: {}",
                     el, d as f64 / 1e9, d as f64 / el / 1e6,
                     P1HITS.load(Ordering::Relaxed));
            nxt += 10.0;
        }
    }
    for h in handles {
        h.join().unwrap();
    }
    let el = t0.elapsed().as_secs_f64();
    let d = DIGITS.load(Ordering::Relaxed);
    println!("\n========== РЕЗУЛЬТАТ (Rust AVX-512) ==========");
    println!("цифр: {:.1} млрд | время: {:.0} сек | скорость: {:.0} млн цифр/сек",
             d as f64 / 1e9, el, d as f64 / el / 1e6);
    println!("приветов: {}", P1HITS.load(Ordering::Relaxed));
    if FOUND.load(Ordering::Relaxed) {
        println!("НАЙДЕНО! зазор {} цифр", GAP.load(Ordering::Relaxed));
    }
}