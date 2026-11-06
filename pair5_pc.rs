// Producer/consumer через SMT: один поток генерирует цифры в кольцевой буфер,
// второй читает блоки и делает fused-скан (vpermi2b).
//
// Режимы (MODE=...):
//   ponly — только генератор+store, без ожиданий (потолок продюсера)
//   conly — только скан статического буфера из памяти (потолок консьюмера)
//   (пусто) — обычный producer/consumer
//
// Чанк: --cfg chunk32k / chunk64k (по умолчанию 8K, L1).
// Требует AVX-512F/BW/VBMI + VAES.

use std::arch::x86_64::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

const BLOCK: usize = 64;
const BLOCKS_PER_CHUNK: usize = if cfg!(chunk64k) {
    1024
} else if cfg!(chunk32k) {
    512
} else {
    128
};
const CHUNK: usize = BLOCK * BLOCKS_PER_CHUNK;
const RING: usize = 4;
const AES_ROUNDS: usize = 3;
const MAX_GAP: usize = 49;

#[repr(align(64))]
struct Chunk([u8; CHUNK]);

static PRODUCED: AtomicU64 = AtomicU64::new(0);
static CONSUMED: AtomicU64 = AtomicU64::new(0);
static DIGITS: AtomicU64 = AtomicU64::new(0);
static SCANNED: AtomicU64 = AtomicU64::new(0);
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

#[inline(always)]
unsafe fn mulhi32(a: __m512i, b: __m512i) -> __m512i {
    let e = _mm512_mul_epu32(a, b);
    let o = _mm512_mul_epu32(_mm512_srli_epi64::<32>(a), _mm512_srli_epi64::<32>(b));
    let he = _mm512_srli_epi64::<32>(e);
    let ho = _mm512_slli_epi64::<32>(_mm512_srli_epi64::<32>(o));
    _mm512_mask_blend_epi32(0xAAAA, he, ho)
}

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
    let s3 = _mm512_permutex2var_epi8(prev, idx3, cur);
    let m = _mm512_cmpeq_epi8_mask(prev, c0)
        & _mm512_cmpeq_epi8_mask(s1, c1)
        & _mm512_cmpeq_epi8_mask(s2, c2)
        & _mm512_cmpeq_epi8_mask(s3, c3);
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
        verify_at(sp, p, p1lo, p1hi, p2lo, p2hi, gapmax);
        if STOP.load(Ordering::Relaxed) {
            break;
        }
    }
}

#[inline(always)]
fn wait(yield_mode: bool, backoff: usize) {
    if yield_mode {
        std::thread::yield_now();
    } else {
        for _ in 0..backoff {
            std::hint::spin_loop();
        }
    }
}

unsafe fn producer(ptr: *mut u8, seed: u64, yield_mode: bool, ponly: bool, backoff: usize) {
    let mut rng = Rng(seed);
    let b0 = rng.next();
    let b1 = rng.next();
    let mut r0 = [0u64; 8];
    let mut r1 = [0u64; 8];
    for i in 0..8 {
        r0[i] = b0.wrapping_add(i as u64);
        r1[i] = b1.wrapping_add(i as u64);
    }
    let key0 = _mm512_set1_epi64(rng.next() as i64);
    let key1 = _mm512_set1_epi64(rng.next() as i64);
    let mut c0 = _mm512_loadu_si512(r0.as_ptr() as *const __m512i);
    let mut c1 = _mm512_loadu_si512(r1.as_ptr() as *const __m512i);

    let mut seq: u64 = 0;
    let mut local: u64 = 0;
    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        if !ponly && seq.wrapping_sub(CONSUMED.load(Ordering::Acquire)) >= RING as u64 {
            wait(yield_mode, backoff);
            continue;
        }
        let slot = (seq % RING as u64) as usize;
        let base = ptr.add(slot * CHUNK);
        let mut b = 0;
        while b < BLOCKS_PER_CHUNK {
            let w0 = gen_block_aes(&mut c1, key1);
            _mm512_storeu_si512(base.add(64 * b) as *mut __m512i, w0);
            let w1 = gen_block_aes(&mut c0, key0);
            _mm512_storeu_si512(base.add(64 * (b + 1)) as *mut __m512i, w1);
            let w2 = gen_block_aes(&mut c1, key1);
            _mm512_storeu_si512(base.add(64 * (b + 2)) as *mut __m512i, w2);
            let w3 = gen_block_aes(&mut c0, key0);
            _mm512_storeu_si512(base.add(64 * (b + 3)) as *mut __m512i, w3);
            b += 4;
        }
        PRODUCED.store(seq + 1, Ordering::Release);
        local += CHUNK as u64;
        if seq % 16 == 15 {
            DIGITS.fetch_add(local, Ordering::Relaxed);
            local = 0;
        }
        seq += 1;
    }
    DIGITS.fetch_add(local, Ordering::Relaxed);
}

unsafe fn consumer(
    ptr: *const u8,
    p1lo: u64,
    p1hi: u16,
    p2lo: u32,
    p2hi: u16,
    gapmax: usize,
    yield_mode: bool,
    conly: bool,
    backoff: usize,
) {
    let c0 = _mm512_set1_epi8(b'1' as i8);
    let c1 = _mm512_set1_epi8(b'7' as i8);
    let c2 = _mm512_set1_epi8(b'1' as i8);
    let c3 = _mm512_set1_epi8(b'8' as i8);
    let idx1 = _mm512_loadu_si512(IDX1.as_ptr() as *const __m512i);
    let idx2 = _mm512_loadu_si512(IDX2.as_ptr() as *const __m512i);
    let idx3 = _mm512_loadu_si512(IDX3.as_ptr() as *const __m512i);

    let mut seq: u64 = 0;
    let mut local: u64 = 0;
    loop {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        if !conly && PRODUCED.load(Ordering::Acquire) < seq + 2 {
            wait(yield_mode, backoff);
            continue;
        }
        let slot = (seq % RING as u64) as usize;
        let nslot = ((seq + 1) % RING as u64) as usize;
        let base = ptr.add(slot * CHUNK);
        let nbase = ptr.add(nslot * CHUNK);

        let mut prev = _mm512_loadu_si512(base as *const __m512i);
        let mut b = 1;
        while b < BLOCKS_PER_CHUNK {
            let cur = _mm512_loadu_si512(base.add(64 * b) as *const __m512i);
            scan(
                prev, cur, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
            );
            prev = cur;
            b += 1;
        }
        let nb = _mm512_loadu_si512(nbase as *const __m512i);
        scan(
            prev, nb, c0, c1, c2, c3, idx1, idx2, idx3, p1lo, p1hi, p2lo, p2hi, gapmax,
        );

        if conly {
            local += CHUNK as u64;
            if seq % 16 == 15 {
                SCANNED.fetch_add(local, Ordering::Relaxed);
                local = 0;
            }
        } else {
            CONSUMED.store(seq + 1, Ordering::Release);
        }
        seq += 1;
    }
    if conly {
        SCANNED.fetch_add(local, Ordering::Relaxed);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let runtime: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(300.0);
    let gapmax: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    if gapmax > MAX_GAP {
        eprintln!("producer/consumer: зазор <= {}", MAX_GAP);
        std::process::exit(2);
    }
    let mode = std::env::var("MODE").unwrap_or_default();
    let ponly = mode == "ponly";
    let conly = mode == "conly";
    let yield_mode = std::env::var("YIELD").is_ok();
    let backoff: usize = std::env::var("BACKOFF")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256);
    let no_pin = std::env::var("NOPIN").is_ok();

    let p1: [u8; 10] = *b"1718103620";
    let p1lo = u64::from_le_bytes(p1[0..8].try_into().unwrap());
    let p1hi = u16::from_le_bytes(p1[8..10].try_into().unwrap());
    let p2: [u8; 6] = *b"518214";
    let p2lo = u32::from_le_bytes(p2[0..4].try_into().unwrap());
    let p2hi = u16::from_le_bytes(p2[4..6].try_into().unwrap());

    println!(
        "RUST producer/consumer: mode={}{} | RING={} x {} цифр | зазор <= {} | {:.0} сек",
        if ponly {
            "ponly"
        } else if conly {
            "conly"
        } else {
            "pc"
        },
        if yield_mode { ",yield" } else { ",spin" },
        RING,
        CHUNK,
        gapmax,
        runtime
    );

    let mut ring: Vec<Chunk> = (0..RING).map(|_| Chunk([b'9'; CHUNK])).collect();
    let p = ring.as_mut_ptr() as usize;

    let t0 = Instant::now();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let mut hp = None;
    let mut hc = None;
    if !conly {
        hp = Some(std::thread::spawn(move || unsafe {
            if !no_pin {
                pin_cpu(0);
            }
            producer(
                p as *mut u8,
                seed ^ 0x1111_1111_1111_1111,
                yield_mode,
                ponly,
                backoff,
            );
        }));
    }
    if !ponly {
        hc = Some(std::thread::spawn(move || unsafe {
            if !no_pin {
                pin_cpu(1);
            }
            consumer(
                p as *const u8,
                p1lo,
                p1hi,
                p2lo,
                p2hi,
                gapmax,
                yield_mode,
                conly,
                backoff,
            );
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
            let d = if conly {
                SCANNED.load(Ordering::Relaxed)
            } else {
                DIGITS.load(Ordering::Relaxed)
            };
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
    if let Some(h) = hp {
        h.join().unwrap();
    }
    if let Some(h) = hc {
        h.join().unwrap();
    }
    let el = t0.elapsed().as_secs_f64();
    let d = if conly {
        SCANNED.load(Ordering::Relaxed)
    } else {
        DIGITS.load(Ordering::Relaxed)
    };
    println!("\n========== РЕЗУЛЬТАТ ==========");
    println!(
        "цифр: {:.1} млрд | время: {:.0} сек | скорость: {:.0} млн цифр/сек",
        d as f64 / 1e9,
        el,
        d as f64 / el / 1e6
    );
    println!(
        "потреблено (скан): {:.1} млрд = {:.0} млн цифр/сек ({} чанков)",
        CONSUMED.load(Ordering::Relaxed) as f64 * CHUNK as f64 / 1e9,
        CONSUMED.load(Ordering::Relaxed) as f64 * CHUNK as f64 / el / 1e6,
        CONSUMED.load(Ordering::Relaxed)
    );
    println!("приветов: {}", P1HITS.load(Ordering::Relaxed));
    if FOUND.load(Ordering::Relaxed) {
        println!("НАЙДЕНО! зазор {} цифр", GAP.load(Ordering::Relaxed));
    }
}