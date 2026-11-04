#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <thread>
#include <vector>

static int LEN = 60;
static constexpr int TAIL = 16;

static uint64_t g_target = 0;
static int TLEN = 0;

static std::atomic<uint64_t> g_total{0};
static std::atomic<bool> g_stop{false};
static std::atomic<bool> g_found{false};
static std::atomic<uint64_t> g_found_num{0};
static char g_ctx[256];
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

static void worker(uint64_t seed, uint64_t& local_out)
{
    RNG rng{seed};
    char buf[16 + 64];
    std::memset(buf, '0', (size_t)TAIL);
    const int TOT = TAIL + LEN;
    uint64_t pow10 = 1;
    for (int k = 1; k < TLEN; ++k) pow10 *= 10;
    uint64_t local = 0;

    for (;;) {
        for (int k = 0; k < LEN; ++k) {
            uint64_t r = rng.next();
            int d = (k == 0) ? 1 + (int)(r % 9) : (int)(r % 10);
            buf[TAIL + k] = (char)('0' + d);
        }
        local += 0;
        ++local;
        if ((local & 0xFFFULL) == 0) {
            g_total.fetch_add(0x1000, std::memory_order_relaxed);
            if (g_stop.load(std::memory_order_relaxed)) break;
        }

        uint64_t code = 0;
        for (int k = 0; k < TLEN; ++k)
            code = code * 10 + (uint64_t)(buf[k] - '0');
        for (int i = 0; i + TLEN <= TOT; ++i) {
            int j = i + TLEN;
            if (j > TAIL && code == g_target) {
                if (!g_found.exchange(true)) {
                    uint64_t numidx =
                        g_total.load(std::memory_order_relaxed) + local;
                    g_found_num.store(numidx, std::memory_order_relaxed);
                    int b = i - 18;
                    int e = j + 18;
                    if (b < 0) b = 0;
                    if (e > TOT) e = TOT;
                    char* p = g_ctx;
                    int n = 0;
                    for (int k = b; k < i; ++k) p[n++] = buf[k];
                    p[n++] = '[';
                    for (int k = i; k < j; ++k) p[n++] = buf[k];
                    p[n++] = ']';
                    for (int k = j; k < e; ++k) p[n++] = buf[k];
                    g_ctxlen.store(n);
                    g_stop.store(true, std::memory_order_relaxed);
                }
                std::memmove(buf, buf + LEN, (size_t)TAIL);
                local_out = local;
                return;
            }
            if (j < TOT)
                code = (code - (uint64_t)(buf[i] - '0') * pow10) * 10 +
                       (uint64_t)(buf[j] - '0');
        }
        std::memmove(buf, buf + LEN, (size_t)TAIL);
    }
    local_out = local;
}

int main(int argc, char** argv)
{
    const char* tgt = argc > 1 ? argv[1] : "1718103620518214";
    const double RUNTIME = argc > 2 ? std::atof(argv[2]) : 120.0;
    if (argc > 3) LEN = std::atoi(argv[3]);
    if (LEN < 20) LEN = 20;
    if (LEN > 64) LEN = 64;

    TLEN = (int)std::strlen(tgt);
    if (TLEN < 1 || TLEN > 19) {
        std::fprintf(stderr, "длина цели 1..19 цифр\n");
        return 1;
    }
    for (int k = 0; k < TLEN; ++k) {
        if (tgt[k] < '0' || tgt[k] > '9') {
            std::fprintf(stderr, "цель должна быть цифрами\n");
            return 1;
        }
        g_target = g_target * 10 + (uint64_t)(tgt[k] - '0');
    }

    const int threads = [] {
        unsigned h = std::thread::hardware_concurrency();
        return h ? (int)h : 2;
    }();
    std::printf("Цель: %s (%d цифр = %.0e вариантов) | поток: числа по %d цифр, хвост %d | потоков: %d\n\n",
                tgt, TLEN, std::pow(10.0, TLEN), LEN, TAIL, threads);
    std::fflush(stdout);

    const auto t0 = std::chrono::steady_clock::now();
    const uint64_t seed0 = (uint64_t)t0.time_since_epoch().count();

    std::vector<std::thread> ts;
    std::vector<uint64_t> locals((size_t)threads, 0);
    ts.reserve((size_t)threads);
    for (int t = 0; t < threads; ++t)
        ts.emplace_back([&, t] {
            worker(seed0 ^ (0x9E3779B97F4A7C15ULL * (uint64_t)(t + 1)),
                   locals[(size_t)t]);
        });

    double nxt = 10.0;
    while (!g_found.load(std::memory_order_relaxed)) {
        std::this_thread::sleep_for(std::chrono::milliseconds(500));
        const double el = std::chrono::duration<double>(
                              std::chrono::steady_clock::now() - t0)
                              .count();
        if (el >= RUNTIME) {
            g_stop.store(true, std::memory_order_relaxed);
            break;
        }
        if (el >= nxt) {
            std::printf("... %.0f сек: %llu чисел, %.1f млрд цифр, %.0f млн цифр/сек\n",
                        el, (unsigned long long)g_total.load(),
                        (double)g_total.load() * LEN / 1e9,
                        (double)g_total.load() * LEN / el / 1e6);
            std::fflush(stdout);
            nxt += 10.0;
        }
    }
    for (auto& th : ts) th.join();

    const auto t1 = std::chrono::steady_clock::now();
    const double el = std::chrono::duration<double>(t1 - t0).count();
    uint64_t total = g_total.load();
    for (uint64_t v : locals) total += v & 0xFFF;
    const double digits = (double)total * LEN;

    std::printf("\n========== РЕЗУЛЬТАТ ==========\n");
    std::printf("итераций (сгенерировано чисел): %llu\n", (unsigned long long)total);
    std::printf("потрачено времени: %.1f сек\n", el);
    std::printf("пройдено цифр: %.1f млрд\n", digits / 1e9);
    std::printf("скорость: %.2f млн чисел/сек = %.0f млн цифр/сек\n",
                (double)total / el / 1e6, digits / el / 1e6);

    if (g_found.load()) {
        int n = g_ctxlen.load();
        std::string ctx(g_ctx, (size_t)n);
        std::printf("\nНАЙДЕНО на числе № %llu за %.1f сек\n",
                    (unsigned long long)g_found_num.load(), el);
        std::printf("контекст ленты: ...%s...\n", ctx.c_str());
    } else {
        double expected_digits = std::pow(10.0, TLEN);
        double need_sec = el * expected_digits / digits;
        std::printf("\nНЕ НАЙДЕНО за %.1f сек\n", el);
        std::printf("пройдено %.2e цифр из ожидаемых %.2e (%.4f%% пути)\n",
                    digits, expected_digits, digits / expected_digits * 100.0);
        std::printf("ожидаемое время до находки при этой скорости: %.2e сек (%.0f суток, ~%.1f лет)\n",
                    need_sec, need_sec / 86400.0, need_sec / 86400.0 / 365.0);
    }
    return 0;
}