#include <cstdio>
#include <cstdint>

static inline uint64_t splitmix64(uint64_t& x)
{
    uint64_t z = (x += 0x9E3779B97F4A7C15ULL);
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

int main()
{
    uint64_t x = 0x9E3779B97F4A7C15ULL;
    uint64_t buf = 0;
    int have = 0;
    unsigned long long hist[10] = {0};
    unsigned long long p2 = 0, p1 = 0;
    uint64_t w = 0;
    const unsigned long long N = 2000000000ULL;

    for (unsigned long long i = 0; i < N; ++i) {
        if (!have) { buf = splitmix64(x); have = 2; }
        uint32_t d = (uint32_t)(buf & 0xFFFFFFFFULL) % 10u;
        buf >>= 32;
        --have;
        hist[d]++;
        w = (w % 1000000000ULL) * 10ULL + d;
        if ((w % 1000000ULL) == 518214ULL) ++p2;
        if (w == 1718103620ULL) ++p1;
    }
    printf("CPU splitmix64: digits=%llu\n  histogram:", N);
    for (int d = 0; d < 10; ++d) printf(" %d:%.3f%%", d, 100.0 * hist[d] / N);
    printf("\n  другов: %llu (ожидание %.0f) | приветов: %llu (ожидание %.1f)\n",
           p2, (double)N * 1e-6, p1, (double)N * 1e-10);
    return 0;
}