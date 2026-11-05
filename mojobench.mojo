fn main():
    var n: UInt64 = 400_000_000
    var x: UInt64 = 0x9E3779B97F4A7C15
    var w: UInt64 = 0
    var hits: UInt64 = 0
    for _ in range(n):
        x += 0x9E3779B97F4A7C15
        var z: UInt64 = x
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9
        z = (z ^ (z >> 27)) * 0x94D049BB133111EB
        z = z ^ (z >> 31)
        var d: UInt64 = z % 10
        w = (w % 1_000_000_000) * 10 + d
        if w == 1_718_103_620:
            hits += 1
    print("Mojo: hits", hits, "w", w)