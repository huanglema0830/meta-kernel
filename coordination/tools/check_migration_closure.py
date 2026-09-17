#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""迁移**闭包**判据 + **分片规划**（2.3b 分片迁移专用）。

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

## 三个模式
| 模式 | 作用 |
|---|---|
| （默认）**check** | 判「当前已迁集合是否封闭」，不封闭则给**最小补迁集** |
| `--plan` | **脚本产出分片**（D40）：把未迁模块按"依赖是否已就位"**分层**，层 1 = 现在就能整片迁的自包含片 |
| `--emit N` | 打印**第 N 层的模块清单**（供迁移脚本直接消费，**不再手写清单**） |
| `--selftest` | **阴阳自检**：注入"普通缺口"与"仅测试缺口"两侧样例，必须都被检出；无缺口时必须放行 |

## 判据
* **[1] 封闭性**：已迁模块引用的每个 `crate::X`，X 必须**已在目标 crate** 或**不在源 crate**（如 `std`/`core`）。
* **[2] 递归闭包**：若 [1] 不通过，给出**最小补迁集**（含其自身依赖的递归展开）与行数。
* **[3] 测试依赖单列**：把"只在 `#[cfg(test)]` 内出现"的缺口**单独标出**——
  它们**不会**让裸机构建失败，**只会**让 `cargo test` 失败（最容易被漏掉的一类）。
* **[4] 分层（--plan）**：`层 k = 所有未迁依赖都已在「已迁 ∪ 更早层」的模块`。
  ⚠️ 分层用的引用**含测试块**（与 [1] 同口径），否则会把"测试缺口"留到下一片才炸。

用法：
    python coordination/tools/check_migration_closure.py \\
        --src meta-kernel-core/src --dst meta-kernel-core-nostd/src
    python coordination/tools/check_migration_closure.py --plan
    python coordination/tools/check_migration_closure.py --emit 1
    python coordination/tools/check_migration_closure.py --selftest
"""
from __future__ import annotations

import argparse
import io
import os
import re
import shutil
import sys
import tempfile
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
        g = m.group(1)
        s.add(g)
        s.add(g.split("::")[0])
        s.add(g.replace("::", "/"))          # `l7::grade` → `l7/grade`
    return s


def build_deps(src: dict[str, Path]) -> dict[str, set[str]]:
    """每个源模块 → 它引用到的**其它源模块键**（**含测试块**，见文件头）。"""
    keys = list(src)
    out: dict[str, set[str]] = {}
    for m, p in src.items():
        txt = io.open(p, encoding="utf-8").read()
        ds: set[str] = set()
        for r in refs(txt):
            for k in keys:
                if k == m:
                    continue
                if k == r or k.split("/")[-1] == r:
                    ds.add(k)
        out[m] = ds
    return out


def line_count(p: Path) -> int:
    return sum(1 for _ in io.open(p, encoding="utf-8"))


def layerize(remaining: set[str], deps: dict[str, set[str]], migrated: set[str]) -> list[list[str]]:
    """层 k = 未迁依赖 ⊆ (已迁 ∪ 更早层)。成环且无解时返回剩余集合作为最后一层（并标注）。"""
    layers: list[list[str]] = []
    done = set(migrated)
    rest = set(remaining)
    while rest:
        cur = sorted(m for m in rest if deps.get(m, set()) <= done)
        if not cur:
            layers.append(sorted(rest))      # 环形残留：整块一起迁（片内成环不影响封闭性）
            break
        layers.append(cur)
        done |= set(cur)
        rest -= set(cur)
    return layers


# ============================== 三个模式 ==============================

def mode_check(src: dict[str, Path], dst: dict[str, Path]) -> int:
    gaps: dict[str, set[str]] = {}
    test_only: dict[str, set[str]] = {}
    for m, p in sorted(dst.items()):
        txt = io.open(p, encoding="utf-8").read()
        all_ref = {r for r in refs(txt) if r in src and r not in dst}
        nt_ref = {r for r in refs(strip_test(txt)) if r in src and r not in dst}
        if all_ref:
            gaps[m] = all_ref
            if all_ref - nt_ref:
                test_only[m] = all_ref - nt_ref

    if not gaps:
        print("[PASS] [1] 封闭性：目标 crate 未引用任何「源 crate 有、目标 crate 无」的模块")
    else:
        print("[FAIL] [1] 封闭性：存在缺口")
        for m, ds in gaps.items():
            tag = " ← ⚠️ **只在 #[cfg(test)] 内**（裸机绿、`cargo test` 红）" if m in test_only else ""
            print(f"       {m} → 缺 {sorted(ds)}{tag}")

    need: set[str] = set()
    for ds in gaps.values():
        need |= ds
    if need:
        while True:
            grew = False
            for m in sorted(need):
                for r in refs(io.open(src[m], encoding="utf-8").read()):
                    for k in src:
                        if k.split("/")[-1] == r and k not in dst and k not in need:
                            need.add(k)
                            grew = True
            if not grew:
                break
        tot = 0
        print("\n[info] [2] 最小补迁集：")
        for m in sorted(need):
            n = line_count(src[m])
            tot += n
            print(f"       {m:20s} {n:5d} 行")
        print(f"       合计 {len(need)} 模块 / {tot} 行")
        print("\n结果：❌ 不封闭（须补迁后再验）")
        return 1

    print("\n[info] [3] 测试依赖单列：" + ("（无缺口）" if not test_only else str(test_only)))
    print("\n结果：✅ 封闭（裸机构建与 `cargo test` 两侧前提都已满足）")
    return 0


def plan_core(src: dict[str, Path], dst: dict[str, Path]) -> list[list[str]]:
    deps = build_deps(src)
    remaining = {m for m in src if m not in dst and not m.endswith("/mod") and m != "lib"}
    return layerize(remaining, deps, set(dst))


def mode_plan(src: dict[str, Path], dst: dict[str, Path]) -> int:
    layers = plan_core(src, dst)
    total = sum(line_count(src[m]) for ls in layers for m in ls)
    print(f"[info] 源 crate {len(src)} 模块｜已迁 {len(dst)} 模块｜"
          f"未迁 {sum(len(ls) for ls in layers)} 模块 / {total} 行")
    print("[info] 分层口径：**层 k = 所有未迁依赖都已在「已迁 ∪ 更早层」的模块**"
          "（引用**含 `#[cfg(test)]` 块**，与封闭性判据同口径）")
    for i, ls in enumerate(layers, 1):
        n = sum(line_count(src[m]) for m in ls)
        print(f"\n  ── 片 {i}（＝层 {i}）：{len(ls)} 模块 / {n} 行"
              f"{'  ← **现在就能整片迁**' if i == 1 else '  ← 需等前片完成'}")
        for m in sorted(ls, key=lambda x: -line_count(src[x])):
            print(f"      {m:22s} {line_count(src[m]):5d} 行")
    print(f"\n结果：✅ 已产出 {len(layers)} 片（片 1 即本片；**由脚本产出，不手写**）")
    return 0


def mode_emit(src: dict[str, Path], dst: dict[str, Path], n: int) -> int:
    layers = plan_core(src, dst)
    if not 1 <= n <= len(layers):
        print(f"❌ 层号越界：{n}（共 {len(layers)} 层）")
        return 2
    print(" ".join(layers[n - 1]))
    return 0


def mode_selftest() -> int:
    """**阴阳自检**：注入正反两侧样例，验证判据既不空转也不误报。"""
    tmp = Path(tempfile.mkdtemp(prefix="closure_st_"))
    src, dst = tmp / "src", tmp / "dst"
    src.mkdir(), dst.mkdir()
    ok = True

    def W(p: Path, t: str) -> None:
        io.open(p, "w", encoding="utf-8", newline="\n").write(t)

    def build(files_src: dict[str, str], files_dst: dict[str, str]):
        for f in list(src.glob("*.rs")):
            f.unlink()
        for f in list(dst.glob("*.rs")):
            f.unlink()
        for k, v in files_src.items():
            W(src / f"{k}.rs", v)
        for k, v in files_dst.items():
            W(dst / f"{k}.rs", v)

    import contextlib
    buf = io.StringIO()

    # ——— ① 正例：封闭 → 必须放行 ———
    build({"a": "pub fn f() {}\n", "b": "use crate::a;\npub fn g() { a::f() }\n"},
          {"a": "pub fn f() {}\n", "b": "use crate::a;\npub fn g() { a::f() }\n"})
    with contextlib.redirect_stdout(buf):
        rc = mode_check(modules(src), modules(dst))
    p1 = (rc == 0)
    print(f"  [阴阳自检 ①] 封闭样例 → 判 {rc}（期望 0）{'✅' if p1 else '❌'}")
    ok &= p1

    # ——— ② 反例 A：普通缺口 → 必须判红 ———
    build({"a": "pub fn f() {}\n", "b": "use crate::a;\npub fn g() { a::f() }\n"},
          {"b": "use crate::a;\npub fn g() { a::f() }\n"})
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        rc = mode_check(modules(src), modules(dst))
    out = buf.getvalue()
    p2 = (rc == 1 and "缺 ['a']" in out)
    print(f"  [阴阳自检 ②] 普通缺口样例 → 判 {rc}（期望 1）且点名 a：{'✅' if p2 else '❌'}")
    ok &= p2

    # ——— ③ 反例 B：**只在 #[cfg(test)] 内**的缺口 → 必须判红**并单独标出** ———
    build({"a": "pub fn f() {}\n"},
          {"b": "pub fn g() {}\n\n#[cfg(test)]\nmod t {\n    #[test]\n    fn x() { crate::a::f(); }\n}\n"})
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        rc = mode_check(modules(src), modules(dst))
    out = buf.getvalue()
    p3 = (rc == 1 and "只在 #[cfg(test)] 内" in out)
    print(f"  [阴阳自检 ③] **仅测试**缺口样例 → 判 {rc}（期望 1）且标出「只在 cfg(test)」：{'✅' if p3 else '❌'}")
    ok &= p3

    # ——— ④ 分层：环必须不发散（同层一起吃）———
    build({"r1": "use crate::r2;\npub fn f() { r2::g() }\n", "r2": "use crate::r1;\npub fn g() { r1::f() }\n",
           "z": "pub fn h() {}\n"}, {})
    lay = plan_core(modules(src), modules(dst))
    p4 = (len(lay) >= 1 and sorted(m for ls in lay for m in ls) == ["r1", "r2", "z"])
    print(f"  [阴阳自检 ④] 环形样例 → 分层 {lay}（期望含全部 3 模块且不发散）：{'✅' if p4 else '❌'}")
    ok &= p4

    shutil.rmtree(tmp, ignore_errors=True)
    print(f"\n自检结论：{'✅ 判据正反两侧均符合预期' if ok else '❌ 判据存在问题，须修'}")
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", default="meta-kernel-core/src")
    ap.add_argument("--dst", default="meta-kernel-core-nostd/src")
    ap.add_argument("--plan", action="store_true", help="按依赖分层**产出分片**（D40）")
    ap.add_argument("--emit", type=int, default=0, help="打印第 N 层的模块清单")
    ap.add_argument("--selftest", action="store_true", help="阴阳自检")
    a = ap.parse_args()

    if a.selftest:
        return mode_selftest()

    src, dst = modules(Path(a.src)), modules(Path(a.dst))
    if not a.plan and not a.emit:
        print(f"[info] 源 crate 模块 {len(src)} 个｜目标 crate 模块 {len(dst)} 个")
    if a.emit:
        return mode_emit(src, dst, a.emit)
    if a.plan:
        return mode_plan(src, dst)
    return mode_check(src, dst)


if __name__ == "__main__":
    sys.exit(main())
