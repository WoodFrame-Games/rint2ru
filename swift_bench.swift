import Foundation

let n: UInt64 = 400_000_000
var x: UInt64 = 0x9E3779B97F4A7C15
var w: UInt64 = 0
var hits: UInt64 = 0
let t0 = Date()
for _ in 0..<n {
    x = x &+ 0x9E3779B97F4A7C15
    var z = x
    z = (z ^ (z >> 30)) &* 0xBF58476D1CE4E5B9
    z = (z ^ (z >> 27)) &* 0x94D049BB133111EB
    z ^= z >> 31
    let d = z % 10
    w = (w % 1_000_000_000) &* 10 &+ d
    if w == 1718103620 { hits += 1 }
}
let el = Date().timeIntervalSince(t0)
print("Swift: \(Double(n) / el / 1e6) млн/сек | hits \(hits) | w \(w)")