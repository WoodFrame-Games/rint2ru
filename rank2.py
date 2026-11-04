import json
import math
import re
import sys
import time
import urllib.request
from collections import Counter, defaultdict

ALPH = set("абвгдеёжзийклмнопрстуфхцчшщъыьэюя-")
URL = "http://127.0.0.1:8817/v1/chat/completions"


def clean(w):
    return w.lower().strip("«»\"'()[].,;:!?—–-0123456789")


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


uni = Counter()
bi = defaultdict(Counter)
nlines = 0
for line in open("corpus2.txt", encoding="utf-8"):
    nlines += 1
    ws = [clean(w) for w in line.split()]
    ws = [w for w in ws if w and all(c in ALPH for c in w) and 3 <= len(w) <= 12]
    for w in ws:
        uni[w] += 1
    for a, b in zip(ws, ws[1:]):
        bi[a][b] += 1

Nc = sum(uni.values())
V = max(len(uni), 1)
c1 = {a: sum(d.values()) for a, d in bi.items()}
npairs = sum(len(d) for d in bi.values())
print(f"корпус: {nlines} текстов, {Nc} слов, {V} уникальных, {npairs} уникальных пар")


def logP(w):
    return math.log10((uni[w] + 0.5) / (Nc + 0.5 * V))


def logcond(a, b):
    puni = (uni[b] + 0.5) / (Nc + 0.5 * V)
    return math.log10((bi[a][b] + 0.2 * puni) / (c1.get(a, 0) + 0.2))


rows, seen = [], set()
for line in open("hits.txt", encoding="utf-8"):
    p = line.rstrip("\n").split("\t")
    if len(p) != 5:
        continue
    num, phrase, nw, cov, sc = p
    if phrase in seen:
        continue
    seen.add(phrase)
    ws = phrase.split()
    u = sum(logP(w) for w in ws)
    bg = sum(logcond(a, b) for a, b in zip(ws, ws[1:]))
    hit = sum(1 for a, b in zip(ws, ws[1:]) if bi[a][b] > 0)
    rho = hit / (len(ws) - 1) if len(ws) > 1 else 0.0
    rows.append((u + bg, rho, num, phrase, int(nw), int(cov), float(sc)))

cov_all = sum(r[4] for r in rows) / max(len(rows), 1)
print(f"кандидатов: {len(rows)}, среднее покрытие пар биграммами: {cov_all * 100:.0f}%")

for nw in (2, 3, 4, 5, 6):
    grp = [r for r in rows if r[4] == nw]
    if not grp:
        continue
    grp.sort(reverse=True)
    print(f"\n--- РАНЖИР ПО РЕАЛЬНОМУ КОРПУСУ: {nw} слов ---")
    for final, rho, num, phrase, _, cov, _ in grp[:5]:
        print(f"{final:7.2f} ({rho * 100:3.0f}% пар)  {num} => {phrase}")

sel = sorted(rows, reverse=True)[:30]
res = []
t0 = time.time()
for i, (final, rho, num, phrase, nw, cov, sc) in enumerate(sel):
    ai = ask(phrase)
    res.append((ai, num, phrase, nw, cov, final, rho))
    if (i + 1) % 10 == 0:
        print(f"... {i + 1}/30 оценок, {time.time() - t0:.0f} сек", flush=True)

res.sort(key=lambda r: (-r[0], -r[5]))
print("\n=== ИТОГ: РЕАЛЬНЫЙ КОРПУС + ИИ-СУДЬЯ ===")
for ai, num, phrase, nw, cov, final, rho in res[:15]:
    print(f"ИИ:{ai} | {num} | {phrase} | {nw} сл. | {cov}/60 | биграм {rho * 100:.0f}% | {final:.1f}")