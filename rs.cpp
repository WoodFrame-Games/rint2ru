#include <atomic>
#include <charconv>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <random>
#include <thread>
#include <vector>

static constexpr const char* TARGET = "1718103620";
static constexpr int TLEN = 10;

static inline bool contains(const char* s, int n)
{
    for (int i = 0; i + TLEN <= n; ++i)
    {
        if (s[i] == '1' && s[i + 1] == '7')
        {
            int j = 2;
            while (j < TLEN && s[i + j] == TARGET[j])
                ++j;
            if (j == TLEN)
                return true;
        }
    }
    return false;
}

int main()
{
    const int threads = [] {
        const unsigned h = std::thread::hardware_concurrency();
        return h ? static_cast<int>(h) : 8;
    }();

    static constexpr uint64_t LO   = 100000000000000000ULL;
    static constexpr uint64_t SPAN = 8900000000000000000ULL;

    std::atomic<uint64_t> attempts{0};
    std::atomic<uint64_t> found{0};
    std::atomic<bool> done{false};

    const auto t0 = std::chrono::steady_clock::now();
    const uint64_t seed0 =
        static_cast<uint64_t>(t0.time_since_epoch().count());

    std::vector<std::thread> ts;
    ts.reserve(threads);

    for (int t = 0; t < threads; ++t)
    {
        ts.emplace_back([&, t]
        {
            std::mt19937_64 rng(
                seed0 ^ (0x9E3779B97F4A7C15ULL * static_cast<uint64_t>(t + 1)));
            std::uniform_int_distribution<uint64_t> dist(LO, LO + SPAN - 1);

            char buf[32];
            uint64_t local = 0;

            for (;;)
            {
                const uint64_t number = dist(rng);
                ++local;

                if ((local & 0xFFFFULL) == 0)
                {
                    attempts.fetch_add(0x10000, std::memory_order_relaxed);
                    if (done.load(std::memory_order_relaxed))
                        return;
                }

                const auto r = std::to_chars(buf, buf + sizeof buf, number);
                const int n = static_cast<int>(r.ptr - buf);

                if (contains(buf, n))
                {
                    attempts.fetch_add(local & 0xFFFF, std::memory_order_relaxed);
                    uint64_t expected = 0;
                    found.compare_exchange_strong(expected, number);
                    done.store(true, std::memory_order_relaxed);
                    return;
                }
            }
        });
    }

    for (auto& th : ts)
        th.join();

    const auto t1 = std::chrono::steady_clock::now();
    const double sec = std::chrono::duration<double>(t1 - t0).count();
    const uint64_t att = attempts.load();

    std::printf("Поиск: %s\n", TARGET);
    std::printf("========== РЕЗУЛЬТАТ (C++) ==========\n");
    std::printf("Найденное число: %llu\n",
                static_cast<unsigned long long>(found.load()));
    std::printf("Всего генераций: %llu\n",
                static_cast<unsigned long long>(att));
    std::printf("Время поиска: %.4f сек\n", sec);
    std::printf("Скорость: %.0f генераций/сек\n",
                static_cast<double>(att) / sec);
    std::printf("Потоков: %d\n", threads);

    return 0;
}
