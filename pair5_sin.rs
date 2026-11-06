// Синус-генератор вместо рандома на fused-ядре (PIPE4 + vpermi2b 4B + pin).
//
// Алгоритм (как в Python):
//   x += step
//   n = |int(sin(x*x) * 100)| % 100
//   две десятичные цифры n пишутся в поток
//
// Ищем ту же пару: «привет» (1718103620) + «друг» (518214), зазор <= 49
// (ограничение fused: prev+cur = 128 байт).
//
// CHECK=1 — напечатать первые 15 чисел (30 цифр) для сверки с Python.
// THREADS=N — число потоков (по умолчанию все vCPU).
// Аргументы: <секунды> <зазор> <step>   (step по умолчанию 1.0)

use std::arch::x86_64::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

const BLOCK: usize = 64;
const MAX_GAP: usize = 49;

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

struct SinGen {
    x: f64,
    step: f64,
}
impl SinGen {
    #[inline(always)]
    fn next_n(&mut self) -> u8 {
        self.x += self.step;
        let v = (self.x * self.x).sin() * 100.0;
        ((v as i64).abs() % 100) as u8
    }
}

// 32 значения n -> 64 ASCII-цифры в одном ZMM
#[inline(always)]
unsafe fn gen_block_sin(sg: &mut SinGen) -> __m512i {
    let mut buf = [0u8; BLOCK];
    let mut i = 0;
    while i < 32 {
        let n = sg.next_n();
        buf[2 * i] = b'0' + n / 10;
        buf[2 * i + 1] = b'0' + n % 10;
        i += 1;
    }
    _mm512_loadu_si512(buf.as_ptr() as *const __m512i)
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

unsafe fn worker(
    tid: usize,
    p1lo: u64,
    p1hi: u16,
    p2lo: u32,
    p2hi: u16,
    gapmax: usize,
    step: f64,
) {
    let mut sg = SinGen {
        x: (tid as f64) * 1.0e9,
        step,
    };
    let c0 = _mm512_set1_epi8(b'1' as i8);
    let c1 = _mm512_set1_epi8(b'7' as i8);
    let c2 = _mm512_set1_epi8(b'1' as i8);
    let c3 = _mm512_set1_epi8(b'8' as i8);
    let idx1 = _mm512_loadu_si512(IDX1.as_ptr() as *const __m512i);
    let idx2 = _mm512_loadu_si512(IDX2.as_ptr() as *const __m512i);
    let idx3 = _mm512_loadu_si512(IDX3.as_ptr() as *const __m512i);
    let genonly = std::env::var("GENONLY").is_ok();

    let mut prev = gen_block_sin(&mut sg);
    let mut local: u64 = 0;
    let mut iters: u32 = 0;

    loop {
        let a = gen_block_sin(&mut sg);
        let b = gen_block_sin(&mut sg);
        let c = gen_block_sin(&mut sg);
        let d = gen_block_sin(&mut sg);
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
    DIGITS.fetch_add(local, Ordering::Relaxed);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let runtime: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(300.0);
    let gapmax: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let step: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);

    if std::env::var("CHECK").is_ok() {
        let mut sg = SinGen { x: 0.0, step };
        let mut out = String::new();
        for _ in 0..15 {
            out.push_str(&format!("{:02}", sg.next_n()));
        }
        println!("{}", out);
        return;
    }

    if gapmax > MAX_GAP {
        eprintln!(
            "sine fused: зазор <= {} (prev+cur = 128 байт); для 10000 используйте другой буфер",
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
    println!(
        "RUST AVX-512 sine: x+=step, n=|int(sin(x^2)*100)|%100, 2 цифры/шаг | step={} | зазор <= {} | {} потока | {:.0} сек",
        step, gapmax, threads, runtime
    );
    let t0 = Instant::now();

    let mut handles = Vec::new();
    for t in 0..threads {
        handles.push(std::thread::spawn(move || unsafe {
            pin_cpu(t);
            worker(t, p1lo, p1hi, p2lo, p2hi, gapmax, step);
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
    println!("\n========== РЕЗУЛЬТАТ (sine fused) ==========");
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