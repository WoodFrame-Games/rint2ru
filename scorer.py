import json
import math
import random
import re
import time
import urllib.request
from collections import Counter, defaultdict

ALPH = set("абвгдеёжзийклмнопрстуфхцчшщъыьэюя-")
URL = "http://127.0.0.1:8817/v1/chat/completions"


def load_vocab(path="ru_50k.txt", take=9000):
    vocab = []
    for line in open(path, encoding="utf-8", errors="ignore"):
        parts = line.split()
        if not parts:
            continue
        w = parts[0].lower()
        if w.isalpha() and set(w) <= ALPH and 3 <= len(w) <= 8:
            vocab.append(w)
        if len(vocab) >= take:
            break
    return vocab


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


vocab = load_vocab()
uni = Counter()
bi = defaultdict(Counter)
seen_sent = set()
for line in open("corpus.txt", encoding="utf-8"):
    s = line.strip()
    if s in seen_sent:
        continue
    seen_sent.add(s)
    ws = s.split()
    if len(ws) < 2:
        continue
    for w in ws:
        uni[w] += 1
    for a, b in zip(ws, ws[1:]):
        bi[a][b] += 1

Nc = sum(uni.values())
Veff = max(len(uni), 1)
c1 = {a: sum(d.values()) for a, d in bi.items()}


def logP(w):
    return math.log10((uni[w] + 0.5) / (Nc + 0.5 * Veff))


def logcond(a, b):
    puni = (uni[b] + 0.5) / (Nc + 0.5 * Veff)
    return math.log10((bi[a][b] + 0.2 * puni) / (c1.get(a, 0) + 0.2))


print(f"корпус: {len(seen_sent)} предложений, {Nc} слов, "
      f"{sum(len(d) for d in bi.values())} уникальных пар")

cands, seen = [], set()
for line in open("hits.txt", encoding="utf-8"):
    parts = line.rstrip("\n").split("\t")
    if len(parts) != 5:
        continue
    num, phrase, nw, cov, sc = parts
    if phrase in seen:
        continue
    seen.add(phrase)
    cands.append((num, phrase, int(nw), int(cov), float(sc)))

ranked = []
for num, phrase, nw, cov, sc in cands:
    ws = phrase.split()
    u = sum(logP(w) for w in ws)
    bg = sum(logcond(a, b) for a, b in zip(ws, ws[1:]))
    hits = sum(1 for a, b in zip(ws, ws[1:]) if bi[a][b] > 0)
    rho = hits / (nw - 1) if nw > 1 else 0.0
    ranked.append((u + 0.8 * bg, rho, num, phrase, nw, cov, sc))

for nwgrp in (2, 3, 4):
    grp = [r for r in ranked if r[4] == nwgrp]
    if not grp:
        continue
    grp.sort(reverse=True)
    print(f"\n--- БИГРАМ-ТОП ({nwgrp} слова) ---")
    for final, rho, num, phrase, _, cov, _ in grp[:8]:
        print(f"{final:7.2f} ({rho * 100:3.0f}% пар из корпуса)  {num} => {phrase}")

sel = sorted(ranked, reverse=True)[:35]
res = []
t0 = time.time()
for i, (final, rho, num, phrase, nw, cov, sc) in enumerate(sel):
    ai = ask(phrase)
    res.append((ai, num, phrase, nw, cov, float(sc), final, rho))
    if (i + 1) % 10 == 0:
        print(f"... {i + 1}/35 оценок, {time.time() - t0:.0f} сек", flush=True)

res.sort(key=lambda r: (-r[0], -r[6]))
with open("judged.txt", "w", encoding="utf-8") as f:
    for ai, num, phrase, nw, cov, sc, final, rho in res:
        f.write(f"{ai}\t{num}\t{phrase}\t{nw}\t{cov}\t{sc:.2f}\t{final:.2f}\n")

print("\n=== ИИ-СУДЬЯ ПОВЕРХ БИГРАМ-РАНЖИРА ===")
shown = 0
for ai, num, phrase, nw, cov, sc, final, rho in res:
    if ai <= 0:
        continue
    print(f"ИИ:{ai} | {num} | {phrase} | {nw} сл. | {cov}/19 | биграм {rho * 100:.0f}%")
    shown += 1
    if shown >= 15:
        break
