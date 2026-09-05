#!/usr/bin/env python3
"""examples/tools/embed_wasm.py — 生成可“双击打开”的离线 HTML

浏览器禁止 file:// 下 fetch 本地文件，因此把 npb.wasm 以 base64 内嵌进
HTML（window.NPB_B64），JS 优先走内嵌字节，双击即可运行。

用法:
    python3 examples/tools/embed_wasm.py <index.html> <npb.wasm> [输出名]
默认输出为 <index.html 同目录>/index_embedded.html
"""
import base64
import os
import sys

def main() -> int:
    if len(sys.argv) < 3:
        print("用法: embed_wasm.py <index.html> <npb.wasm> [输出名]")
        return 2
    html_path = os.path.abspath(sys.argv[1])
    wasm_path = os.path.abspath(sys.argv[2])
    out_path = os.path.abspath(sys.argv[3]) if len(sys.argv) >= 4 else \
        os.path.join(os.path.dirname(html_path), "index_embedded.html")

    with open(html_path, "r", encoding="utf-8") as f:
        html = f.read()
    with open(wasm_path, "rb") as f:
        b64 = base64.b64encode(f.read()).decode("ascii")

    inject = f'<script>window.NPB_B64="{b64}";</script>'
    marker = "<script src="
    idx = html.find(marker)
    if idx == -1:
        print(f"[embed] 未找到脚本注入点: {html_path}")
        return 1
    html = html[:idx] + inject + "\n" + html[idx:]

    with open(out_path, "w", encoding="utf-8") as f:
        f.write(html)
    kb = os.path.getsize(out_path) // 1024
    print(f"[embed] OK -> {out_path} ({kb} KB) 双击即可运行")
    return 0

if __name__ == "__main__":
    sys.exit(main())
