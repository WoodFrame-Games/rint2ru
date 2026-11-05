package bench

import "core:fmt"
import "core:time"

main :: proc() {
	n := u64(400_000_000)
	x := u64(0x9E3779B97F4A7C15)
	w := u64(0)
	hits := u64(0)
	t0 := time.now()
	for i in 0 ..< n {
		x += 0x9E3779B97F4A7C15
		z := x
		z = (z ~ (z >> 30)) * 0xBF58476D1CE4E5B9
		z = (z ~ (z >> 27)) * 0x94D049BB133111EB
		z ~= z >> 31
		d := z % 10
		w = (w % 1_000_000_000) * 10 + d
		if w == 1_718_103_620 {
			hits += 1
		}
	}
	el := time.duration_seconds(time.since(t0))
	fmt.printf("Odin: %.0f млн цифр/сек hits %d\n", f64(n) / el / 1e6, hits)
}