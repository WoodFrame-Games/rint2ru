// Простой анализ случайного потока без цели:
//   - гистограмма цифр 0..9
//   - гистограмма 2-грамм 00..99
// Генератор: counter-based VAES r3 (как в pair5_fused_best).
// Анализируется каждый 1024-й 64-цифровой блок (выборка ~0.1%),
// чтобы не замедлять генерацию.
//
// Аргумент: секунды (по умолчанию 1800). THREADS=N — число потоков.

use std::arch::x86_64::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

const AES_ROUNDS: usize = 3;
const SAMPLE: u64 = 1024; // степень двойки

static DIGITS: AtomicU64 = AtomicU64::new(0);
static STOP: AtomicBool = AtomicBool::new(false);

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

struct Stats {
    hist1: [u64; 10],
    hist2: [u64; 100],
    sampled_blocks: u64,
}

#[inline(always)]
unsafe fn analyze(w: __m512i, st: &mut Stats) {
    let mut buf = [0u8; 64];
    _mm512_storeu_si512(buf.as_mut_ptr() as *mut __m512i, w);
    let mut prev = (buf[0] - b'0') as usize;
    st.hist1[prev] += 1;
    let mut i = 1;
    while i < 64 {
        let d = (buf[i] - b'0') as usize;
        st.hist1[d] += 1;
        st.hist2[prev * 10 + d] += 1;
        prev = d;
        i += 1;
    }
    st.sampled_blocks += 1;
}

unsafe fn worker(seed: u64) -> Stats {
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

    let mut st = Stats {
        hist1: [0; 10],
        hist2: [0; 100],
        sampled_blocks: 0,
    };
    // sink не даёт компилятору выкинуть генерацию неотобранных блоков
    let mut sink = _mm512_setzero_si512();
    let mut local: u64 = 0;
    let mut iters: u32 = 0;
    let mut block: u64 = 0;

    loop {
        let a = gen_block_aes(&mut c1, key1);
        let b = gen_block_aes(&mut c0, key0);
        let c = gen_block_aes(&mut c1, key1);
        let d = gen_block_aes(&mut c0, key0);

        sink = _mm512_xor_si512(
            sink,
            _mm512_xor_si512(_mm512_xor_si512(a, b), _mm512_xor_si512(c, d)),
        );

        // Блок всегда кратен 4, поэтому анализируем группу целиком:
        // раз в 4*SAMPLE блоков берём все четыре (a,c — st1; b,d — st0).
        // Итоговая выборка остаётся 1/SAMPLE, но покрывает оба потока.
        if block % (SAMPLE * 4) == 0 {
            analyze(a, &mut st);
            analyze(b, &mut st);
            analyze(c, &mut st);
            analyze(d, &mut st);
        }
        block += 4;

        local += 256;
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
    DIGITS.fetch_add(local, Ordering::Relaxed);
    let _ = std::hint::black_box(sink);
    st
}

fn main() {
    let runtime: f64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800.0);
    let threads = std::env::var("THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2)
        });

    println!(
        "RUST rstat: VAES r3, без цели | гистограмма цифр + 2-граммы, выборка 1/{} | {} потока | {:.0} сек",
        SAMPLE, threads, runtime
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
            pin_cpu(t);
            worker(s)
        }));
    }

    let mut nxt = 60.0;
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
                "... {:.0} сек | {:.2} трлн цифр | {:.0} млн цифр/сек",
                el,
                d as f64 / 1e12,
                d as f64 / el / 1e6
            );
            nxt += 60.0;
        }
    }

    let mut hist1 = [0u64; 10];
    let mut hist2 = [0u64; 100];
    let mut sampled_blocks = 0u64;
    for h in handles {
        let st = h.join().unwrap();
        for i in 0..10 {
            hist1[i] += st.hist1[i];
        }
        for i in 0..100 {
            hist2[i] += st.hist2[i];
        }
        sampled_blocks += st.sampled_blocks;
    }

    let el = t0.elapsed().as_secs_f64();
    let d = DIGITS.load(Ordering::Relaxed);
    let n1: u64 = hist1.iter().sum();
    let n2: u64 = hist2.iter().sum();

    println!("\n========== РЕЗУЛЬТАТ (rstat) ==========");
    println!(
        "сгенерировано: {:.2} трлн цифр | {:.0} сек | {:.0} млн цифр/сек",
        d as f64 / 1e12,
        el,
        d as f64 / el / 1e6
    );
    println!(
        "выборка: {} блоков = {:.2} млрд цифр ({:.4}%)",
        sampled_blocks,
        n1 as f64 / 1e9,
        100.0 * n1 as f64 / d as f64
    );

    print!("доли цифр:");
    for i in 0..10 {
        print!(" {}={:.5}", i, hist1[i] as f64 / n1 as f64);
    }
    println!();

    let e1 = n1 as f64 / 10.0;
    let chi1: f64 = hist1
        .iter()
        .map(|&c| {
            let t = c as f64 - e1;
            t * t / e1
        })
        .sum();
    println!(
        "chi2 цифр: {:.1} при df=9 -> chi2/df={:.4}, z={:+.2}",
        chi1,
        chi1 / 9.0,
        (chi1 - 9.0) / (2.0f64 * 9.0).sqrt()
    );

    let e2 = n2 as f64 / 100.0;
    let chi2: f64 = hist2
        .iter()
        .map(|&c| {
            let t = c as f64 - e2;
            t * t / e2
        })
        .sum();
    let mut idx: Vec<usize> = (0..100).collect();
    idx.sort_by_key(|&i| hist2[i]);
    let fmt = |i: usize| format!("{}{}", i / 10, i % 10);
    let bottom: Vec<String> = idx[..5].iter().map(|&i| fmt(i)).collect();
    let top: Vec<String> = idx[95..].iter().rev().map(|&i| fmt(i)).collect();
    println!(
        "chi2 2-грамм: {:.1} при df=99 -> chi2/df={:.4}, z={:+.2}",
        chi2,
        chi2 / 99.0,
        (chi2 - 99.0) / (2.0f64 * 99.0).sqrt()
    );
    println!(
        "2-граммы min/max: {}/{} (среднее {:.1}); редкие: {}; частые: {}",
        hist2[idx[0]],
        hist2[idx[99]],
        e2,
        bottom.join(","),
        top.join(",")
    );
}