import json
import sys
import time
import urllib.parse
import urllib.request

API = "https://ru.wikipedia.org/w/api.php"
UA = "randnum-research/1.0 (educational experiment)"


def fetch(batch=20):
    params = {
        "action": "query", "generator": "random", "grnnamespace": "0",
        "grnlimit": str(batch), "prop": "extracts", "explaintext": "1",
        "exlimit": "max", "format": "json", "formatversion": "2"
    }
    url = API + "?" + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=90) as r:
        d = json.loads(r.read())
    out = []
    for p in d.get("query", {}).get("pages", []):
        t = p.get("extract") or ""
        if t:
            out.append(t)
    return out


N = int(sys.argv[1]) if len(sys.argv) > 1 else 100
try:
    probe = fetch(2)
    print(f"API OK, пробная выборка: {sum(len(t) for t in probe)} байт", flush=True)
except Exception as e:
    print(f"API BLOCKED: {e}", flush=True)
    sys.exit(1)

out = open("corpus2.txt", "a", encoding="utf-8")
total = 0
for i in range(N):
    try:
        parts = fetch()
    except Exception as e:
        print(f"err {i}: {e}", flush=True)
        time.sleep(2)
        continue
    for t in parts:
        txt = " ".join(t.split())
        if len(txt) > 50:
            out.write(txt + "\n")
            total += len(txt)
    out.flush()
    if (i + 1) % 10 == 0:
        print(f"{i + 1}/{N} батчей, {total / 1e6:.1f} МБ", flush=True)
print(f"DONE {total / 1e6:.1f} МБ", flush=True)