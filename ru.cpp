#include <algorithm>
#include <array>
#include <atomic>
#include <charconv>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <set>
#include <string>
#include <thread>
#include <vector>

static constexpr const char* ALPH =
    "\xD0\xB0\xD0\xB1\xD0\xB2\xD0\xB3\xD0\xB4\xD0\xB5\xD1\x91\xD0\xB6"
    "\xD0\xB7\xD0\xB8\xD0\xB9\xD0\xBA\xD0\xBB\xD0\xBC\xD0\xBD\xD0\xBE"
    "\xD0\xBF\xD1\x80\xD1\x81\xD1\x82\xD1\x83\xD1\x84\xD1\x85\xD1\x86"
    "\xD1\x87\xD1\x88\xD1\x89\xD1\x8A\xD1\x8B\xD1\x8C\xD1\x8D\xD1\x8E"
    "\xD1\x8F";

static constexpr uint64_t LO   = 100000000000000000ULL;
static constexpr uint64_t SPAN = 8900000000000000000ULL;

struct Entry { uint64_t code; int32_t wid; };
struct Sel   { uint8_t i, j; int32_t wid; };
struct Cand  {
    uint64_t num;
    double score;
    int nw, cov;
    Sel sel[6];
};

static std::vector<Entry> tbl;
static size_t tmask = 0;
static std::vector<std::string> wordstr;
static std::vector<double> wlog;
static std::vector<int> enclen;
static bool valid_len[24] = {};
static int16_t lmap[65536];
static std::vector<uint8_t> wcat;
static std::set<std::string> g_stopw;
static std::atomic<uint64_t> g_gram{0};

static const char* VSUFS[] = {
    "ть", "тся", "ться", "ла", "ло", "ли", "ет", "ит", "ут", "ют",
    "ат", "ят", "ем", "им", "ся", "сь", "ешь", "ишь", "ешься", "ишься",
    "лась", "лось", "лись", "лся"
};

static bool verb_like(const std::string& w)
{
    for (const char* s : VSUFS) {
        size_t m = std::strlen(s);
        if (w.size() > m && w.compare(w.size() - m, m, s) == 0) return true;
    }
    return false;
}

static const char* STOPW[] = {
    "я", "ты", "он", "она", "мы", "вы", "они", "мне", "меня", "тебя",
    "тебе", "вас", "вам", "нас", "нам", "их", "его", "ее", "ей", "им",
    "себя", "себе", "сам", "там", "тут", "вот", "где", "куда", "как",
    "что", "чтобы", "зачем", "почему", "когда", "уже", "еще", "ещё",
    "тоже", "так", "да", "нет", "не", "ни", "но", "и", "а", "или",
    "ли", "же", "бы", "уж", "все", "всё", "это"
};

static inline uint64_t mixh(uint64_t x)
{
    x *= 0x9E3779B97F4A7C15ULL;
    x ^= x >> 29;
    x *= 0xBF58476D1CE4E5B9ULL;
    x ^= x >> 32;
    return x;
}

static bool load_dict(const char* path, int take_n)
{
    for (const char* s : STOPW) g_stopw.insert(s);
    std::memset(lmap, -1, sizeof lmap);
    const unsigned char* p = (const unsigned char*)ALPH;
    for (int i = 0; *p; ++i, p += 2)
        lmap[(p[0] << 8) | p[1]] = (int16_t)(i + 1);

    FILE* f = std::fopen(path, "rb");
    if (!f) return false;

    char line[256];
    while (std::fgets(line, sizeof line, f) && (int)wordstr.size() < take_n) {
        char w[64];
        long long cnt = 1;
        if (std::sscanf(line, "%63s %lld", w, &cnt) < 1) continue;
        int nb = (int)std::strlen(w);
        if (nb % 2) continue;
        int L = 0, el = 0;
        uint64_t code = 0;
        bool ok = true;
        for (int b = 0; b + 1 < nb; b += 2) {
            uint16_t key = (uint16_t)(((unsigned char)w[b] << 8) |
                                      (unsigned char)w[b + 1]);
            int v = lmap[key];
            if (v < 0) { ok = false; break; }
            ++L;
            if (v < 10) { code = code * 10 + (uint64_t)v; el += 1; }
            else        { code = code * 100 + (uint64_t)v; el += 2; }
        }
        if (!ok || nb % 2 || L < 3 || L > 8) continue;
        uint8_t k = 0;
        if (g_stopw.count(w)) k = 2;
        else if (verb_like(w)) k = 1;
        wcat.push_back(k);
        wordstr.push_back(w);
        wlog.push_back(std::log10((double)(cnt > 0 ? cnt : 1)));
        enclen.push_back(el);
    }
    std::fclose(f);

    size_t need = wordstr.size() * 2;
    size_t ts = 1;
    while (ts < need) ts <<= 1;
    tmask = ts - 1;
    tbl.assign(ts, Entry{0, -1});
    long long coll = 0, placed = 0;
    std::vector<uint64_t> codes(wordstr.size());
    {
        for (size_t i = 0; i < wordstr.size(); ++i) {
            const std::string& w = wordstr[i];
            uint64_t code = 0;
            const unsigned char* q = (const unsigned char*)w.c_str();
            for (size_t b = 0; b + 1 < w.size(); b += 2) {
                uint16_t key = (uint16_t)((q[b] << 8) | q[b + 1]);
                int v = lmap[key];
                if (v < 10) code = code * 10 + (uint64_t)v;
                else code = code * 100 + (uint64_t)v;
            }
            codes[i] = code;
        }
    }
    for (size_t i = 0; i < codes.size(); ++i) {
        uint64_t code = codes[i];
        size_t h = mixh(code) & tmask;
        for (;;) {
            Entry& e = tbl[h];
            if (e.wid < 0) { e.code = code; e.wid = (int32_t)i; ++placed; break; }
            if (e.code == code) { ++coll; break; }
            h = (h + 1) & tmask;
        }
    }
    for (int el : enclen)
        if (el >= 0 && el < 20) valid_len[el] = true;
    std::fprintf(stderr, "словарь: %zu слов, кодов в таблице: %lld, коллизий: %lld\n",
                 wordstr.size(), placed, coll);
    return true;
}

static std::atomic<uint64_t> g_total{0}, g_n2{0}, g_n3{0}, g_n4{0}, g_rep{0};
static std::atomic<bool> g_stop{false};

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

static void worker(uint64_t seed, std::vector<Cand>& cands,
                   std::vector<Cand>& top2, int& maxnw)
{
    static constexpr uint64_t p10[20] = {
        1ULL, 10ULL, 100ULL, 1000ULL, 10000ULL, 100000ULL, 1000000ULL,
        10000000ULL, 100000000ULL, 1000000000ULL, 10000000000ULL,
        100000000000ULL, 1000000000000ULL, 10000000000000ULL,
        100000000000000ULL, 1000000000000000ULL, 10000000000000000ULL,
        100000000000000000ULL, 1000000000000000000ULL,
        10000000000000000000ULL
    };
    RNG rng{seed};
    char buf[32];
    uint64_t pre[24];
    uint64_t local = 0;

    cands.reserve(8192);

    for (;;) {
        uint64_t num = LO + rng.next() % SPAN;
        ++local;
        if ((local & 0xFFFFULL) == 0) {
            g_total.fetch_add(0x10000, std::memory_order_relaxed);
            if (g_stop.load(std::memory_order_relaxed)) break;
        }
        auto r = std::to_chars(buf, buf + sizeof buf, num);
        int n = (int)(r.ptr - buf);
        pre[0] = 0;
        for (int k = 0; k < n; ++k)
            pre[k + 1] = pre[k] * 10 + (uint64_t)(buf[k] - '0');

        Sel hh[64];
        int nh = 0;
        for (int i = 0; i < n; ++i) {
            int lim = n - i;
            if (lim > 16) lim = 16;
            const uint64_t base = pre[i];
            for (int ln = 3; ln <= lim; ++ln) {
                if (!valid_len[ln]) continue;
                uint64_t code = pre[i + ln] - base * p10[ln];
                size_t h = mixh(code) & tmask;
                for (;;) {
                    Entry& e = tbl[h];
                    if (e.wid < 0) break;
                    if (e.code == code) {
                        if (nh < 64)
                            hh[nh++] = Sel{(uint8_t)i, (uint8_t)(i + ln), e.wid};
                        break;
                    }
                    h = (h + 1) & tmask;
                }
            }
        }
        if (nh < 2) continue;

        std::sort(hh, hh + nh, [](const Sel& a, const Sel& b) {
            return a.i != b.i ? a.i < b.i : a.j > b.j;
        });
        Sel sel[6];
        int ns = 0, last = -1;
        for (int k = 0; k < nh && ns < 6; ++k)
            if (hh[k].i > last) { sel[ns++] = hh[k]; last = hh[k].j; }
        if (ns < 2) continue;

        bool rep = false;
        for (int a = 0; a < ns && !rep; ++a)
            for (int b = a + 1; b < ns; ++b)
                if (sel[a].wid == sel[b].wid) { rep = true; break; }
        if (rep) {
            g_rep.fetch_add(1, std::memory_order_relaxed);
            continue;
        }

        if (ns == 2) g_n2.fetch_add(1, std::memory_order_relaxed);
        else if (ns == 3) g_n3.fetch_add(1, std::memory_order_relaxed);
        else g_n4.fetch_add(1, std::memory_order_relaxed);

        int nv = 0, nstop = 0;
        for (int k = 0; k < ns; ++k) {
            uint8_t ct = wcat[(size_t)sel[k].wid];
            if (ct == 1) ++nv;
            else if (ct == 2) ++nstop;
        }
        if (nv == 0 || nstop > 1) {
            g_gram.fetch_add(1, std::memory_order_relaxed);
            continue;
        }

        Cand c;
        c.num = num;
        c.nw = ns;
        c.cov = sel[ns - 1].j - sel[0].i;
        for (int k = 0; k < ns; ++k) c.sel[k] = sel[k];
        c.score = 0.0;
        for (int k = 0; k < ns; ++k) c.score += wlog[sel[k].wid];

        if (ns == 2) {
            if (top2.size() < 8) {
                top2.push_back(c);
                std::sort(top2.begin(), top2.end(),
                          [](const Cand& a, const Cand& b) {
                              return a.score > b.score;
                          });
            } else if (c.score > top2.back().score) {
                top2.back() = c;
                std::sort(top2.begin(), top2.end(),
                          [](const Cand& a, const Cand& b) {
                              return a.score > b.score;
                          });
            }
        } else {
            if (ns > maxnw) maxnw = ns;
            cands.push_back(c);
            if (cands.size() > 4096) {
                std::sort(cands.begin(), cands.end(),
                          [](const Cand& a, const Cand& b) {
                              return a.nw != b.nw ? a.nw > b.nw
                                                  : a.score > b.score;
                          });
                cands.resize(1024);
            }
        }
    }
}

static void markup(uint64_t num, const Sel* sel, int ns, std::string& out)
{
    char buf[32];
    auto r = std::to_chars(buf, buf + sizeof buf, num);
    int n = (int)(r.ptr - buf);
    int cur = 0;
    for (int k = 0; k < ns; ++k) {
        int i = sel[k].i, j = sel[k].j;
        for (int t = cur; t < i; ++t) out.push_back('.');
        out.append(buf + i, (size_t)(j - i));
        cur = j;
    }
    for (int t = cur; t < n; ++t) out.push_back('.');
}

int main(int argc, char** argv)
{
    const char* dict = argc > 1 ? argv[1] : "ru_50k.txt";
    const double RUNTIME = argc > 2 ? std::atof(argv[2]) : 270.0;
    if (!load_dict(dict, 9000)) {
        std::fprintf(stderr, "нет словаря\n");
        return 1;
    }
    {
        uint64_t pc = 1718103620ULL;
        size_t h = mixh(pc) & tmask;
        while (tbl[h].wid >= 0 && tbl[h].code != pc) h = (h + 1) & tmask;
        std::printf("проверка кодировки: 1718103620 -> %s\n",
                    tbl[h].wid >= 0 ? wordstr[(size_t)tbl[h].wid].c_str() : "?");
    }

    const int threads = [] {
        unsigned h = std::thread::hardware_concurrency();
        return h ? (int)h : 2;
    }();
    std::printf("Поиск: мультисловные чтения | потоков: %d | словарь: %s\n\n",
                threads, dict);

    const auto t0 = std::chrono::steady_clock::now();
    const uint64_t seed0 = (uint64_t)t0.time_since_epoch().count();

    std::vector<std::thread> ts;
    std::vector<std::vector<Cand>> vc((size_t)threads);
    std::vector<std::vector<Cand>> v2((size_t)threads);
    std::vector<int> vmax((size_t)threads, 0);
    ts.reserve((size_t)threads);

    for (int t = 0; t < threads; ++t)
        ts.emplace_back([&, t] {
            worker(seed0 ^ (0x9E3779B97F4A7C15ULL * (uint64_t)(t + 1)),
                   vc[(size_t)t], v2[(size_t)t], vmax[(size_t)t]);
        });

    double nxt = 15.0;
    for (;;) {
        std::this_thread::sleep_for(std::chrono::milliseconds(3000));
        const double el = std::chrono::duration<double>(
                              std::chrono::steady_clock::now() - t0)
                              .count();
        if (el >= RUNTIME) break;
        if (el >= nxt) {
            std::printf("... %llu чисел | 2 сл.: %llu | 3 сл.: %llu | 4+ сл.: %llu\n",
                        (unsigned long long)g_total.load(),
                        (unsigned long long)g_n2.load(),
                        (unsigned long long)g_n3.load(),
                        (unsigned long long)g_n4.load());
            std::fflush(stdout);
            nxt += 15.0;
        }
    }
    g_stop.store(true, std::memory_order_relaxed);
    for (auto& th : ts) th.join();

    const auto t1 = std::chrono::steady_clock::now();
    const double el = std::chrono::duration<double>(t1 - t0).count();
    const uint64_t total = g_total.load();
    const uint64_t n2 = g_n2.load(), n3 = g_n3.load(), n4 = g_n4.load();
    int maxnw = 0;
    std::vector<Cand> all;
    for (int t = 0; t < threads; ++t) {
        if (vmax[(size_t)t] > maxnw) maxnw = vmax[(size_t)t];
        all.insert(all.end(), vc[(size_t)t].begin(), vc[(size_t)t].end());
        all.insert(all.end(), v2[(size_t)t].begin(), v2[(size_t)t].end());
    }

    std::printf("\nитог: %llu чисел за %.0f сек (%.0f чисел/сек)\n",
                (unsigned long long)total, el, (double)total / el);
    std::printf("2 слова: %llu | 3 слова: %llu | 4+ слова: %llu | максимум слов: %d | повторов: %llu | без глагола/клея: %llu\n",
                (unsigned long long)n2, (unsigned long long)n3,
                (unsigned long long)n4, maxnw, (unsigned long long)g_rep.load(),
                (unsigned long long)g_gram.load());

    std::sort(all.begin(), all.end(), [](const Cand& a, const Cand& b) {
        if (a.nw != b.nw) return a.nw > b.nw;
        if (a.score != b.score) return a.score > b.score;
        return a.cov > b.cov;
    });

    FILE* hf = std::fopen("hits.txt", "w");
    if (hf) {
        std::set<std::string> fseen;
        for (const Cand& c : all) {
            std::string phrase;
            for (int k = 0; k < c.nw; ++k) {
                if (k) phrase.push_back(' ');
                phrase += wordstr[(size_t)c.sel[k].wid];
            }
            if (fseen.count(phrase)) continue;
            fseen.insert(phrase);
            std::fprintf(hf, "%llu\t%s\t%d\t%d\t%.2f\n",
                         (unsigned long long)c.num, phrase.c_str(),
                         c.nw, c.cov, c.score);
        }
        std::fclose(hf);
    }
    std::printf("\n=== ТОП: САМЫЕ МНОГОСЛОВНЫЕ ЧТЕНИЯ ===\n");
    std::set<std::string> seen;
    int shown = 0;
    for (const Cand& c : all) {
        std::string phrase, mark;
        for (int k = 0; k < c.nw; ++k) {
            if (k) phrase.push_back(' ');
            phrase += wordstr[(size_t)c.sel[k].wid];
        }
        markup(c.num, c.sel, c.nw, mark);
        if (seen.count(phrase)) continue;
        seen.insert(phrase);
        char buf2[512];
        std::snprintf(buf2, sizeof buf2,
                      "%llu  =>  %-34s [%s]  сл.: %d  score: %.2f\n",
                      (unsigned long long)c.num, phrase.c_str(), mark.c_str(),
                      c.nw, c.score);
        std::printf("%s", buf2);
        if (++shown >= 20) break;
    }

    std::sort(all.begin(), all.end(), [](const Cand& a, const Cand& b) {
        if (a.score != b.score) return a.score > b.score;
        return a.cov > b.cov;
    });
    std::printf("\n=== ТОП ПО N-GRAM СУДЬЕ (самые частотные слова) ===\n");
    seen.clear();
    shown = 0;
    for (const Cand& c : all) {
        std::string phrase, mark;
        for (int k = 0; k < c.nw; ++k) {
            if (k) phrase.push_back(' ');
            phrase += wordstr[(size_t)c.sel[k].wid];
        }
        markup(c.num, c.sel, c.nw, mark);
        if (seen.count(phrase)) continue;
        seen.insert(phrase);
        char buf2[512];
        std::snprintf(buf2, sizeof buf2,
                      "%llu  =>  %-34s [%s]  сл.: %d  score: %.2f\n",
                      (unsigned long long)c.num, phrase.c_str(), mark.c_str(),
                      c.nw, c.score);
        std::printf("%s", buf2);
        if (++shown >= 20) break;
    }
    std::printf("\n(все %zu кандидатов сохранены в hits.txt)\n", all.size());
    return 0;
}
