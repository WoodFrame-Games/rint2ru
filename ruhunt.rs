// Rust-порт быстрого словарного хантера (по мотивам ru10_lanes4.cpp):
// Aho-Corasick по цифровому потоку, 4 ленты на поток, u16-таблица переходов,
// xorshift64-генератор (4 цифры на 32-битный лейн).
//
// Пишет в текущую папку:
//   hunt_journal.txt   — лучшая фраза (4+ слов) каждые 10 с
//   hunt_stats.txt     — счётчики каждые 60 с и финал
//   hunt_longwords.txt — первые появления слов 6+ букв
//
// Аргументы: <словарь> <секунды> [LEN] [LW_MIN] [take_n]
//   ru_50k.txt 1800
//
// Требует AVX-512F/BW/VBMI (vpermi2b не нужен; генератор использует F/BW).
// Потоки пиннятся на CPU t.

use std::arch::x86_64::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const LEN: usize = 60;
const TAIL: usize = 16;
const MAXH: usize = 256;

const ALPH: &str = "абвгдеёжзийклмнопрстуфхцчшщъыьэюя";

const VSUFS: &[&str] = &[
    "ть", "тся", "ться", "ла", "ло", "ли", "ет", "ит", "ут", "ют", "ат", "ят", "ем", "им",
    "ся", "сь", "ешь", "ишь", "ешься", "ишься", "лась", "лось", "лись", "лся", "ёт", "ал",
    "ел", "ил", "ул", "ыл", "ол",
];

const STOPW: &[&str] = &[
    "я", "ты", "он", "она", "оно", "мы", "вы", "они", "мне", "меня", "тебя", "тебе", "вас",
    "вам", "нас", "нам", "их", "его", "ее", "ей", "им", "ими", "себя", "себе", "сам", "сама",
    "сами", "свой", "своя", "свои", "свое", "моя", "мой", "мои", "мое", "твой", "твоя", "твои",
    "ваши", "наши", "наш", "ваш", "там", "тут", "вот", "где", "куда", "как", "что", "чтобы",
    "зачем", "почему", "когда", "уже", "еще", "ещё", "тоже", "так", "да", "нет", "не", "ни",
    "но", "и", "а", "или", "ли", "же", "бы", "уж", "все", "всё", "это", "этот", "эта", "эти",
    "тот", "та", "те", "тех", "того", "тому", "том", "тем", "чем", "чего", "чему", "кому",
    "кого", "эту", "без", "два", "две", "над", "под", "при", "про", "для", "до", "от", "по",
    "за", "из", "об", "во", "на", "оба", "обе", "лет", "раз", "то", "ну", "ведь", "лишь",
    "только", "даже", "почти", "чуть", "очень", "совсем", "вдруг", "опять", "снова", "между",
    "через", "после", "перед", "около", "возле", "вместо", "кроме", "среди",
];

const BADW: &[&str] = &[
    "тед", "джо", "бет", "ева", "деб", "даг", "бла", "дке", "пёп", "базз", "хуя", "ааа",
    "эээ", "угу", "ага", "нее", "неа", "даа", "мда", "ооо", "эх",
];

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
unsafe fn gen64(st: &mut __m512i, out: *mut u8) {
    let mut x = *st;
    x = _mm512_xor_si512(x, _mm512_slli_epi64::<13>(x));
    x = _mm512_xor_si512(x, _mm512_srli_epi64::<7>(x));
    x = _mm512_xor_si512(x, _mm512_slli_epi64::<17>(x));
    *st = x;
    _mm512_storeu_si512(out as *mut __m512i, digits256(x));
}

#[derive(Clone, Copy, Default)]
struct Sel {
    i: u8,
    j: u8,
    wid: i32,
}

#[derive(Clone, Copy, Default)]
struct OE {
    wid: i32,
    len: u8,
    next: i32,
}

struct Dict {
    tr: Vec<u16>,
    nout: Vec<i32>,
    outlist: Vec<OE>,
    wordstr: Vec<String>,
    wlog: Vec<f64>,
    wcat: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Cand {
    nw: u8,
    cov: u8,
    cross: u16,
    score: f64,
    s: [u8; TAIL + LEN],
    sel: [Sel; 10],
}

struct Stats {
    digits: AtomicU64,
    n2: AtomicU64,
    n3: AtomicU64,
    n4: AtomicU64,
    n5: AtomicU64,
    n6: AtomicU64,
    rep: AtomicU64,
    gram: AtomicU64,
    crossw: AtomicU64,
    crossp: AtomicU64,
    longhits: AtomicU64,
    maxnw: AtomicU64,
}
impl Stats {
    fn new() -> Self {
        Self {
            digits: AtomicU64::new(0),
            n2: AtomicU64::new(0),
            n3: AtomicU64::new(0),
            n4: AtomicU64::new(0),
            n5: AtomicU64::new(0),
            n6: AtomicU64::new(0),
            rep: AtomicU64::new(0),
            gram: AtomicU64::new(0),
            crossw: AtomicU64::new(0),
            crossp: AtomicU64::new(0),
            longhits: AtomicU64::new(0),
            maxnw: AtomicU64::new(0),
        }
    }
}

struct LongState {
    map: HashMap<String, u64>,
    file: Option<File>,
}

struct BestState {
    best: Option<Cand>,
}

#[inline]
fn verb_like(w: &str) -> bool {
    VSUFS
        .iter()
        .any(|s| w.len() > s.len() && w.ends_with(s))
}

fn load_dict(path: &str, take_n: usize) -> Dict {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("нет словаря {}: {}", path, e);
        std::process::exit(1);
    });
    let text = text.trim_start_matches('\u{feff}');

    let mut lmap = vec![-1i16; 65536];
    let ab = ALPH.as_bytes();
    for (i, pair) in ab.chunks_exact(2).enumerate() {
        let key = ((pair[0] as u16) << 8) | pair[1] as u16;
        lmap[key as usize] = (i + 1) as i16;
    }
    let stop: HashSet<&str> = STOPW.iter().copied().collect();
    let bad: HashSet<&str> = BADW.iter().copied().collect();

    let mut tr: Vec<u16> = vec![0; 10];
    let mut nout: Vec<i32> = vec![-1];
    let mut ndepn: Vec<u8> = vec![0];
    let mut nwidx: Vec<i32> = vec![-1];
    let mut outlist: Vec<OE> = Vec::new();
    let mut wordstr: Vec<String> = Vec::new();
    let mut wlog: Vec<f64> = Vec::new();
    let mut wcat: Vec<u8> = Vec::new();

    for line in text.lines() {
        if wordstr.len() >= take_n {
            break;
        }
        let mut it = line.split_whitespace();
        let w = match it.next() {
            Some(w) => w,
            None => continue,
        };
        let cnt: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        let b = w.as_bytes();
        if b.len() % 2 != 0 {
            continue;
        }
        let mut code: u64 = 0;
        let mut letters = 0usize;
        let mut ok = true;
        for pair in b.chunks_exact(2) {
            let key = ((pair[0] as u16) << 8) | pair[1] as u16;
            let v = lmap[key as usize];
            if v < 0 {
                ok = false;
                break;
            }
            letters += 1;
            if v < 10 {
                code = code * 10 + v as u64;
            } else {
                code = code * 100 + v as u64;
            }
        }
        if !ok || letters < 3 || letters > 8 {
            continue;
        }

        let wid = wordstr.len() as i32;
        let mut ds = [0u8; 20];
        let mut dk = 0usize;
        let mut c = code;
        while c > 0 {
            ds[dk] = (c % 10) as u8;
            c /= 10;
            dk += 1;
        }
        let mut cur = 0usize;
        for idx in (0..dk).rev() {
            let d = ds[idx] as usize;
            let pos = cur * 10 + d;
            let v = tr[pos];
            if v == 0 {
                let ns = (tr.len() / 10) as u16;
                tr.extend_from_slice(&[0u16; 10]);
                tr[pos] = ns;
                nout.push(-1);
                ndepn.push(ndepn[cur] + 1);
                nwidx.push(-1);
                cur = ns as usize;
            } else {
                cur = v as usize;
            }
        }
        if nwidx[cur] < 0 {
            nwidx[cur] = wid;
        }

        let cat = if bad.contains(w) {
            3
        } else if stop.contains(w) {
            2
        } else if verb_like(w) {
            1
        } else {
            0
        };
        wordstr.push(w.to_string());
        wlog.push((cnt.max(1) as f64).log10());
        wcat.push(cat);
    }

    if nwidx.len() >= 65535 {
        eprintln!(
            "слишком много узлов для u16: {} (уменьшите take_n)",
            nwidx.len()
        );
        std::process::exit(1);
    }

    let n = nwidx.len();
    let mut fail = vec![0u16; n];
    let mut q: VecDeque<u16> = VecDeque::with_capacity(n / 2 + 8);
    for d in 0..10 {
        let v = tr[d];
        if v != 0 {
            if nwidx[v as usize] >= 0 {
                outlist.push(OE {
                    wid: nwidx[v as usize],
                    len: ndepn[v as usize],
                    next: -1,
                });
                nout[v as usize] = outlist.len() as i32 - 1;
            }
            q.push_back(v);
        }
    }
    while let Some(u16v) = q.pop_front() {
        let u = u16v as usize;
        for d in 0..10 {
            let pos = u * 10 + d;
            let v = tr[pos];
            if v != 0 {
                let fl = tr[fail[u] as usize * 10 + d];
                fail[v as usize] = fl;
                if nwidx[v as usize] >= 0 {
                    let next = nout[fl as usize];
                    outlist.push(OE {
                        wid: nwidx[v as usize],
                        len: ndepn[v as usize],
                        next,
                    });
                    nout[v as usize] = outlist.len() as i32 - 1;
                } else {
                    nout[v as usize] = nout[fl as usize];
                }
                q.push_back(v);
            } else {
                tr[pos] = tr[fail[u] as usize * 10 + d];
            }
        }
    }

    eprintln!(
        "словарь: {} слов, узлов {}, выходов {}",
        wordstr.len(),
        nwidx.len(),
        outlist.len()
    );

    Dict {
        tr,
        nout,
        outlist,
        wordstr,
        wlog,
        wcat,
    }
}

fn handle_round(
    dict: &Dict,
    stats: &Stats,
    long: &Mutex<LongState>,
    best: &Mutex<BestState>,
    buf: &[u8],
    hh: &mut [Sel; MAXH],
    nh: usize,
    lw_min: usize,
) {
    if nh < 2 {
        return;
    }
    hh[..nh].sort_unstable_by(|a, b| a.i.cmp(&b.i).then_with(|| b.j.cmp(&a.j)));

    let mut sel = [Sel::default(); 10];
    let mut ns = 0usize;
    let mut last: i16 = -1;
    for k in 0..nh {
        if ns >= 10 {
            break;
        }
        if hh[k].i as i16 >= last {
            sel[ns] = hh[k];
            last = hh[k].j as i16;
            ns += 1;
        }
    }
    if ns < 2 {
        return;
    }

    let mut rep = false;
    'outer: for a in 0..ns {
        for b in a + 1..ns {
            if sel[a].wid == sel[b].wid {
                rep = true;
                break 'outer;
            }
        }
    }
    if rep {
        stats.rep.fetch_add(1, Ordering::Relaxed);
        return;
    }

    match ns {
        2 => stats.n2.fetch_add(1, Ordering::Relaxed),
        3 => stats.n3.fetch_add(1, Ordering::Relaxed),
        4 => stats.n4.fetch_add(1, Ordering::Relaxed),
        5 => stats.n5.fetch_add(1, Ordering::Relaxed),
        _ => stats.n6.fetch_add(1, Ordering::Relaxed),
    };

    let mut nv = 0;
    let mut nstop = 0;
    let mut bad = false;
    let mut cm: u16 = 0;
    for k in 0..ns {
        let ct = dict.wcat[sel[k].wid as usize];
        if ct == 1 {
            nv += 1;
        } else if ct == 2 {
            nstop += 1;
        } else if ct == 3 {
            bad = true;
        }
        if (sel[k].i as usize) < TAIL && (sel[k].j as usize) > TAIL {
            cm |= 1u16 << k;
        }
    }
    if bad || nv == 0 || nstop > 2 {
        stats.gram.fetch_add(1, Ordering::Relaxed);
        return;
    }
    stats
        .crossw
        .fetch_add(cm.count_ones() as u64, Ordering::Relaxed);
    if cm != 0 {
        stats.crossp.fetch_add(1, Ordering::Relaxed);
    }

    for k in 0..ns {
        if dict.wcat[sel[k].wid as usize] == 3 {
            continue;
        }
        let w = &dict.wordstr[sel[k].wid as usize];
        let letters = w.as_bytes().len() / 2;
        if letters < lw_min {
            continue;
        }
        stats.longhits.fetch_add(1, Ordering::Relaxed);
        let iscross = (sel[k].i as usize) < TAIL && (sel[k].j as usize) > TAIL;
        let mut lk = long.lock().unwrap();
        let first = {
            let cnt = lk.map.entry(w.clone()).or_insert(0);
            let first = *cnt == 0;
            *cnt += 1;
            first
        };
        if first {
            if let Some(f) = lk.file.as_mut() {
                let ts = now_secs();
                let _ = write!(
                    f,
                    "{} | {} букв | {} | @{}-{}{} | фраза: ",
                    ts,
                    letters,
                    w,
                    sel[k].i,
                    sel[k].j,
                    if iscross { " СТЫК" } else { "" }
                );
                for q in 0..ns {
                    if q > 0 {
                        let _ = write!(f, " ");
                    }
                    let _ = write!(f, "{}", dict.wordstr[sel[q].wid as usize]);
                }
                let _ = writeln!(f);
                let _ = f.flush();
            }
        }
    }

    if ns >= 4 {
        let mut c = Cand {
            nw: ns as u8,
            cov: sel[ns - 1].j - sel[0].i,
            cross: cm,
            score: 0.0,
            s: [0u8; TAIL + LEN],
            sel: [Sel::default(); 10],
        };
        for k in 0..ns {
            c.score += dict.wlog[sel[k].wid as usize];
        }
        c.s.copy_from_slice(&buf[..TAIL + LEN]);
        c.sel[..ns].copy_from_slice(&sel[..ns]);
        stats.maxnw.fetch_max(ns as u64, Ordering::Relaxed);
        let mut b = best.lock().unwrap();
        let better = match b.best {
            None => true,
            Some(cur) => ns > cur.nw as usize || (ns == cur.nw as usize && c.score > cur.score),
        };
        if better {
            b.best = Some(c);
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn fmt_cand(c: &Cand, dict: &Dict) -> (String, String) {
    let mut phrase = String::new();
    for k in 0..c.nw as usize {
        if k > 0 {
            phrase.push(' ');
        }
        phrase.push_str(&dict.wordstr[c.sel[k].wid as usize]);
        if c.cross & (1u16 << k) != 0 {
            phrase.push('*');
        }
    }
    let mut digits = String::with_capacity(TAIL + LEN + 1);
    for k in 0..TAIL + LEN {
        if k == TAIL {
            digits.push('|');
        }
        digits.push(c.s[k] as char);
    }
    (digits, phrase)
}

unsafe fn worker(
    dict: Arc<Dict>,
    stats: Arc<Stats>,
    long: Arc<Mutex<LongState>>,
    best: Arc<Mutex<BestState>>,
    seed: u64,
    lw_min: usize,
) {
    let d = &*dict;
    let mut rng = Rng(seed);
    let mut raw = [[0u64; 8]; 4];
    for l in 0..4 {
        for v in raw[l].iter_mut() {
            *v = rng.next() | 1;
        }
    }
    let mut stA = _mm512_loadu_si512(raw[0].as_ptr() as *const __m512i);
    let mut stB = _mm512_loadu_si512(raw[1].as_ptr() as *const __m512i);
    let mut stC = _mm512_loadu_si512(raw[2].as_ptr() as *const __m512i);
    let mut stD = _mm512_loadu_si512(raw[3].as_ptr() as *const __m512i);
    let mut bufA = [0u8; TAIL + 96];
    let mut bufB = [0u8; TAIL + 96];
    let mut bufC = [0u8; TAIL + 96];
    let mut bufD = [0u8; TAIL + 96];
    for k in 0..TAIL {
        bufA[k] = b'0';
        bufB[k] = b'0';
        bufC[k] = b'0';
        bufD[k] = b'0';
    }
    let mut astA: u16 = 0;
    let mut astB: u16 = 0;
    let mut astC: u16 = 0;
    let mut astD: u16 = 0;
    let mut hhA = [Sel::default(); MAXH];
    let mut hhB = [Sel::default(); MAXH];
    let mut hhC = [Sel::default(); MAXH];
    let mut hhD = [Sel::default(); MAXH];
    let mut nhA: usize = 0;
    let mut nhB: usize = 0;
    let mut nhC: usize = 0;
    let mut nhD: usize = 0;
    let mut local: u64 = 0;
    let mut iters: u64 = 0;

    macro_rules! step {
        ($ast:ident, $buf:ident, $hh:ident, $nh:ident, $p:expr) => {{
            let p = $p;
            let dig = (*$buf.get_unchecked(p) - b'0') as usize;
            let s = *d.tr.get_unchecked($ast as usize * 10 + dig);
            $ast = s;
            let mut o = *d.nout.get_unchecked(s as usize);
            while o >= 0 {
                let e = *d.outlist.get_unchecked(o as usize);
                let endp = p + 1;
                let i = endp as isize - e.len as isize;
                if i >= 0 && $nh < MAXH {
                    *$hh.get_unchecked_mut($nh) = Sel {
                        i: i as u8,
                        j: endp as u8,
                        wid: e.wid,
                    };
                    $nh += 1;
                }
                o = e.next;
            }
        }};
    }

    loop {
        gen64(&mut stA, bufA.as_mut_ptr().add(TAIL));
        gen64(&mut stB, bufB.as_mut_ptr().add(TAIL));
        gen64(&mut stC, bufC.as_mut_ptr().add(TAIL));
        gen64(&mut stD, bufD.as_mut_ptr().add(TAIL));
        nhA = 0;
        nhB = 0;
        nhC = 0;
        nhD = 0;
        for p in TAIL..TAIL + LEN {
            step!(astA, bufA, hhA, nhA, p);
            step!(astB, bufB, hhB, nhB, p);
            step!(astC, bufC, hhC, nhC, p);
            step!(astD, bufD, hhD, nhD, p);
        }
        handle_round(d, &stats, &long, &best, &bufA, &mut hhA, nhA, lw_min);
        handle_round(d, &stats, &long, &best, &bufB, &mut hhB, nhB, lw_min);
        handle_round(d, &stats, &long, &best, &bufC, &mut hhC, nhC, lw_min);
        handle_round(d, &stats, &long, &best, &bufD, &mut hhD, nhD, lw_min);
        bufA.copy_within(LEN..LEN + TAIL, 0);
        bufB.copy_within(LEN..LEN + TAIL, 0);
        bufC.copy_within(LEN..LEN + TAIL, 0);
        bufD.copy_within(LEN..LEN + TAIL, 0);

        local += (4 * LEN) as u64;
        iters += 1;
        if iters & 0x3FFF == 0 {
            stats.digits.fetch_add(local, Ordering::Relaxed);
            local = 0;
            if STOP.load(Ordering::Relaxed) {
                break;
            }
        }
    }
    stats.digits.fetch_add(local, Ordering::Relaxed);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dict_path = args.get(1).map(|s| s.as_str()).unwrap_or("ru_50k.txt");
    let runtime: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(86400.0);
    let lw_min: usize = args
        .get(4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(6usize)
        .max(4);
    let take_n: usize = args
        .get(5)
        .and_then(|s| s.parse().ok())
        .unwrap_or(9000usize)
        .max(100);

    let dict = Arc::new(load_dict(dict_path, take_n));
    let stats = Arc::new(Stats::new());
    let long = Arc::new(Mutex::new(LongState {
        map: HashMap::new(),
        file: OpenOptions::new()
            .create(true)
            .append(true)
            .open("hunt_longwords.txt")
            .ok(),
    }));
    let best = Arc::new(Mutex::new(BestState { best: None }));

    let mut jf = OpenOptions::new()
        .create(true)
        .append(true)
        .open("hunt_journal.txt")
        .unwrap();
    let mut sf = OpenOptions::new()
        .create(true)
        .append(true)
        .open("hunt_stats.txt")
        .unwrap();

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    let _ = writeln!(
        sf,
        "=== СТАРТ {} | словарь {} слов | потоков {} | цель {:.0} сек ===",
        now_secs(),
        dict.wordstr.len(),
        threads,
        runtime
    );
    let _ = sf.flush();

    let t0 = Instant::now();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let mut handles = Vec::new();
    for t in 0..threads {
        let d = dict.clone();
        let st = stats.clone();
        let lg = long.clone();
        let bs = best.clone();
        let s = seed ^ 0x9E3779B97F4A7C15u64.wrapping_mul(t as u64 + 1);
        handles.push(std::thread::spawn(move || unsafe {
            pin_cpu(t);
            worker(d, st, lg, bs, s, lw_min);
        }));
    }

    let mut next_j = 10.0;
    let mut next_s = 60.0;
    let mut seen: HashSet<String> = HashSet::new();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let el = t0.elapsed().as_secs_f64();
        if el >= next_j {
            next_j += 10.0;
            let c = best.lock().unwrap().best.take();
            let ts = now_secs();
            match c {
                Some(c) => {
                    let (digits, phrase) = fmt_cand(&c, &dict);
                    if seen.contains(&phrase) {
                        let _ = writeln!(jf, "{} | (повтор) {} сл. | {}", ts, c.nw, phrase);
                    } else {
                        if seen.len() < 500000 {
                            seen.insert(phrase.clone());
                        }
                        let _ = writeln!(jf, "{} | {} сл. | {}\n   {}", ts, c.nw, phrase, digits);
                    }
                }
                None => {
                    let _ = writeln!(jf, "{} | (нет находок 4+ слов за интервал)", ts);
                }
            }
            let _ = jf.flush();
        }
        if el >= next_s {
            next_s += 60.0;
            let d = stats.digits.load(Ordering::Relaxed);
            let _ = writeln!(
                sf,
                "{:.0} сек | чисел {} | {:.1} млрд цифр | {:.0} млн цифр/сек | 2:{} 3:{} 4:{} 5:{} 6+:{} | стык:{} | макс слов {}",
                el,
                d / LEN as u64,
                d as f64 / 1e9,
                d as f64 / el / 1e6,
                stats.n2.load(Ordering::Relaxed),
                stats.n3.load(Ordering::Relaxed),
                stats.n4.load(Ordering::Relaxed),
                stats.n5.load(Ordering::Relaxed),
                stats.n6.load(Ordering::Relaxed),
                stats.crossw.load(Ordering::Relaxed),
                stats.maxnw.load(Ordering::Relaxed)
            );
            let _ = sf.flush();
        }
        if el >= runtime {
            break;
        }
    }

    STOP.store(true, Ordering::Relaxed);
    for h in handles {
        h.join().unwrap();
    }

    let el = t0.elapsed().as_secs_f64();
    let d = stats.digits.load(Ordering::Relaxed);
    {
        let lk = long.lock().unwrap();
        let mut v: Vec<(u64, String)> = lk
            .map
            .iter()
            .map(|(k, &c)| (c, k.clone()))
            .collect();
        v.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        let maxlen = lk
            .map
            .keys()
            .map(|w| w.as_bytes().len() / 2)
            .max()
            .unwrap_or(0);
        let _ = writeln!(
            sf,
            "=== ДЛИННЫЕ СЛОВА (>= {} букв): {} уникальных, {} попаданий, максимум {} букв; топ-30 ===",
            lw_min,
            lk.map.len(),
            stats.longhits.load(Ordering::Relaxed),
            maxlen
        );
        for (cnt, w) in v.iter().take(30) {
            let _ = writeln!(sf, "{:6} | {} ({} букв)", cnt, w, w.as_bytes().len() / 2);
        }
    }
    let _ = writeln!(
        sf,
        "=== ФИНАЛ {:.0} сек | чисел {} | {:.1} млрд цифр | {:.0} млн цифр/сек | 2:{} 3:{} 4:{} 5:{} 6+:{} | стык:{} | макс слов {} ===",
        el,
        d / LEN as u64,
        d as f64 / 1e9,
        d as f64 / el / 1e6,
        stats.n2.load(Ordering::Relaxed),
        stats.n3.load(Ordering::Relaxed),
        stats.n4.load(Ordering::Relaxed),
        stats.n5.load(Ordering::Relaxed),
        stats.n6.load(Ordering::Relaxed),
        stats.crossw.load(Ordering::Relaxed),
        stats.maxnw.load(Ordering::Relaxed)
    );
    let _ = sf.flush();
}