import random
import sys
import time

DICT = sys.argv[1] if len(sys.argv) > 1 else "ru_50k.txt"
RUNTIME = float(sys.argv[2]) if len(sys.argv) > 2 else 270.0

ALPH = "абвгдеёжзийклмнопрстуфхцчшщъыьэюя"
IDX = {c: i + 1 for i, c in enumerate(ALPH)}

def enc(w):
    return "".join(str(IDX[c]) for c in w)

assert enc("привет") == "1718103620", enc("привет")

words = []
with open(DICT, encoding="utf-8", errors="ignore") as f:
    for line in f:
        parts = line.split()
        if not parts:
            continue
        w = parts[0].lower()
        if w.isalpha() and all(c in IDX for c in w) and 3 <= len(w) <= 8:
            words.append(w)
words = words[:9000]

enc2word = {}
for w in words:
    enc2word.setdefault(enc(w), w)
ES = set(enc2word)
print(f"словарь: {len(words)} слов -> {len(ES)} уникальных цифровых кодов", flush=True)

LO, HI = 10**17, 9 * 10**18
SPAN = HI - LO
rng = random.Random(time.time())

MINL, MAXL = 4, 13
RANGES = [(i, ln) for i in range(19) for ln in range(MINL, min(MAXL, 19 - i) + 1)]

get = enc2word.get
best = []
n2 = n3 = 0
N = 0
t0 = time.time()
next_report = 25.0

while True:
    dt = time.time() - t0
    if dt >= RUNTIME:
        break
    if dt >= next_report:
        print(f"... {N} чисел | чтений с 2+ словами: {n2} | с 3+ словами: {n3}", flush=True)
        next_report += 25.0
    s = str(LO + rng.getrandbits(63) % SPAN)
    N += 1
    hits = []
    for i, ln in RANGES:
        w = get(s[i:i + ln])
        if w:
            hits.append((i, i + ln, w))
    if len(hits) < 2:
        continue
    hits.sort(key=lambda h: (h[0], -(h[1] - h[0])))
    sel = []
    last = -1
    for h in hits:
        if h[0] > last:
            sel.append(h)
            last = h[1]
    if len(sel) < 2:
        continue
    cov = sel[-1][1] - sel[0][0]
    nwords = len(sel)
    if nwords >= 3:
        n3 += 1
    n2 += 1
    best.append((nwords, cov, s, sel))
    if len(best) > 4000:
        best.sort(key=lambda b: (b[0], b[1]))
        del best[:1000]

el = time.time() - t0
print(f"\nитог: {N} чисел за {el:.0f} сек ({N / el:.0f} чисел/сек)")
print(f"чисел, читающихся как 2+ словарных слова: {n2} | как 3+ слова: {n3}")

best.sort(key=lambda b: (-b[0], -b[1]))
print("\n=== ЧИСЛА-ПРЕДЛОЖЕНИЯ (точки = цифры вне слов) ===")
shown = 0
seen = set()
for nwords, cov, s, sel in best:
    phrase = " ".join(w for _, _, w in sel)
    if phrase in seen:
        continue
    seen.add(phrase)
    parts = []
    cur = 0
    for i, j, w in sel:
        if i > cur:
            parts.append("." * (i - cur))
        parts.append(s[i:j])
        cur = j
    if cur < len(s):
        parts.append("." * (len(s) - cur))
    print(f"{s}  =>  {phrase}   [{'.'.join(parts)}] ({nwords} сл., {cov}/{len(s)} цифр)", flush=True)
    shown += 1
    if shown >= 25:
        break
