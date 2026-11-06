use std::time::Instant;

fn main() {
    let n: u64 = 400_000_000;
    let mut x: u64 = 0x9E3779B97F4A7C15;
    let mut w: u64 = 0;
    let mut hits: u64 = 0;
    let t0 = Instant::now();
    for _ in 0..n {
        x = x.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        let d = z % 10;
        w = (w % 1_000_000_000) * 10 + d;
        if w == 1718103620 {
            hits += 1;
        }
    }
    let el = t0.elapsed().as_secs_f64();
    println!("Rust: {:.0} млн/сек | hits {} | w {}", n as f64 / el / 1e6, hits, w);
}