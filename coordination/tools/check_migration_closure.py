#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""迁移**闭包**判据（2.3b 分片迁移专用）：判断"当前已迁集合是否封闭"。

## 为什么必须有它（片4 用真金白银换来的）
算分片时我**剔除了 `#[cfg(test)]` 依赖**——这对**裸机构建是对的**（`cfg(test)` 关闭），
但对 **host `cargo test` 是错的**（测试被打开）。结果片4 首跑：
* `cargo build --target x86_64-unknown-none` **绿**（看不出来）
* `cargo test` **红**：`l1_field_parse.rs` 的**测试模块**引用了未迁的 `crate::habit`

⇒ **教训：迁移集必须同时对「非测试依赖」与「测试依赖」封闭**。
⇒ 本工具**不剔除测试**，专抓这类"**只在对的判据下才暴露**"的缺口。

## 还纠正了一个更根本的说法（片4 同样暴露）
订正清单写「片4 严格链式（后者依赖前者）」——**错**。
* Rust **同 crate 内模块可互引、无需拓扑序**；
* `gene_library` ↔ `l5_context` **就是互引成环**；
⇒ 真正的约束是「**迁移集在目标 crate 内封闭**」，**不是**"先后顺序"。

## 判据
* **[1] 封闭性**：已迁模块引用的每个 `crate::X`，X 必须**已在目标 crate** 或**不在源 crate**（如 `std`/`core`）。
* **[2] 递归闭包**：若 [1] 不通过，给出**最小补迁集**（含其自身依赖的递归展开）与行数。
* **[3] 测试依赖单列**：把"只在 `#[cfg(test)]` 内出现"的缺口**单独标出**——
  它们**不会**让裸机构建失败，**只会**让 `cargo test` 失败（最容易被漏掉的一类）。

用法：
    python coordination/tools/check_migration_closure.py \\
        --src meta-kernel-core/src --dst meta-kernel-core-nostd/src
"""
from __future__ import annotations

import argparse
import io
import os
import re
from pathlib import Path

CFG_TEST = re.compile(r"#\[cfg\(test\)\]")
REF = re.compile(r"crate::([a-z0-9_]+(?:::[a-z0-9_]+)?)")


def strip_test(txt: str) -> str:
    """剔除 `#[cfg(test)]` 起的整块（花括号配对）。"""
    spans: list[tuple[int, int]] = []
    for m in CFG_TEST.finditer(txt):
        j = txt.find("{", m.end())
        if j < 0:
            continue
        d, k = 0, j
        while k < len(txt):
            if txt[k] == "{":
                d += 1
            elif txt[k] == "}":
                d -= 1
                if d == 0:
                    break
            k += 1
        spans.append((m.start(), k + 1))
    if not spans:
        return txt
    out, last = [], 0
    for a, b in sorted(spans):
        if a < last:
            continue
        out.append(txt[last:a])
        last = b
    out.append(txt[last:])
    return "".join(out)


def modules(root: Path) -> dict[str, Path]:
    out: dict[str, Path] = {}
    for dp, _, fs in os.walk(root):
        for f in fs:
            if not f.endswith(".rs"):
                continue
            p = Path(dp) / f
            rel = p.relative_to(root).with_suffix("").as_posix()
            if rel.endswith("/mod"):
                rel = rel[:-4]
            out[rel] = p
    return out


def refs(txt: str) -> set[str]:
    s = set()
    for m in REF.finditer(txt):
        s.add(m.group(1))
        s.add(m.group(1).split("::")[0])
    return s


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", default="meta-kernel-core/src")
    ap.add_argument("--dst", default="meta-kernel-core-nostd/src")
    a = ap.parse_args()

    src, dst = modules(Path(a.src)), modules(Path(a.dst))
    print(f"[info] 源 crate 模块 {len(src)} 个｜目标 crate 模块 {len(dst)} 个")

    # ——— [1] 封闭性（含测试）———
    gaps: dict[str, set[str]] = {}       # 模块 → 缺失依赖（含测试）
    test_only: dict[str, set[str]] = {}  # 模块 → 只在测试里出现的缺失依赖
    for m, p in sorted(dst.items()):
        txt = io.open(p, encoding="utf-8").read()
        all_ref = {r for r in refs(txt) if r in src and r not in dst}
        nt_ref = {r for r in refs(strip_test(txt)) if r in src and r not in dst}
        if all_ref:
            gaps[m] = all_ref
            only_test = all_ref - nt_ref
            if only_test:
                test_only[m] = only_test

    if not gaps:
        print("[PASS] [1] 封闭性：目标 crate 未引用任何「源 crate 有、目标 crate 无」的模块")
    else:
        print("[FAIL] [1] 封闭性：存在缺口")
        for m, ds in gaps.items():
            tag = " ← ⚠️ **只在 #[cfg(test)] 内**（裸机绿、`cargo test` 红）" if m in test_only else ""
            print(f"       {m} → 缺 {sorted(ds)}{tag}")

    # ——— [2] 递归闭包 ———
    need: set[str] = set()
    for ds in gaps.values():
        need |= ds
    if need:
        while True:
            grew = False
            for m in sorted(need):
                for r in refs(io.open(src[m], encoding="utf-8").read()):
                    if r in src and r not in dst and r not in need:
                        need.add(r)
                        grew = True
            if not grew:
                break
        tot = 0
        print("\n[info] [2] 最小补迁集：")
        for m in sorted(need):
            n = sum(1 for _ in io.open(src[m], encoding="utf-8"))
            tot += n
            print(f"       {m:20s} {n:5d} 行")
        print(f"       合计 {len(need)} 模块 / {tot} 行")
        print("\n结果：❌ 不封闭（须补迁后再验）")
        return 1

    print("\n[info] [3] 测试依赖单列：" + ("（无缺口）" if not test_only else str(test_only)))
    print("\n结果：✅ 封闭（裸机构建与 `cargo test` 两侧前提都已满足）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
