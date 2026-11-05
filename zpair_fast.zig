const std = @import("std");

const V16u32 = @Vector(16, u32);
const V16u16 = @Vector(16, u16);
const V64u8 = @Vector(64, u8);
const V32u8 = @Vector(32, u8);
const V32i32 = @Vector(32, i32);

const CHUNKS: usize = 128;
const LEN: usize = 60;
const NEWD: usize = CHUNKS * LEN;
const TAIL: usize = 32;
const S: usize = TAIL + NEWD;
const PMAX: usize = S - 24;

const P1LO: u64 = 0x3633303138313731;
const P1HI: u16 = 0x3032;
const P2: u64 = 0x343132383135;
const GAPMAX: usize = 5;

const SEL = blk: {
    var m: [32]i32 = undefined;
    for (0..16) |i| {
        m[2 * i] = @intCast(4 * i);
        m[2 * i + 1] = @intCast(4 * i + 1);
    }
    break :blk @as(V32i32, m);
};

var g_p1 = std.atomic.Value(u64).init(0);
var g_pairs = std.atomic.Value(u64).init(0);

inline fn ld64(p: [*]const u8) u64 {
    return std.mem.readInt(u64, p[0..8], .little);
}
inline fn ld16(p: [*]const u8) u16 {
    return std.mem.readInt(u16, p[0..2], .little);
}
inline fn ldv(p: [*]const u8) V64u8 {
    return @as(*align(1) const V64u8, @ptrCast(p)).*;
}

inline fn gen32(st: *V16u32, out: [*]u8) void {
    var x = st.*;
    x ^= x >> @splat(@as(u5, 13));
    x ^= x << @splat(@as(u5, 17));
    x ^= x >> @splat(@as(u5, 5));
    st.* = x;

    const sh8a: @Vector(16, u5) = @splat(8);
    const sh24: @Vector(16, u5) = @splat(24);
    const v100: V16u32 = @splat(100);
    const pair32: V16u32 = ((x >> sh8a) *% v100) >> sh24;

    const v205: V16u32 = @splat(205);
    const v10: V16u32 = @splat(10);
    const v48: V16u32 = @splat(48);
    const sh11: @Vector(16, u5) = @splat(11);
    const t: V16u32 = (pair32 *% v205) >> sh11;
    const u: V16u32 = pair32 -% t *% v10;

    const word: V16u32 = (t +% v48) | ((u +% v48) << sh8a);
    const wb: V64u8 = @bitCast(word);
    const pk: V32u8 = @shuffle(u8, wb, wb, SEL);
    @as(*align(1) V32u8, @ptrCast(out)).* = pk;
}

fn worker(tid: u64, blocks: u64) void {
    var seed: u64 = 0x9E3779B97F4A7C15 *% (tid +% 1);
    var st: V16u32 = undefined;
    inline for (0..16) |i| {
        seed +%= 0x9E3779B97F4A7C15;
        var z = seed;
        z = (z ^ (z >> 30)) *% 0xBF58476D1CE4E5B9;
        z = (z ^ (z >> 27)) *% 0x94D049BB133111EB;
        z ^= z >> 31;
        st[i] = @truncate(z);
    }

    var buf: [S + 64]u8 = undefined;
    @memset(buf[0..TAIL], '0');

    var b: u64 = 0;
    while (b < blocks) : (b += 1) {
        var k: usize = 0;
        while (k < NEWD) : (k += LEN) {
            gen32(&st, @ptrCast(&buf[TAIL + k]));
            gen32(&st, @ptrCast(&buf[TAIL + k + 32]));
        }

        const base: [*]const u8 = @ptrCast(&buf);
        var p0: usize = 11;
        while (p0 + 66 <= S) : (p0 += 64) {
            const m1: u64 = @bitCast(ldv(base + p0) == @as(V64u8, @splat('1')));
            const m7: u64 = @bitCast(ldv(base + p0 + 1) == @as(V64u8, @splat('7')));
            const m10: u64 = @bitCast(ldv(base + p0 + 2) == @as(V64u8, @splat('1')));
            var m: u64 = m1 & m7 & m10;
            while (m != 0) {
                const bit: usize = @ctz(m);
                m &= m - 1;
                const p = p0 + bit;
                if (p > PMAX) break;
                if (ld64(base + p) != P1LO) continue;
                if (ld16(base + p + 8) != P1HI) continue;
                _ = g_p1.fetchAdd(1, .monotonic);
                var g: usize = 0;
                while (g <= GAPMAX) : (g += 1) {
                    if ((ld64(base + p + 10 + g) & 0xFFFFFFFFFFFF) != P2) continue;
                    _ = g_pairs.fetchAdd(1, .monotonic);
                    break;
                }
            }
        }

        @memcpy(buf[0..TAIL], buf[NEWD .. NEWD + TAIL]);
    }
}

pub fn main() !void {
    const blocks: u64 = 1_000_000;
    var threads: [2]std.Thread = undefined;
    for (0..2) |t| {
        threads[t] = try std.Thread.spawn(.{}, worker, .{ t, blocks });
    }
    for (&threads) |*h| h.join();

    const digits: u64 = 2 * blocks * NEWD;
    std.debug.print("Zig AVX-512 (shuffle-pack): цифр {d} | приветов {d} | пар {d}\n", .{
        digits,
        g_p1.load(.monotonic),
        g_pairs.load(.monotonic),
    });
}