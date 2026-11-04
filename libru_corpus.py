import html
import re
import sys
import time
import urllib.parse
import urllib.request

UA = "Mozilla/5.0"
BASE = "http://lib.ru"


def get(url, timeout=60):
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()


def links(page):
    return re.findall(r'href=["\']?([^"\'\s>]+)', page, re.I)


def strip_text(raw):
    s = raw.decode("koi8-r", errors="ignore")
    s = html.unescape(s)
    s = re.sub(r"(?is)<(script|style|form|select|option).*?</\1>", " ", s)
    s = re.sub(r"(?s)<[^>]+>", "\n", s)
    out = []
    for line in s.splitlines():
        line = " ".join(line.split())
        if len(line) < 20:
            continue
        letters = sum(1 for c in line if c.isalpha())
        if letters < len(line) * 0.6:
            continue
        low = line.lower()
        if "lib.ru" in low or "http" in low or "-->":
            continue
        out.append(line)
    return out


cats = ["/PROZA/", "/RUSS_DETEKTIW/", "/INPROZ/"]
out = open("corpus2.txt", "a", encoding="utf-8")
total = 0
files = 0
for cat in cats:
    try:
        page = get(BASE + cat).decode("koi8-r", errors="ignore")
    except Exception as e:
        print(f"категория {cat} fail: {e}", flush=True)
        continue
    subs, seen = [], set()
    for h in links(page):
        if h.endswith("/") and re.fullmatch(r"[A-Za-z0-9_.\-]+/", h):
            if h not in seen:
                seen.add(h)
                subs.append(h)
    picked = 0
    for sub in subs:
        if picked >= 8 or files >= 60:
            break
        try:
            apage = get(BASE + cat + sub).decode("koi8-r", errors="ignore")
        except Exception:
            continue
        txts = [h for h in links(apage)
                if h.lower().endswith(".txt") and not h.startswith("http")]
        for t in txts[:2]:
            if files >= 60:
                break
            u = urllib.parse.urljoin(BASE + cat + sub, t)
            try:
                raw = get(u)
            except Exception:
                continue
            lines = strip_text(raw)
            if lines:
                out.write("\n".join(lines) + "\n")
                total += sum(len(x) for x in lines)
                files += 1
                print(f"{files} {total/1e6:.1f}MB {u}", flush=True)
                time.sleep(0.05)
        picked += 1
        out.flush()
    print(f"{cat}: файлов={files}, {total / 1e6:.1f} МБ", flush=True)
out.close()
print(f"DONE файлов={files}, {total / 1e6:.1f} МБ", flush=True)