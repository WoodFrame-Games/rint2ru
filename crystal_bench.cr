n = 400_000_000u64
x = 0x9E3779B97F4A7C15u64
w = 0u64
hits = 0u64
t0 = Time.monotonic
n.times do
  x &+= 0x9E3779B97F4A7C15u64
  z = x
  z = (z ^ (z >> 30)) &* 0xBF58476D1CE4E5B9u64
  z = (z ^ (z >> 27)) &* 0x94D049BB133111EBu64
  z ^= z >> 31
  d = z % 10u64
  w = (w % 1_000_000_000u64) &* 10u64 &+ d
  hits &+= 1u64 if w == 1_718_103_620u64
end
el = Time.monotonic - t0
puts "Crystal: #{(n.to_f64 / el.total_seconds / 1e6).round} млн/сек | hits #{hits} | w #{w}"