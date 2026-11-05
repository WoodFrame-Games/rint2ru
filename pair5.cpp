#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <immintrin.h>
#include <thread>
#include <vector>

static constexpr int CHUNKS = 128;
static constexpr int LEN = 60;
static constexpr int NEWD = CHUNKS * LEN;
static constexpr int S = 32 + NEWD;
static constexpr int PMAX = S - 24;

static int GAPMAX = 5;

static uint64_t P1_LO = 0;
static uint16_t P1_HI = 0;
static uint64_t P2 = 0;

static std::atomic<uint64_t> g_digits{0};
static std::atomic<uint64_t> g_p1hits{0};
static std::atomic<bool> g_found{false};
static std::atomic<bool> g_stop{false};
static std::atomic<uint64_t> g_gap{0};
static std::atomic<uint64_t> g_absdigits{0};
static char g_ctx[160];
static std::atomic<int> g_ctxlen{0};

struct RNG {
    uint64_t s;
    uint64_t next()
    {
        s += 0x9E3779B97F4A7C15ULL;
        uint64_t z = s;
        z ^= z >> 30; z *= 0xBF58476D1CE4E5B9ULL;
        z ^= z >> 27; z *= 0x94D049BB133111EBULL;
        z ^= z >> 31;
        return z;
    }
};

static inline __m256i mulhi32(__m256i a, __m256i b)
{
    __m256i e = _mm256_mul_epu32(a, b);
    __m256i o = _mm256_mul_epu32(_mm256_srli_epi64(a, 32),
                                 _mm256_srli_epi64(b, 32));
    __m256i he = _mm256_srli_epi64(e, 32);
    __m256i ho = _mm256_slli_epi64(_mm256_srli_epi64(o, 32), 32);
    return _mm256_blend_epi32(he, ho, 0xAA);
}

static inline __m256i xs32(__m256i x)
{
    x = _mm256_xor_si256(x, _mm256_srli_epi32(x, 13));
    x = _mm256_xor_si256(x, _mm256_slli_epi32(x, 17));
    x = _mm256_xor_si256(x, _mm256_srli_epi32(x, 5));
    return x;
}

static inline void gen16(__m256i& st, char* out)
{
    st = xs32(st);
    __m256i q100 = _mm256_srli_epi32(
        mulhi32(st, _mm256_set1_epi32((int)0x51EB851F)), 5);
    __m256i r = _mm256_sub_epi32(st,
        _mm256_mullo_epi32(q100, _mm256_set1_epi32(100)));
    __m256i t = _mm256_srli_epi32(
        _mm256_mullo_epi32(r, _mm256_set1_epi32(205)), 11);
    __m256i u = _mm256_sub_epi32(r,
        _mm256_mullo_epi32(t, _mm256_set1_epi32(10)));
    __m256i v = _mm256_or_si256(
        _mm256_add_epi32(t, _mm256_set1_epi32('0')),
        _mm256_slli_epi32(_mm256_add_epi32(u, _mm256_set1_epi32('0')), 8));
    const __m256i sh = _mm256_setr_epi8(
        0, 1, 4, 5, 8, 9, 12, 13, -1, -1, -1, -1, -1, -1, -1, -1,
        0, 1, 4, 5, 8, 9, 12, 13, -1, -1, -1, -1, -1, -1, -1, -1);
    __m256i p = _mm256_shuffle_epi8(v, sh);
    __m256i g = _mm256_permute4x64_epi64(p, 0x08);
    _mm_storeu_si128((__m128i*)out, _mm256_castsi256_si128(g));
}

static inline uint64_t ld64(const char* p)
{
    uint64_t v;
    std::memcpy(&v, p, 8);
    return v;
}

static inline uint16_t ld16(const char* p)
{
    uint16_t v;
    std::memcpy(&v, p, 2);
    return v;
}

static void worker(uint64_t seed)
{
    RNG rng{seed};
    __m256i st = _mm256_setr_epi32(
        (int)(uint32_t)rng.next(), (int)(uint32_t)rng.next(),
        (int)(uint32_t)rng.next(), (int)(uint32_t)rng.next(),
        (int)(uint32_t)rng.next(), (int)(uint32_t)rng.next(),
        (int)(uint32_t)rng.next(), (int)(uint32_t)rng.next());
    static thread_local char buf[S + 32];
    std::memset(buf, '0', 32);
    uint64_t localdigits = 0;

    const __m256i ones = _mm256_set1_epi8('1');

    for (;;) {
        int k = 0;
        while (k < NEWD) {
            gen16(st, buf + 32 + k + 0);
            gen16(st, buf + 32 + k + 16);
            gen16(st, buf + 32 + k + 32);
            gen16(st, buf + 32 + k + 48);
            buf[32 + k] = (char)('0' + 1 + (int)(rng.next() % 9));
            k += LEN;
        }
        localdigits += NEWD;
        g_digits.fetch_add(NEWD, std::memory_order_relaxed);
        if (g_stop.load(std::memory_order_relaxed)) break;

        for (int p0 = 11; p0 + 32 <= S; p0 += 32) {
            __m256i v = _mm256_loadu_si256((const __m256i*)(buf + p0));
            unsigned m = (unsigned)_mm256_movemask_epi8(
                _mm256_cmpeq_epi8(v, ones));
            while (m) {
                int bit = __builtin_ctz(m);
                m &= m - 1;
                int p = p0 + bit;
                if (p > PMAX) break;
                if (ld64(buf + p) != P1_LO) continue;
                if (ld16(buf + p + 8) != P1_HI) continue;
                g_p1hits.fetch_add(1, std::memory_order_relaxed);
                for (int g2 = 0; g2 <= GAPMAX; ++g2) {
                    if ((ld64(buf + p + 10 + g2) & 0xFFFFFFFFFFFFULL) != P2)
                        continue;
                    if (!g_found.exchange(true)) {
                        int b = p - 20;
                        if (b < 0) b = 0;
                        int e = p + 10 + g2 + 6 + 20;
                        if (e > S) e = S;
                        int n = 0;
                        for (int q = b; q < e; ++q) {
                            if (q == p) g_ctx[n++] = '[';
                            if (q == p + 10 + g2) g_ctx[n++] = ']';
                            if (q == p + 10) g_ctx[n++] = '|';
                            g_ctx[n++] = buf[q];
                        }
                        g_ctxlen.store(n);
                        g_gap.store((uint64_t)g2);
                        g_absdigits.store(localdigits);
                        g_stop.store(true, std::memory_order_relaxed);
                    }
                    break;
                }
                if (g_stop.load(std::memory_order_relaxed)) break;
            }
            if (g_stop.load(std::memory_order_relaxed)) break;
        }
        if (g_stop.load(std::memory_order_relaxed)) break;
        std::memmove(buf, buf + NEWD, 32);
    }
}

int main(int argc, char** argv)
{
    const double RUNTIME = argc > 1 ? std::atof(argv[1]) : 300.0;
    if (argc > 2) GAPMAX = std::atoi(argv[2]);

    std::memcpy(&P1_LO, "1718103620", 8);
    std::memcpy(&P1_HI, "1718103620" + 8, 2);
    P2 = 0;
    std::memcpy(&P2, "518214", 6);

    const int threads = [] {
        unsigned h = std::thread::hardware_concurrency();
        return h ? (int)h : 2;
    }();
    std::printf("ПОИСК ПАРЫ: привет(1718103620) + друг(518214), зазор <= %d цифр\n", GAPMAX);
    std::printf("поток цифр, %d потока, AVX2-генерация + SIMD-фильтр\n\n", threads);
    std::fflush(stdout);

    const auto t0 = std::chrono::steady_clock::now();
    const uint64_t seed0 = (uint64_t)t0.time_since_epoch().count();
    std::vector<std::thread> ts;
    for (int t = 0; t < threads; ++t)
        ts.emplace_back([&, t] { worker(seed0 ^ (0x9E3779B97F4A7C15ULL * (uint64_t)(t + 1))); });

    double nxt = 10.0;
    while (!g_found.load(std::memory_order_relaxed)) {
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
        const double el = std::chrono::duration<double>(
                              std::chrono::steady_clock::now() - t0).count();
        if (el >= RUNTIME) {
            g_stop.store(true, std::memory_order_relaxed);
            break;
        }
        if (el >= nxt) {
            const uint64_t d = g_digits.load();
            std::printf("... %.0f сек | %.1f млрд цифр | %.0f млн цифр/сек | приветов: %llu\n",
                        el, (double)d / 1e9, (double)d / el / 1e6,
                        (unsigned long long)g_p1hits.load());
            std::fflush(stdout);
            nxt += 10.0;
        }
    }
    for (auto& th : ts) th.join();
    const auto t1 = std::chrono::steady_clock::now();
    const double el = std::chrono::duration<double>(t1 - t0).count();
    const uint64_t d = g_digits.load();

    std::printf("\n========== РЕЗУЛЬТАТ ==========\n");
    std::printf("цифр: %.1f млрд | время: %.1f сек | скорость: %.0f млн цифр/сек\n",
                (double)d / 1e9, el, (double)d / el / 1e6);
    std::printf("вхождений «привет»: %llu\n", (unsigned long long)g_p1hits.load());
    if (g_found.load()) {
        std::printf("\nНАЙДЕНО! зазор %llu цифр\n", (unsigned long long)g_gap.load());
        std::printf("...%s...\n", g_ctx);
    } else {
        const double need = 1.0 / (1e-10 * (double)(GAPMAX + 1) * 1e-6);
        const double eta = need / ((double)d / el);
        std::printf("\nне найдено (ожидание ~%.1e цифр => ~%.1f суток при этой скорости)\n",
                    need, eta / 86400.0);
    }
    return 0;
}