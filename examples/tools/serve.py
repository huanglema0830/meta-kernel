#!/usr/bin/env python3
"""examples/tools/serve.py — 一键本地预览任意示例目录.

用法:
    python3 examples/tools/serve.py [目录] [端口]
默认目录 examples/zen-oscilloscope，默认端口 8000。
浏览器自动打开 http://localhost:8000
"""
import os
import sys
import threading
import webbrowser
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

ROOT = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(ROOT)  # examples/
BASE = os.path.join(os.path.dirname(REPO), "examples")
port = 8000

args = [a for a in sys.argv[1:] if not a.startswith("-")]
if len(args) >= 1:
    target = os.path.abspath(args[0])
else:
    target = os.path.join(BASE, "zen-oscilloscope")
if len(args) >= 2:
    port = int(args[1])

if not os.path.isdir(target):
    print(f"[serve] 目录不存在: {target}")
    sys.exit(1)

os.chdir(target)
url = f"http://localhost:{port}"
print(f"[serve] 目录: {target}")
print(f"[serve] 打开: {url}   (Ctrl+C 停止)")

threading.Timer(1.0, lambda: webbrowser.open(url)).start()
ThreadingHTTPServer(("127.0.0.1", port), SimpleHTTPRequestHandler).serve_forever()
