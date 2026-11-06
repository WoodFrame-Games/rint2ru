#define _GNU_SOURCE
#include <sched.h>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <immintrin.h>
#include <mutex>
#include <thread>
#include <vector>

static constexpr int CHUNKS = 128;
static constexpr int LEN = 60;
static constexpr int NEWD = CHUNKS * LEN;
static constexpr int S = 32 + NEWD;
static constexpr int PMAX = S - 24;

static int GAPMAX = 10000;

static uint64_t P1_LO = 0;
static uint16_t P1_HI = 0;
static uint64_t P2 = 0;

static std::atomic<uint64_t> g_digits{0};
static std::atomic<uint64_t> g_p1hits{0};
static std::atomic<uint64_t> g_p2hits{0};
static std::atomic<uint64_t> g_finds{0};
static std::atomic<bool> g_stop{false};
static std::mutex g_om;
static FILE* g_pf = nullptr;

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

struct Ring {
    uint64_t e_abs[8];
    char ctx[8][64];
    uint8_t clen[8];
    int idx;
};

static void worker(uint64_t seed, int tid)
{
    cpu_set_t set;
    CPU_ZERO(&set);
    CPU_SET(tid, &set);
    sched_setaffinity(0, sizeof(set), &set);

    RNG rng{seed};
    __m256i st = _mm256_setr_epi32(
        (int)(uint32_t)rng.next(), (int)(uint32_t)rng.next(),
        (int)(uint32_t)rng.next(), (int)(uint32_t)rng.next(),
        (int)(uint32_t)rng.next(), (int)(uint32_t)rng.next(),
        (int)(uint32_t)rng.next(), (int)(uint32_t)rng.next());
    static thread_local char buf[S + 32];
    std::memset(buf, '0', 32);
    uint64_t localdigits = 0;

    Ring ring;
    std::memset(&ring, 0, sizeof ring);
    for (int i = 0; i < 8; ++i) ring.e_abs[i] = UINT64_MAX;

    const __m256i ones = _mm256_set1_epi8('1');
    const __m256i fives = _mm256_set1_epi8('5');

    for (;;) {
        // Период xorshift32-вектора ~2^32 итераций (~6.4e10 цифр), поэтому
        // пересейваем состояние каждые NEWD=7680 цифр: поток не повторяется.
        st = _mm256_set_epi64x(
            (long long)rng.next(), (long long)rng.next(),
            (long long)rng.next(), (long long)rng.next());
        int k = 0;
        while (k < NEWD) {
            gen16(st, buf + 32 + k + 0);
            gen16(st, buf + 32 + k + 16);
            gen16(st, buf + 32 + k + 32);
            gen16(st, buf + 32 + k + 48);
            buf[32 + k] = (char)('0' + 1 + (int)(rng.next() % 9));
            k += LEN;
        }
        const uint64_t base = localdigits - NEWD;
        localdigits += NEWD;
        g_digits.fetch_add(NEWD, std::memory_order_relaxed);
        if (g_stop.load(std::memory_order_relaxed)) break;

        for (int p0 = 11; p0 + 32 <= S; p0 += 32) {
            __m256i v = _mm256_loadu_si256((const __m256i*)(buf + p0));
            unsigned m1 = (unsigned)_mm256_movemask_epi8(
                _mm256_cmpeq_epi8(v, ones));
            while (m1) {
                int bit = __builtin_ctz(m1);
                m1 &= m1 - 1;
                int p = p0 + bit;
                if (p > PMAX) break;
                if (ld64(buf + p) != P1_LO) continue;
                if (ld16(buf + p + 8) != P1_HI) continue;
                g_p1hits.fetch_add(1, std::memory_order_relaxed);
                int c = ring.idx & 7;
                ring.e_abs[c] = base + (uint64_t)(p + 10) - 32;
                int b0 = p - 20;
                if (b0 < 0) b0 = 0;
                int e0 = p + 30;
                if (e0 > S) e0 = S;
                int n = 0;
                for (int q = b0; q < e0 && n < 60; ++q) {
                    if (q == p) ring.ctx[c][n++] = '[';
                    if (q == p + 10) ring.ctx[c][n++] = ']';
                    ring.ctx[c][n++] = buf[q];
                }
                ring.clen[c] = (uint8_t)n;
                ring.idx++;
            }
            unsigned m5 = (unsigned)_mm256_movemask_epi8(
                _mm256_cmpeq_epi8(v, fives));
            while (m5) {
                int bit = __builtin_ctz(m5);
                m5 &= m5 - 1;
                int p = p0 + bit;
                if (p > PMAX) break;
                if ((ld64(buf + p) & 0xFFFFFFFFFFFFULL) != P2) continue;
                g_p2hits.fetch_add(1, std::memory_order_relaxed);
                const uint64_t s_abs = base + (uint64_t)p - 32;
                for (int i = 0; i < 8; ++i) {
                    uint64_t e = ring.e_abs[i];
                    if (e == UINT64_MAX) continue;
                    if (s_abs < e) continue;
                    uint64_t gap = s_abs - e;
                    if (gap > (uint64_t)GAPMAX) continue;
                    int b0 = p - 20;
                    if (b0 < 0) b0 = 0;
                    int e0 = p + 26;
                    if (e0 > S) e0 = S;
                    char ctx2[64];
                    int n = 0;
                    for (int q = b0; q < e0 && n < 60; ++q) {
                        if (q == p) ctx2[n++] = '{';
                        if (q == p + 6) ctx2[n++] = '}';
                        ctx2[n++] = buf[q];
                    }
                    ctx2[n] = 0;
                    char ctx1[64];
                    int cl = ring.clen[i];
                    std::memcpy(ctx1, ring.ctx[i], (size_t)cl);
                    ctx1[cl] = 0;
                    g_finds.fetch_add(1, std::memory_order_relaxed);
                    std::lock_guard<std::mutex> lk(g_om);
                    if (g_pf) {
                        std::fprintf(g_pf, "gap=%llu | привет@%llu: %s | друг: %s\n",
                                     (unsigned long long)gap,
                                     (unsigned long long)e,
                                     ctx1, ctx2);
                        std::fflush(g_pf);
                    }
                    std::printf("НАЙДЕНО: зазор %llu\n  привет: %s\n  друг:   %s\n",
                                (unsigned long long)gap, ctx1, ctx2);
                    std::fflush(stdout);
                    break;
                }
            }
        }
        std::memmove(buf, buf + NEWD, 32);
    }
}

int main(int argc, char** argv)
{
    const double RUNTIME = argc > 1 ? std::atof(argv[1]) : 1800.0;
    if (argc > 2) GAPMAX = std::atoi(argv[2]);

    std::memcpy(&P1_LO, "1718103620", 8);
    std::memcpy(&P1_HI, "1718103620" + 8, 2);
    P2 = 0;
    std::memcpy(&P2, "518214", 6);

    g_pf = std::fopen("pairs.txt", "a");
    const int threads = [] {
        unsigned h = std::thread::hardware_concurrency();
        return h ? (int)h : 2;
    }();
    std::printf("ПОИСК ПАР: «привет» + «друг», зазор <= %d цифр | %d потока | %.0f сек\n\n",
                GAPMAX, threads, RUNTIME);
    std::fflush(stdout);

    const auto t0 = std::chrono::steady_clock::now();
    const uint64_t seed0 = (uint64_t)t0.time_since_epoch().count();
    std::vector<std::thread> ts;
    for (int t = 0; t < threads; ++t)
        ts.emplace_back([&, t] { worker(seed0 ^ (0x9E3779B97F4A7C15ULL * (uint64_t)(t + 1)), t); });

    double nxt = 30.0;
    for (;;) {
        std::this_thread::sleep_for(std::chrono::milliseconds(250));
        const double el = std::chrono::duration<double>(
                              std::chrono::steady_clock::now() - t0).count();
        if (el >= RUNTIME) {
            g_stop.store(true, std::memory_order_relaxed);
            break;
        }
        if (el >= nxt) {
            const uint64_t d = g_digits.load();
            std::printf("... %.0f сек | %.1f млрд цифр | %.0f млн цифр/сек | приветов: %llu | пар: %llu\n",
                        el, (double)d / 1e9, (double)d / el / 1e6,
                        (unsigned long long)g_p1hits.load(),
                        (unsigned long long)g_finds.load());
            std::fflush(stdout);
            nxt += 30.0;
        }
    }
    for (auto& th : ts) th.join();
    const auto t1 = std::chrono::steady_clock::now();
    const double el = std::chrono::duration<double>(t1 - t0).count();
    const uint64_t d = g_digits.load();

    std::printf("\n========== РЕЗУЛЬТАТ ==========\n");
    std::printf("цифр: %.1f млрд | время: %.0f сек | скорость: %.0f млн цифр/сек\n",
                (double)d / 1e9, el, (double)d / el / 1e6);
    std::printf("приветов: %llu | друга: %llu | пар с зазором <= %d: %llu\n",
                (unsigned long long)g_p1hits.load(),
                (unsigned long long)g_p2hits.load(), GAPMAX,
                (unsigned long long)g_finds.load());
    if (!g_finds.load()) {
        const double need = 1.0 / (1e-10 * (double)(GAPMAX + 1) * 1e-6);
        std::printf("не найдено (ожидание ~%.1e цифр => ~%.1f мин)\n",
                    need, need / ((double)d / el) / 60.0);
    }
    if (g_pf) std::fclose(g_pf);
    return 0;
}