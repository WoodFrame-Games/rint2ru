const std = @import("std");

pub fn main() !void {
    const n: u64 = 400_000_000;

    var x: u64 = 0x9E3779B97F4A7C15;
    var w: u64 = 0;
    var hits: u64 = 0;
    var i: u64 = 0;
    while (i < n) : (i += 1) {
        x +%= 0x9E3779B97F4A7C15;
        var z: u64 = x;
        z = (z ^ (z >> 30)) *% 0xBF58476D1CE4E5B9;
        z = (z ^ (z >> 27)) *% 0x94D049BB133111EB;
        z ^= z >> 31;
        const d: u64 = z % 10;
        w = (w % 1_000_000_000) * 10 + d;
        if (w == 1_718_103_620) hits += 1;
    }
    std.debug.print("Zig: hits={d} w={d}\n", .{ hits, w });
}