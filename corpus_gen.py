import json
import re
import sys
import urllib.request

URL = "http://127.0.0.1:8817/v1/chat/completions"
N_CALLS = int(sys.argv[1]) if len(sys.argv) > 1 else 70
ALPH = set("абвгдеёжзийклмнопрстуфхцчшщъыьэюя-")


def ask():
    body = json.dumps({
        "model": "qwen1.5-4b-chat",
        "messages": [{"role": "user", "content":
            "Составь 10 разных коротких простых русских предложений из 2-5 слов. "
            "По одному в строке, без номеров. Тема любая: быт, люди, природа, еда."}],
        "max_tokens": 150, "temperature": 1.0, "top_p": 0.95
    }).encode()
    req = urllib.request.Request(URL, data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=150) as r:
        d = json.loads(r.read())
    return d["choices"][0]["message"]["content"]


out = open("corpus.txt", "a", encoding="utf-8")
prog = open("corpus_progress.txt", "w", encoding="utf-8")
kept_total = 0
seen = set()
for i in range(N_CALLS):
    txt = ""
    try:
        txt = ask()
    except Exception:
        pass
    kept = 0
    for line in txt.splitlines():
        toks = [w.lower() for w in re.split(r"[\s,.;:!?«»\"()\[\]-]+", line.strip())
                if w and not w.isdigit()]
        toks = [w for w in toks if 2 <= len(w) <= 10 and set(w) <= ALPH]
        if 2 <= len(toks) <= 6:
            s = " ".join(toks)
            if s not in seen:
                seen.add(s)
                out.write(s + "\n")
                kept += 1
    kept_total += kept
    out.flush()
    prog.seek(0)
    prog.truncate()
    prog.write(f"{i + 1}/{N_CALLS} calls, sentences={kept_total}")
    prog.flush()
prog.write("\nDONE\n")
prog.flush()
out.close()
prog.close()
print("DONE", kept_total)
