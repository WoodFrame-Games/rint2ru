import std/times

const N = 400_000_000'u64
const G = 0x9E3779B97F4A7C15'u64
const C1 = 0xBF58476D1CE4E5B9'u64
const C2 = 0x94D049BB133111EB'u64

var x = G
var w = 0'u64
var hits = 0'u64

let t0 = epochTime()
for i in 0 ..< N:
  x += G
  var z = x
  z = (z xor (z shr 30)) * C1
  z = (z xor (z shr 27)) * C2
  z = z xor (z shr 31)
  let d = z mod 10
  w = (w mod 1_000_000_000'u64) * 10 + d
  if w == 1_718_103_620'u64:
    inc hits
let el = epochTime() - t0
echo "Nim: ", int(float(N) / el / 1e6), " млн цифр/сек hits ", hits, " w ", w