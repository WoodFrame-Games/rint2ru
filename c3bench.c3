module bench;
import std::io;

fn void main()
{
    ulong n = 400_000_000;
    ulong x = 0x9E3779B97F4A7C15;
    ulong w = 0;
    ulong hits = 0;
    for (ulong i = 0; i < n; i++)
    {
        x += 0x9E3779B97F4A7C15;
        ulong z = x;
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EB;
        z ^= z >> 31;
        ulong d = z % 10;
        w = (w % 1_000_000_000) * 10 + d;
        if (w == 1_718_103_620) hits++;
    }
    io::printfn("C3: hits %d w %d", hits, w);
}