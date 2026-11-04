import json
import random
import re
import sys
import time
import urllib.request

URL = "http://127.0.0.1:8817/v1/chat/completions"


def load_hits(path):
    rows, seen = [], set()
    for line in open(path, encoding="utf-8"):
        parts = line.rstrip("\n").split("\t")
        if len(parts) != 5:
            continue
        num, phrase, nw, cov, score = parts
        if phrase in seen:
            continue
        seen.add(phrase)
        rows.append((num, phrase, int(nw), int(cov), float(score)))
    return rows


def ask(phrase):
    body = json.dumps({
        "model": "qwen1.5-4b-chat",
        "messages": [{
            "role": "user",
            "content": ("Оцени осмысленность русской фразы от 0 (полная бессмыслица) "
                        "до 10 (естественная фраза). Ответь только одним числом. "
                        "Фраза: «" + phrase + "»")
        }],
        "max_tokens": 4,
        "temperature": 0.0
    }).encode()
    req = urllib.request.Request(URL, data=body,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=90) as r:
            d = json.loads(r.read())
        txt = d["choices"][0]["message"]["content"]
        m = re.search(r"\b(10|[0-9])\b", txt)
        return int(m.group(1)) if m else -1
    except Exception:
        return -1


rows = load_hits("hits.txt")
n_top = int(sys.argv[1]) if len(sys.argv) > 1 else 110
n_rand = int(sys.argv[2]) if len(sys.argv) > 2 else 40

sel = rows[:n_top]
rnd = random.Random(42)
pool = rows[n_top:]
sel += (rnd.sample(pool, n_rand) if len(pool) > n_rand else pool)

res = []
t0 = time.time()
for i, (num, phrase, nw, cov, score) in enumerate(sel):
    ai = ask(phrase)
    res.append((ai, int(num), phrase, nw, cov, score))
    if (i + 1) % 20 == 0:
        print(f"... {i + 1}/{len(sel)} оценок, {time.time() - t0:.0f} сек", flush=True)

res.sort(key=lambda r: (-r[0], -r[5]))
hist = {}
for ai, *_ in res:
    hist[ai] = hist.get(ai, 0) + 1

with open("judged.txt", "w", encoding="utf-8") as f:
    for ai, num, phrase, nw, cov, score in res:
        f.write(f"{ai}\t{num}\t{phrase}\t{nw}\t{cov}\t{score:.2f}\n")

print("\nраспределение вердиктов ИИ (0-10; -1 = сбой запроса):")
for k in sorted(hist):
    print(f"  {k}: {hist[k]}")

print("\n=== ВЕРДИКТ ЛОКАЛЬНОЙ ИИ: ТОП ОСМЫСЛЕННЫХ ЧТЕНИЙ ===")
shown = 0
for ai, num, phrase, nw, cov, score in res:
    if ai <= 0:
        break
    print(f"ИИ:{ai:2d} | {num} | {phrase} | сл.: {nw} | покрытие {cov}/{len(str(num))} | ngram {score:.1f}")
    shown += 1
    if shown >= 20:
        break
