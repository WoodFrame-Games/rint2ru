#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <set>
#include <string>
#include <vector>

static constexpr const char* ALPH =
    "\xD0\xB0\xD0\xB1\xD0\xB2\xD0\xB3\xD0\xB4\xD0\xB5\xD1\x91\xD0\xB6"
    "\xD0\xB7\xD0\xB8\xD0\xB9\xD0\xBA\xD0\xBB\xD0\xBC\xD0\xBD\xD0\xBE"
    "\xD0\xBF\xD1\x80\xD1\x81\xD1\x82\xD1\x83\xD1\x84\xD1\x85\xD1\x86"
    "\xD1\x87\xD1\x88\xD1\x89\xD1\x8A\xD1\x8B\xD1\x8C\xD1\x8D\xD1\x8E"
    "\xD1\x8F";

struct E {
    uint64_t code;
    uint8_t flags;
    uint8_t letters;
    uint16_t slen;
    uint32_t soff;
};

static inline uint64_t mixh(uint64_t x)
{
    x *= 0x9E3779B97F4A7C15ULL;
    x ^= x >> 29;
    x *= 0xBF58476D1CE4E5B9ULL;
    x ^= x >> 32;
    return x;
}

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

int main()
{
    int16_t lmap[65536];
    std::memset(lmap, -1, sizeof lmap);
    const unsigned char* p = (const unsigned char*)ALPH;
    for (int i = 0; *p; ++i, p += 2)
        lmap[(p[0] << 8) | p[1]] = (int16_t)(i + 1);

    std::set<std::string> stopw;
    for (const char* s : STOPW) stopw.insert(s);
    std::set<std::string> badw;
    for (const char* s : BADW) badw.insert(s);

    FILE* f = std::fopen("ru_50k.txt", "rb");
    if (!f) { std::fprintf(stderr, "нет ru_50k.txt\n"); return 1; }

    std::vector<std::string> words;
    std::vector<uint64_t> codes;
    std::vector<uint8_t> flags;
    char line[256];
    int take = 9000;
    while (std::fgets(line, sizeof line, f) && (int)words.size() < take) {
        char w[64];
        long long cnt = 1;
        if (std::sscanf(line, "%63s %lld", w, &cnt) < 1) continue;
        int nb = (int)std::strlen(w);
        if (nb % 2) continue;
        int L = 0;
        uint64_t code = 0;
        bool ok = true;
        for (int b = 0; b + 1 < nb; b += 2) {
            uint16_t key = (uint16_t)(((unsigned char)w[b] << 8) |
                                      (unsigned char)w[b + 1]);
            int v = lmap[key];
            if (v < 0) { ok = false; break; }
            ++L;
            if (v < 10) code = code * 10 + (uint64_t)v;
            else code = code * 100 + (uint64_t)v;
        }
        if (!ok || nb % 2 || L < 5 || L > 8) continue;
        uint8_t k = 0;
        if (badw.count(w)) k = 3;
        else if (stopw.count(w)) k = 2;
        else if (verb_like(w)) k = 1;
        words.push_back(w);
        codes.push_back(code);
        flags.push_back(k);
    }
    std::fclose(f);

    size_t nw = words.size();
    size_t need = nw * 2;
    size_t ts = 1;
    while (ts < need) ts <<= 1;

    std::vector<E> tbl(ts, E{0, 0xFF, 0, 0, 0});
    std::string blob;
    long long placed = 0, coll = 0;
    for (size_t i = 0; i < nw; ++i) {
        size_t h = mixh(codes[i]) & (ts - 1);
        for (;;) {
            E& e = tbl[h];
            if (e.flags == 0xFF) {
                e.code = codes[i];
                e.flags = flags[i];
                e.letters = (uint8_t)(words[i].size() / 2);
                e.slen = (uint16_t)words[i].size();
                e.soff = (uint32_t)blob.size();
                blob += words[i];
                ++placed;
                break;
            }
            if (e.code == codes[i]) { ++coll; break; }
            h = (h + 1) & (ts - 1);
        }
    }

    FILE* out = std::fopen("dict.bin", "wb");
    if (!out) return 1;
    uint32_t hdr[8] = {0x52554431u, (uint32_t)ts, (uint32_t)nw,
                       (uint32_t)blob.size(), 0, 0, 0, 0};
    std::fwrite(hdr, sizeof hdr, 1, out);
    std::fwrite(tbl.data(), sizeof(E), tbl.size(), out);
    std::fwrite(blob.data(), 1, blob.size(), out);
    std::fclose(out);

    std::printf("dict.bin: слов=%zu, таблица=%zu, кодов=%lld, коллизий=%lld, blob=%zu байт\n",
                nw, ts, placed, coll, blob.size());
    return 0;
}