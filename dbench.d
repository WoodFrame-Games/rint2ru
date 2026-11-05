import std.stdio;
import std.conv;
import core.time : MonoTime;

void main(string[] args)
{
    ulong n = args.length > 1 ? args[1].to!ulong : 500_000_000UL;
    ulong x = 0x9E3779B97F4A7C15UL;
    ulong w = 0;
    ulong hits = 0;
    auto t0 = MonoTime.currTime;
    for (ulong i = 0; i < n; ++i)
    {
        x += 0x9E3779B97F4A7C15UL;
        ulong z = x;
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9UL;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EBUL;
        z ^= z >> 31;
        ulong d = z % 10;
        w = (w % 1_000_000_000UL) * 10 + d;
        if (w == 1_718_103_620UL) ++hits;
    }
    auto el = (MonoTime.currTime - t0).total!"nsecs" / 1e9;
    writefln("D: %.0f млн цифр/сек | приветов: %d", cast(double)n / el / 1e6, hits);
}