#include <stdio.h>
#include <stdint.h>
#include <time.h>

int main(void)
{
    const uint64_t n = 400000000ULL;
    uint64_t x = 0x9E3779B97F4A7C15ULL, w = 0, hits = 0;
    struct timespec a, b;
    clock_gettime(CLOCK_MONOTONIC, &a);
    for (uint64_t i = 0; i < n; i++) {
        x += 0x9E3779B97F4A7C15ULL;
        uint64_t z = x;
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
        z ^= z >> 31;
        uint64_t d = z % 10;
        w = (w % 1000000000ULL) * 10 + d;
        if (w == 1718103620ULL) hits++;
    }
    clock_gettime(CLOCK_MONOTONIC, &b);
    double el = (double)(b.tv_sec - a.tv_sec) + 1e-9 * (double)(b.tv_nsec - a.tv_nsec);
    printf("C: %.0f млн/сек | hits %llu | w %llu\n", (double)n / el / 1e6,
           (unsigned long long)hits, (unsigned long long)w);
    return 0;
}