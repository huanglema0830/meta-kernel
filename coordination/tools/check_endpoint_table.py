#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""端点表一致性判据（**机制 24**；Q14 裁定 · 2026-09-18）。

## 它判什么

**代码里的路由，底图列全了吗。** 权威口径＝`docs/LAYER_DETAILS.md` 的 **§4.L3 端点表**（Q12 裁定）。
★ **v0.329 轮指针更新**：该表原在 `docs/LAYER_ARCHITECTURE.md`；**底图拆文件**（裁定④）后
**§3–§4 迁入 `docs/LAYER_DETAILS.md`**（**节号未变**）⇒ 本判据的 `TABLE_FILE` **随之更新**
（**未改口径、只改指针** · `C19` 同源）。

- **范围**：`npb-gateway/src/http.rs` 的**非测试区**（`#[cfg(test)]` 之前）——与机制 21 同口径。
- **比什么**：**路径**（不含 method；method 差异由权威表列内写明）。
- **判据**：差集必须**等于白名单**：
  * **代码有、权威表无** ⇒ 新增即判红（**必须**先改权威表）；白名单内豁免。
  * **权威表有、代码无** ⇒ **只提示**（可能是"已设计未实现"或"已下线"）——**不判红**，
    但**要求表里如实标注状态**（避免底图声称"已有"而其实没有）。

## 为什么需要它（不改会怎样）

L3 端点表此前**有 7 处互不一致的口径**，且**代码演进快于文档同步**：实测
**28 条路由 / 23 个路径**，而当时最全的一处口径只覆盖 **8 个** ⇒ 差集 **15 个**。
**其中 7 个（`/v1/msg|msg.txt|report|report.txt|alerts|tasks|tasks.txt`）在 `docs/` 35 份中命中 0 份**
（阳性对照 `/v1/probe` 命中 6 份 ⇒ 检索式有效）——**新人照文档实现会漏掉一整族端点**。
⇒ 把「文档跟不上代码」从**人工发现**变成**机器拦截**。

## 与机制 21／22 的同构（判据设计模式）

| 模式 | 本脚本的落点 |
|---|---|
| **正反对照** | `--selftest`：合规样例应 0；**代码多一个端点**应 1；权威表多一个应 0（只提示）；仅测试应 0；**真实仓库锚点应 >0** |
| **自检** | `--selftest` **五侧**断言（合规／代码多／表多／仅测试／真实仓库锚点），不空跑 |
| **基线只减不增** | `endpoint_whitelist.txt`（当前 **0 条**）；**新增豁免须人工登记** |
| **口径写清** | 报数一律写明「**路径口径**（非 method 口径）」——R35 |

## 用法

    python coordination/tools/check_endpoint_table.py             # 检查
    python coordination/tools/check_endpoint_table.py --selftest  # 判据自检（五侧）
    python coordination/tools/check_endpoint_table.py --list      # 只打印两侧清单（只读）
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

TABLE_FILE = "docs/LAYER_DETAILS.md"   # ★ v0.329 轮：原 `docs/LAYER_ARCHITECTURE.md` §4.L3 随**拆分**迁入本文件
TABLE_ANCHOR = "##### §4.L3 端点表"
CODE_FILE = "npb-gateway/src/http.rs"
WHITELIST = "coordination/security/endpoint_whitelist.txt"

PATH_RE = re.compile(r"/v1/[A-Za-z0-9_./]+")
CFG_TEST = re.compile(r"#\[cfg\(test\)\]")


def find_repo() -> Path:
    return Path(__file__).resolve().parents[2]


def strip_test(txt: str) -> str:
    """只留 `#[cfg(test)]` 之前的部分（与机制 21 同口径）。"""
    m = CFG_TEST.search(txt)
    return txt[: m.start()] if m else txt


def paths_from_text(txt: str) -> set:
    out = set()
    for m in PATH_RE.finditer(txt):
        s = m.group(0).rstrip("./")
        if s.startswith("/v1/") and len(s) > 4:
            out.add(s)
    return out


def authoritative_paths(repo: Path) -> set:
    """从权威表的**表体行**抽路径（不抽正文，避免把示例/别名算进来）。"""
    t = (repo / TABLE_FILE).read_text(encoding="utf-8").split("\n")
    start = None
    for i, l in enumerate(t):
        if l.startswith(TABLE_ANCHOR):
            start = i
            break
    if start is None:
        return set()
    out = set()
    for l in t[start:]:
        if l.startswith("##### ") and not l.startswith(TABLE_ANCHOR):
            break
        if l.startswith("|"):
            out |= paths_from_text(l)
    return out


def code_paths(repo: Path) -> set:
    return paths_from_text(strip_test((repo / CODE_FILE).read_text(encoding="utf-8")))


def load_whitelist(repo: Path) -> set:
    p = repo / WHITELIST
    if not p.exists():
        return set()
    return {l.strip() for l in p.read_text(encoding="utf-8").split("\n")
            if l.strip() and not l.strip().startswith("#")}


def check(repo: Path) -> int:
    auth = authoritative_paths(repo)
    code = code_paths(repo)
    wl = load_whitelist(repo)
    print("=== 端点表一致性判据（机制 24）===")
    print("  口径：**路径口径**（不含 method；method 差异由权威表行内写明）. R35")
    print("  权威表：%s §4.L3" % TABLE_FILE)
    print("  代码范围：%s 非测试区" % CODE_FILE)
    if not auth:
        print("  ❌ 权威表未解析到任何路径（锚点丢失？）⇒ 判红")
        print("结果：❌ 不通过")
        return 1
    print("  权威路径 %d 个｜代码路径 %d 个｜白名单 %d 条"
          % (len(auth), len(code), len(wl)))

    missing = sorted(code - auth - wl)   # 代码有、权威表无 （未登记）⇒ 判红
    stale = sorted(auth - code)          # 权威表有、代码无 ⇒ 只提示

    print("\n[1] 代码有、权威表无（必须 0）：")
    if missing:
        print("  ❌ %d 个：%s" % (len(missing), missing))
    else:
        print("  ✅ 0 个")
    print("[2] 权威表有、代码无（仅提示：需在表内如实标注状态）：")
    if stale:
        print("  ⚠️ %d 个：%s" % (len(stale), stale))
    else:
        print("  ✅ 0 个")
    ok = not missing
    print("\n结果：%s" % ("✅ 一致" if ok else "❌ 不通过（代码有未纳入权威表的路径）"))
    if missing:
        print("  ⇒ 请先把它们写进 %s §4.L3；若确属内部/调试端点，登记到 %s" % (TABLE_FILE, WHITELIST))
    return 0 if ok else 1


def selftest() -> int:
    """五侧自检：合规=0｜代码多一个=1｜权威表多一个=0（只提示）｜仅测试=0｜真实仓库锚点>0。"""
    import tempfile
    print("=== 判据自检（五侧）===")
    with tempfile.TemporaryDirectory() as td:
        repo = Path(td)
        (repo / "docs").mkdir()
        (repo / "npb-gateway" / "src").mkdir(parents=True)
        (repo / "coordination" / "security").mkdir(parents=True)

        def write(table_rows, code_txt):
            (repo / TABLE_FILE).write_text(
                TABLE_ANCHOR + " ★\n\n| # | M | 路径 |\n|---|---|---|\n" + table_rows + "\n",
                encoding="utf-8")
            (repo / CODE_FILE).write_text(code_txt, encoding="utf-8")

        # ① 合规
        write("| 1 | GET | `/v1/push` |\n| 2 | GET | `/v1/state` |\n",
              'if target == "/v1/push" {}\nif target == "/v1/state" {}\n')
        r1 = check(repo)
        print("  ① 合规样例 ⇒ %d（期望 0）%s" % (r1, "✅" if r1 == 0 else "❌"))

        # ② 代码多一个 ⇒ 应判红
        write("| 1 | GET | `/v1/push` |\n",
              'if target == "/v1/push" {}\nif target == "/v1/health" {}\n')
        r2 = check(repo)
        print("  ② 代码多一个端点 ⇒ %d（期望 1）%s" % (r2, "✅" if r2 == 1 else "❌"))

        # ③ 权威表多一个 ⇒ 只提示，不判红
        write("| 1 | GET | `/v1/push` |\n| 2 | GET | `/v1/notyet` |\n",
              'if target == "/v1/push" {}\n')
        r3 = check(repo)
        print("  ③ 权威表多一个 ⇒ %d（期望 0，仅提示）%s" % (r3, "✅" if r3 == 0 else "❌"))

        # ④ 测试区不计（与机制 21 同口径）
        write("| 1 | GET | `/v1/push` |\n",
              'if target == "/v1/push" {}\n#[cfg(test)]\nmod t { fn a() { let _ = "/v1/secret"; } }\n')
        r4 = check(repo)
        print("  ④ 路由只在 `#[cfg(test)]` 内 ⇒ %d（期望 0）%s" % (r4, "✅" if r4 == 0 else "❌"))

    # ⑤ 锚点实跑自检：真实仓库的权威表必须能被解析到（防"夹具与常量同错"）
    real = authoritative_paths(find_repo())
    print("  ⑤ 真实仓库锚点解析 ⇒ %d 个路径（期望 >0）%s"
          % (len(real), "✅" if real else "❌"))

    good = (r1 == 0 and r2 == 1 and r3 == 0 and r4 == 0 and bool(real))
    print("\n自检结论：%s（合规=0 应 0；代码多=1 应 1；表多=0 应 0；仅测试=0 应 0；锚点>0 应 >0）"
          % ("✅ 五侧均符合预期" if good else "❌ 判据失效"))
    return 0 if good else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--list", action="store_true")
    a = ap.parse_args()
    repo = find_repo()
    if a.selftest:
        return selftest()
    if a.list:
        print("权威表：", sorted(authoritative_paths(repo)))
        print("代码：", sorted(code_paths(repo)))
        return 0
    return check(repo)


if __name__ == "__main__":
    raise SystemExit(main())
