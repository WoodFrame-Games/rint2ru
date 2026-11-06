// Детерминированная проверка fused-фильтра: паттерн "1718" ставится
// на каждое смещение 0..63, включая переход prev -> cur.
use std::arch::x86_64::*;

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

fn main() {
    unsafe {
        let c0 = _mm512_set1_epi8(b'1' as i8);
        let c1 = _mm512_set1_epi8(b'7' as i8);
        let c2 = _mm512_set1_epi8(b'1' as i8);
        let c3 = _mm512_set1_epi8(b'8' as i8);
        let idx1 = _mm512_loadu_si512(IDX1.as_ptr() as *const __m512i);
        let idx2 = _mm512_loadu_si512(IDX2.as_ptr() as *const __m512i);
        let idx3 = _mm512_loadu_si512(IDX3.as_ptr() as *const __m512i);
        let pat = *b"1718";

        let mut fails = 0;
        for o in 0..64usize {
            let mut prev = [b'9'; 64];
            let mut cur = [b'9'; 64];
            for j in 0..4 {
                let pos = o + j;
                if pos < 64 {
                    prev[pos] = pat[j];
                } else {
                    cur[pos - 64] = pat[j];
                }
            }
            let p = _mm512_loadu_si512(prev.as_ptr() as *const __m512i);
            let c = _mm512_loadu_si512(cur.as_ptr() as *const __m512i);
            let s1 = _mm512_permutex2var_epi8(p, idx1, c);
            let s2 = _mm512_permutex2var_epi8(p, idx2, c);
            let s3 = _mm512_permutex2var_epi8(p, idx3, c);
            let m = _mm512_cmpeq_epi8_mask(p, c0)
                & _mm512_cmpeq_epi8_mask(s1, c1)
                & _mm512_cmpeq_epi8_mask(s2, c2)
                & _mm512_cmpeq_epi8_mask(s3, c3);
            let got = (m >> o) & 1;
            if got == 0 {
                println!("FAIL o={} mask={:064b}", o, m);
                fails += 1;
            }
            // фоновые '9' не должны давать лишних срабатываний
            if m.count_ones() != 1 {
                println!("EXTRA o={} mask={:064b}", o, m);
                fails += 1;
            }
        }
        println!("offsets 0..63: {} ошибок", fails);
    }
}