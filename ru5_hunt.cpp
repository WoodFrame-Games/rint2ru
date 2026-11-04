#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <mutex>
#include <set>
#include <string>
#include <thread>
#include <unordered_map>
#include <vector>

static constexpr const char* ALPH =
    "\xD0\xB0\xD0\xB1\xD0\xB2\xD0\xB3\xD0\xB4\xD0\xB5\xD1\x91\xD0\xB6"
    "\xD0\xB7\xD0\xB8\xD0\xB9\xD0\xBA\xD0\xBB\xD0\xBC\xD0\xBD\xD0\xBE"
    "\xD0\xBF\xD1\x80\xD1\x81\xD1\x82\xD1\x83\xD1\x84\xD1\x85\xD1\x86"
    "\xD1\x87\xD1\x88\xD1\x89\xD1\x8A\xD1\x8B\xD1\x8C\xD1\x8D\xD1\x8E"
    "\xD1\x8F";

static int LEN = 60;
static constexpr int TAIL = 16;

struct Entry { uint64_t code; int32_t wid; };
struct Sel   { uint8_t i, j; int32_t wid; };
struct Cand  {
    uint8_t nw, cov;
    uint16_t cross;
    double score;
    char s[16 + 64];
    Sel sel[10];
};

static std::vector<Entry> tbl;
static size_t tmask = 0;
static std::vector<std::string> wordstr;
static std::vector<double> wlog;
static std::vector<int> enclen;
static bool valid_len[24] = {};
static int16_t lmap[65536];
static std::vector<uint8_t> wcat;
static std::set<std::string> g_stopw, g_badw;

static const char* VSUFS[] = {
    "ть", "тся", "ться", "ла", "ло", "ли", "ет", "ит", "ут", "ют",
    "ат", "ят", "ем", "им", "ся", "сь", "ешь", "ишь", "ешься", "ишься",
    "лась", "лось", "лись", "лся", "ёт", "ал", "ел", "ил", "ул", "ыл",
    "ол"
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
    "я", "ты", "он", "она", "оно", "мы", "вы", "они", "мне", "меня",
    "тебя", "тебе", "вас", "вам", "нас", "нам", "их", "его", "ее", "ей",
    "им", "ими", "себя", "себе", "сам", "сама", "сами", "свой", "своя",
    "свои", "свое", "моя", "мой", "мои", "мое", "твой", "твоя", "твои",
    "ваши", "наши", "наш", "ваш", "там", "тут", "вот", "где", "куда",
    "как", "что", "чтобы", "зачем", "почему", "когда", "уже", "еще",
    "ещё", "тоже", "так", "да", "нет", "не", "ни", "но", "и", "а",
    "или", "ли", "же", "бы", "уж", "все", "всё", "это", "этот", "эта",
    "эти", "тот", "та", "те", "тех", "того", "тому", "том", "тем",
    "чем", "чего", "чему", "кому", "кого", "эту", "без", "два", "две",
    "над", "под", "при", "про", "для", "до", "от", "по", "за", "из",
    "об", "во", "на", "оба", "обе", "лет", "раз", "то", "ну", "ведь",
    "лишь", "только", "даже", "почти", "чуть", "очень", "совсем",
    "вдруг", "опять", "снова", "между", "через", "после", "перед",
    "около", "возле", "вместо", "кроме", "среди"
};

static const char* BADW[] = {
    "тед", "джо", "бет", "ева", "деб", "даг", "бла", "дке", "пёп",
    "базз", "хуя", "ааа", "эээ", "угу", "ага", "нее", "неа", "даа",
    "мда", "ооо", "эх"
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
    for (const char* s : BADW) g_badw.insert(s);
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
        if (g_badw.count(w)) k = 3;
        else if (g_stopw.count(w)) k = 2;
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
    std::fprintf(stderr, "словарь: %zu слов, кодов: %lld, коллизий: %lld\n",
                 wordstr.size(), placed, coll);
    return true;
}

static std::atomic<uint64_t> g_total{0}, g_n2{0}, g_n3{0}, g_n4{0},
                              g_n5{0}, g_n6{0}, g_rep{0}, g_gram{0},
                              g_crossw{0}, g_crossp{0};
static std::atomic<bool> g_stop{false};
static std::mutex g_mu;
static Cand g_best;
static bool g_hasbest = false;
static int g_maxnw = 0;
static std::mutex g_lwmu;
static std::unordered_map<std::string, uint64_t> g_longwords;
static std::atomic<uint64_t> g_longhits{0};
static FILE* g_lwf = nullptr;
static int LW_MIN = 6;

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

static void worker(uint64_t seed)
{
    RNG rng{seed};
    char buf[16 + 64];
    std::memset(buf, '0', (size_t)TAIL);
    const int TOT = TAIL + LEN;
    uint64_t local = 0;

    for (;;) {
        for (int k = 0; k < LEN; ++k) {
            uint64_t r = rng.next();
            int d = (k == 0) ? 1 + (int)(r % 9) : (int)(r % 10);
            buf[TAIL + k] = (char)('0' + d);
        }
        ++local;
        if ((local & 0x3FFFULL) == 0) {
            g_total.fetch_add(0x4000, std::memory_order_relaxed);
            if (g_stop.load(std::memory_order_relaxed)) break;
        }

        Sel hh[256];
        int nh = 0;
        for (int i = 0; i < TOT; ++i) {
            if (buf[i] == '0') continue;
            uint64_t code = 0;
            int lim = TOT - i;
            if (lim > 16) lim = 16;
            for (int ln = 1; ln <= lim; ++ln) {
                code = code * 10 + (uint64_t)(buf[i + ln - 1] - '0');
                if (ln < 3 || !valid_len[ln]) continue;
                if (i + ln <= TAIL) continue;
                size_t h = mixh(code) & tmask;
                for (;;) {
                    Entry& e = tbl[h];
                    if (e.wid < 0) break;
                    if (e.code == code) {
                        if (nh < 256)
                            hh[nh++] = Sel{(uint8_t)i, (uint8_t)(i + ln), e.wid};
                        break;
                    }
                    h = (h + 1) & tmask;
                }
            }
        }
        if (nh < 2) {
            std::memmove(buf, buf + LEN, (size_t)TAIL);
            continue;
        }

        std::sort(hh, hh + nh, [](const Sel& a, const Sel& b) {
            return a.i != b.i ? a.i < b.i : a.j > b.j;
        });
        Sel sel[10];
        int ns = 0, last = -1;
        for (int k = 0; k < nh && ns < 10; ++k)
            if (hh[k].i >= last) { sel[ns++] = hh[k]; last = hh[k].j; }

        if (ns < 2) {
            std::memmove(buf, buf + LEN, (size_t)TAIL);
            continue;
        }

        bool rep = false;
        for (int a = 0; a < ns && !rep; ++a)
            for (int b = a + 1; b < ns; ++b)
                if (sel[a].wid == sel[b].wid) { rep = true; break; }
        if (rep) {
            g_rep.fetch_add(1, std::memory_order_relaxed);
            std::memmove(buf, buf + LEN, (size_t)TAIL);
            continue;
        }

        if (ns == 2) g_n2.fetch_add(1, std::memory_order_relaxed);
        else if (ns == 3) g_n3.fetch_add(1, std::memory_order_relaxed);
        else if (ns == 4) g_n4.fetch_add(1, std::memory_order_relaxed);
        else if (ns == 5) g_n5.fetch_add(1, std::memory_order_relaxed);
        else g_n6.fetch_add(1, std::memory_order_relaxed);

        int nv = 0, nstop = 0;
        bool bad = false;
        uint16_t cm = 0;
        for (int k = 0; k < ns; ++k) {
            uint8_t ct = wcat[(size_t)sel[k].wid];
            if (ct == 1) ++nv;
            else if (ct == 2) ++nstop;
            else if (ct == 3) bad = true;
            if (sel[k].i < TAIL && sel[k].j > TAIL)
                cm |= (uint16_t)(1u << k);
        }
        if (bad || nv == 0 || nstop > 2) {
            g_gram.fetch_add(1, std::memory_order_relaxed);
            std::memmove(buf, buf + LEN, (size_t)TAIL);
            continue;
        }

        g_crossw.fetch_add((uint64_t)__builtin_popcount((unsigned)cm),
                           std::memory_order_relaxed);
        if (cm)
            g_crossp.fetch_add(1, std::memory_order_relaxed);

        for (int k = 0; k < ns; ++k) {
            if (wcat[(size_t)sel[k].wid] == 3) continue;
            const std::string& w = wordstr[(size_t)sel[k].wid];
            int letters = (int)w.size() / 2;
            if (letters < LW_MIN) continue;
            g_longhits.fetch_add(1, std::memory_order_relaxed);
            bool iscross = sel[k].i < TAIL && sel[k].j > TAIL;
            std::lock_guard<std::mutex> lk(g_lwmu);
            uint64_t& cnt = g_longwords[w];
            if (cnt == 0 && g_lwf) {
                char tsb[32];
                std::time_t tt = std::time(nullptr);
                std::strftime(tsb, sizeof tsb, "%Y-%m-%d %H:%M:%S",
                              std::localtime(&tt));
                std::fprintf(g_lwf, "%s | %d букв | %s | @%d-%d%s | фраза: ",
                             tsb, letters, w.c_str(), (int)sel[k].i,
                             (int)sel[k].j, iscross ? " СТЫК" : "");
                for (int q = 0; q < ns; ++q) {
                    if (q) std::fputc(' ', g_lwf);
                    std::fputs(wordstr[(size_t)sel[q].wid].c_str(), g_lwf);
                }
                std::fputc('\n', g_lwf);
                std::fflush(g_lwf);
            }
            cnt++;
        }

        if (ns >= 4) {
            Cand c;
            c.nw = (uint8_t)ns;
            c.cov = (uint8_t)(sel[ns - 1].j - sel[0].i);
            c.cross = cm;
            c.score = 0.0;
            for (int k = 0; k < ns; ++k) c.score += wlog[(size_t)sel[k].wid];
            std::memcpy(c.s, buf, (size_t)TOT);
            for (int k = 0; k < ns; ++k) c.sel[k] = sel[k];
            std::lock_guard<std::mutex> lk(g_mu);
            if (ns > g_maxnw) g_maxnw = ns;
            if (!g_hasbest || ns > g_best.nw ||
                (ns == g_best.nw && c.score > g_best.score)) {
                g_best = c;
                g_hasbest = true;
            }
        }
        std::memmove(buf, buf + LEN, (size_t)TAIL);
    }
    g_total.fetch_add(local & 0x3FFFULL, std::memory_order_relaxed);
}

static void fmt_cand(const Cand& c, std::string& digits, std::string& phrase,
                     std::string& mark)
{
    for (int k = 0; k < c.nw; ++k) {
        if (k) phrase.push_back(' ');
        phrase += wordstr[(size_t)c.sel[k].wid];
        if (c.cross & (1u << k)) phrase.push_back('*');
    }
    for (int k = 0; k < TAIL + LEN; ++k) {
        if (k == TAIL) { digits.push_back('|'); mark.push_back('|'); }
        digits.push_back(c.s[k]);
        mark.push_back('.');
    }
    for (int k = 0; k < c.nw; ++k)
        for (int t = c.sel[k].i; t < c.sel[k].j; ++t)
            mark[t + (t >= TAIL ? 1 : 0)] = c.s[t];
}

int main(int argc, char** argv)
{
    const char* dict = argc > 1 ? argv[1] : "ru_50k.txt";
    const double RUNTIME = argc > 2 ? std::atof(argv[2]) : 86400.0;
    if (argc > 3) LEN = std::atoi(argv[3]);
    if (LEN < 20) LEN = 20;
    if (LEN > 64) LEN = 64;
    if (argc > 4) LW_MIN = std::atoi(argv[4]);
    if (LW_MIN < 4) LW_MIN = 4;
    int take_n = argc > 5 ? std::atoi(argv[5]) : 9000;
    if (take_n < 100) take_n = 100;

    if (!load_dict(dict, take_n)) {
        std::fprintf(stderr, "нет словаря\n");
        return 1;
    }

    FILE* jf = std::fopen("hunt_journal.txt", "a");
    FILE* sf = std::fopen("hunt_stats.txt", "a");
    g_lwf = std::fopen("hunt_longwords.txt", "a");
    if (!jf || !sf) {
        std::fprintf(stderr, "не могу открыть журналы\n");
        return 1;
    }

    const int threads = [] {
        unsigned h = std::thread::hardware_concurrency();
        return h ? (int)h : 2;
    }();
    std::fprintf(sf, "=== СТАРТ %s | поток 60+16 цифр | словарь %zu слов | потоков %d | цель %.0f сек ===\n",
                 __TIMESTAMP__, wordstr.size(), threads, RUNTIME);
    std::fflush(sf);

    const auto t0 = std::chrono::steady_clock::now();
    const uint64_t seed0 = (uint64_t)t0.time_since_epoch().count();

    std::vector<std::thread> ts;
    ts.reserve((size_t)threads);
    for (int t = 0; t < threads; ++t)
        ts.emplace_back([&, t] {
            worker(seed0 ^ (0x9E3779B97F4A7C15ULL * (uint64_t)(t + 1)));
        });

    double next_j = 10.0, next_s = 60.0;
    std::set<std::string> seen;
    for (;;) {
        std::this_thread::sleep_for(std::chrono::milliseconds(500));
        const double el = std::chrono::duration<double>(
                              std::chrono::steady_clock::now() - t0)
                              .count();

        if (el >= next_j) {
            next_j += 10.0;
            Cand c;
            bool has;
            {
                std::lock_guard<std::mutex> lk(g_mu);
                has = g_hasbest;
                if (has) { c = g_best; g_hasbest = false; }
            }
            char tsbuf[32];
            std::time_t tt = std::time(nullptr);
            std::strftime(tsbuf, sizeof tsbuf, "%Y-%m-%d %H:%M:%S",
                          std::localtime(&tt));
            if (has) {
                std::string digits, phrase, mark;
                fmt_cand(c, digits, phrase, mark);
                if (seen.count(phrase)) {
                    std::fprintf(jf, "%s | (повтор) %d сл. | %s\n", tsbuf, c.nw,
                                 phrase.c_str());
                } else {
                    if (seen.size() < 500000) seen.insert(phrase);
                    std::fprintf(jf, "%s | %d сл. | %s\n   %s\n",
                                 tsbuf, c.nw, phrase.c_str(), digits.c_str());
                }
            } else {
                std::fprintf(jf, "%s | (нет находок 4+ слов за интервал)\n", tsbuf);
            }
            std::fflush(jf);
        }

        if (el >= next_s) {
            next_s += 60.0;
            std::fprintf(sf,
                         "%.0f сек | чисел %llu | %.1f млрд цифр | %.0f млн цифр/сек | 2:%llu 3:%llu 4:%llu 5:%llu 6+:%llu | стык:%llu | макс слов %d\n",
                         el, (unsigned long long)g_total.load(),
                         (double)g_total.load() * LEN / 1e9,
                         (double)g_total.load() * LEN / el / 1e6,
                         (unsigned long long)g_n2.load(),
                         (unsigned long long)g_n3.load(),
                         (unsigned long long)g_n4.load(),
                         (unsigned long long)g_n5.load(),
                         (unsigned long long)g_n6.load(),
                         (unsigned long long)g_crossw.load(), g_maxnw);
            std::fflush(sf);
        }

        if (el >= RUNTIME) break;
    }

    g_stop.store(true, std::memory_order_relaxed);
    for (auto& th : ts) th.join();

    const auto t1 = std::chrono::steady_clock::now();
    const double el = std::chrono::duration<double>(t1 - t0).count();
    {
        std::vector<std::pair<size_t, const std::string*>> v;
        int maxlen = 0;
        {
            std::lock_guard<std::mutex> lk(g_lwmu);
            for (auto& kv : g_longwords) {
                v.emplace_back((size_t)kv.second, &kv.first);
                int L = (int)kv.first.size() / 2;
                if (L > maxlen) maxlen = L;
            }
        }
        std::sort(v.begin(), v.end(),
                  [](const auto& a, const auto& b) { return a.first > b.first; });
        std::fprintf(sf, "=== ДЛИННЫЕ СЛОВА (>= %d букв): %zu уникальных, %llu попаданий, максимум %d букв; топ-30 ===\n",
                     LW_MIN, g_longwords.size(),
                     (unsigned long long)g_longhits.load(), maxlen);
        for (size_t i = 0; i < v.size() && i < 30; ++i)
            std::fprintf(sf, "%6zu | %s (%d букв)\n", v[i].first,
                         v[i].second->c_str(), (int)v[i].second->size() / 2);
        if (g_lwf) std::fflush(g_lwf);
    }
    std::fprintf(sf, "=== ФИНАЛ %.0f сек | чисел %llu | %.1f млрд цифр | 2:%llu 3:%llu 4:%llu 5:%llu 6+:%llu | стык:%llu | макс слов %d ===\n",
                 el, (unsigned long long)g_total.load(),
                 (double)g_total.load() * LEN / 1e9,
                 (unsigned long long)g_n2.load(),
                 (unsigned long long)g_n3.load(),
                 (unsigned long long)g_n4.load(),
                 (unsigned long long)g_n5.load(),
                 (unsigned long long)g_n6.load(),
                 (unsigned long long)g_crossw.load(), g_maxnw);
    std::fflush(sf);
    if (g_lwf) std::fclose(g_lwf);
    std::fclose(sf);
    std::fclose(jf);
    return 0;
}