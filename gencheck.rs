// Валидация генератора: гистограмма цифр, частоты окон и chi-square по
// непересекающимся 4-значным блокам.
// xorshift64 по умолчанию; --cfg aesrng — counter-based VAES.
use std::arch::x86_64::*;
use std::time::Instant;

const LEN: usize = 60;
const NOLEAD: bool = cfg!(nolead);
const AESRNG: bool = cfg!(aesrng);

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

// 16 x u32 -> 64 ASCII-цифр
#[inline(always)]
unsafe fn digits(x: __m512i) -> __m512i {
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
unsafe fn gen64(st: &mut __m512i) -> __m512i {
    let mut x = *st;
    x = _mm512_xor_si512(x, _mm512_slli_epi64::<13>(x));
    x = _mm512_xor_si512(x, _mm512_srli_epi64::<7>(x));
    x = _mm512_xor_si512(x, _mm512_slli_epi64::<17>(x));
    *st = x;
    digits(x)
}

const AES_ROUNDS: usize = if cfg!(aes4) {
    4
} else if cfg!(aes3) {
    3
} else if cfg!(aes2) {
    2
} else {
    1
};

#[inline(always)]
unsafe fn gen64_aes(ctr: &mut __m512i, key: __m512i) -> __m512i {
    *ctr = _mm512_add_epi64(*ctr, _mm512_set1_epi64(8));
    let mut x = *ctr;
    let mut r = 0;
    while r < AES_ROUNDS {
        x = _mm512_aesenc_epi128(x, key);
        r += 1;
    }
    digits(x)
}

fn main() {
    let secs: f64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20.0);

    let mut rng = Rng(0x1234_5678_9ABC_DEF0);
    let mut raw = [[0u64; 8]; 2];
    if AESRNG {
        let b0 = rng.next();
        let b1 = rng.next();
        for i in 0..8 {
            raw[0][i] = b0.wrapping_add(i as u64);
            raw[1][i] = b1.wrapping_add(i as u64);
        }
    } else {
        for s in 0..2 {
            for v in raw[s].iter_mut() {
                *v = rng.next() | 1;
            }
        }
    }
    let key0 = unsafe { _mm512_set1_epi64(rng.next() as i64) };
    let key1 = unsafe { _mm512_set1_epi64(rng.next() as i64) };
    let mut st = unsafe {
        [
            _mm512_loadu_si512(raw[0].as_ptr() as *const __m512i),
            _mm512_loadu_si512(raw[1].as_ptr() as *const __m512i),
        ]
    };

    const W6: u64 = 518_214;
    const W8: u64 = 17_181_036;
    println!(
        "генератор: {} | проверка распределения, цели {} и {}",
        if AESRNG { "VAES" } else { "xorshift64" },
        W6,
        W8
    );

    let t0 = Instant::now();
    let mut total: u64 = 0;
    let mut hist = [0u64; 10];
    let mut hist4 = vec![0u64; 10000];
    let mut c6: u64 = 0;
    let mut c8: u64 = 0;
    let mut w6: u64 = 0;
    let mut w8: u64 = 0;
    let mut buf = [0u8; 64];
    let mut chunk: u64 = 0;

    while t0.elapsed().as_secs_f64() < secs {
        let s = (chunk & 1) as usize;
        let w = unsafe {
            if AESRNG {
                if s == 0 {
                    gen64_aes(&mut st[0], key0)
                } else {
                    gen64_aes(&mut st[1], key1)
                }
            } else {
                gen64(&mut st[s])
            }
        };
        unsafe { _mm512_storeu_si512(buf.as_mut_ptr() as *mut __m512i, w) };
        if !NOLEAD {
            buf[0] = b'0' + 1 + (rng.next() % 9) as u8;
        }
        let mut blk4: u64 = 0;
        for i in 0..LEN {
            let d = (buf[i] - b'0') as u64;
            hist[d as usize] += 1;
            w6 = (w6 * 10 + d) % 1_000_000;
            w8 = (w8 * 10 + d) % 100_000_000;
            if w6 == W6 {
                c6 += 1;
            }
            if w8 == W8 {
                c8 += 1;
            }
            blk4 = blk4 * 10 + d;
            if i % 4 == 3 {
                hist4[blk4 as usize] += 1;
                blk4 = 0;
            }
        }
        total += LEN as u64;
        chunk += 1;
    }

    let el = t0.elapsed().as_secs_f64();
    println!(
        "\nцифр: {:.2} млрд за {:.1} сек ({:.0} млн/сек)",
        total as f64 / 1e9,
        el,
        total as f64 / el / 1e6
    );
    print!("доли цифр:");
    for d in 0..10 {
        print!(" {}={:.4}", d, hist[d] as f64 / total as f64);
    }
    println!();
    println!(
        "{}: {} (ожидание {:.0}, отношение {:.3})",
        W6,
        c6,
        total as f64 / 1e6,
        c6 as f64 / (total as f64 / 1e6)
    );
    println!(
        "{}: {} (ожидание {:.1}, отношение {:.3})",
        W8,
        c8,
        total as f64 / 1e8,
        c8 as f64 / (total as f64 / 1e8)
    );

    let blocks = total / 4;
    if blocks > 0 {
        let exp = blocks as f64 / 10000.0;
        let chi: f64 = hist4
            .iter()
            .map(|&c| {
                let d = c as f64 - exp;
                d * d / exp
            })
            .sum();
        let df = 9999.0;
        println!(
            "chi2(4 цифры, непересекающиеся): {:.0} при df={} -> chi2/df={:.4}, z={:+.2}",
            chi,
            df as u64,
            chi / df,
            (chi - df) / (2.0 * df).sqrt()
        );
    }
}