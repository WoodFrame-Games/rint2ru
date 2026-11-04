import json
import sys
import time
import urllib.parse
import urllib.request

API = "https://ru.wikisource.org/w/api.php"
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
    return [p.get("extract") or "" for p in d.get("query", {}).get("pages", [])]


N = int(sys.argv[1]) if len(sys.argv) > 1 else 100
out = open("corpus2.txt", "a", encoding="utf-8")
total = 0
fails = 0
for i in range(N):
    try:
        parts = fetch()
    except Exception as e:
        fails += 1
        if fails > 10:
            print(f"TOO MANY FAILS: {e}", flush=True)
            break
        time.sleep(1.5)
        continue
    fails = 0
    for t in parts:
        txt = " ".join(t.split())
        if len(txt) > 50:
            out.write(txt + "\n")
            total += len(txt)
    out.flush()
    if (i + 1) % 10 == 0:
        print(f"{i + 1}/{N} батчей, {total / 1e6:.2f} МБ", flush=True)
print(f"DONE {total / 1e6:.2f} МБ", flush=True)