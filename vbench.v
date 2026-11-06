import time

fn main() {
	n := 400_000_000
	mut x := u64(0x9E3779B97F4A7C15)
	mut w := u64(0)
	mut hits := u64(0)
	t0 := time.now()
	for _ in 0 .. n {
		x += 0x9E3779B97F4A7C15
		mut z := x
		z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9
		z = (z ^ (z >> 27)) * 0x94D049BB133111EB
		z ^= z >> 31
		d := z % 10
		w = (w % 1_000_000_000) * 10 + d
		if w == 1718103620 {
			hits++
		}
	}
	el := f64(time.now().unix_nano() - t0.unix_nano()) / 1e9
	speed := f64(n) / el / 1e6
	println('V: ${speed:.0f} млн/сек | hits ${hits} | w ${w}')
}