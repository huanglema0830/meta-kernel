#!/usr/bin/env python3
"""迁移**保真度**判据（2.3b 分片迁移专用）。自带 `--selftest` 正反对照。

**为什么固化它**：迁移的正确性判据是"**除适配行外，其余逐行逐字未改**"。
用 `diff`＋管道临时拼出来的判据**连续出错 4 次**，每次产生假绿或假红：
  * 过滤管道把**空行**也吃掉；`replace("")` 吃掉首个换行；
  * **插入块是多行，却按单行匹配** ⇒ 必然报假红；
  * **文档注释里的 `std::fs`**（"无 std::fs"）被当成真实依赖 ⇒ 假红；
  * 插入行**前缀表不全**（片1 用 `//! 仅作…`、`//! 1.` 编号，不在表内）⇒ 假红。
故把它写成脚本，并把**三类**判据都做进去。

判据（四条）：
  [1] 施加**同一套已登记替换**后，与源文件**逐行逐字相同**（剔除插入行与空行）。
  [2] 实际出现的 `std::…`（**仅代码行，排除注释**）必须全部在 `ALLOWED_SUBS` 内。
  [3] 迁移后**代码区不得出现 `std::`**（去掉注释、去掉 `#[cfg(test)]` 块之后）。
  [4] 识别的插入行数**可枚举、可复核**（打印出来供人核对）。

⚠️ **权威判据仍是裸机编译**：文本判据只是"快速、可读"的旁证；
   若文本判据说干净而 `cargo build --target x86_64-unknown-none` 失败，**以编译器为准**。

用法：
    python coordination/tools/check_migration_fidelity.py <源文件> <迁移后文件>
    python coordination/tools/check_migration_fidelity.py --selftest
"""
from __future__ import annotations

import io
import re
import sys
import tempfile
from pathlib import Path

# 允许的替换：键=原串，值=替换串。**新增替换必须先登记，否则判红**（防"悄悄改语义"）
ALLOWED_SUBS: dict[str, str] = {
    "std::f32::consts::PI": "core::f32::consts::PI",
    "std::f32::consts::FRAC_PI_2": "core::f32::consts::FRAC_PI_2",
    "std::f32::consts::TAU": "core::f32::consts::TAU",
    "std::cmp::Ordering": "core::cmp::Ordering",
    "std::collections::BTreeMap": "alloc::collections::BTreeMap",
    "std::collections::BTreeSet": "alloc::collections::BTreeSet",
    # ⚠️ 唯一一处**类型替换**（非纯路径改写）：no_std 无 HashSet。
    #    成立前提＝该集合**只用 insert/len、从不迭代** ⇒ 逐位等价（见报告 D37）。
    "std::collections::HashSet": "alloc::collections::BTreeSet",
}

MARKER = "//! 【2.3b"
_USE_WHITELIST = (
    "use alloc::",
    "use core::",
    "use crate::fmath::FloatOps;",
    "use crate::fmath::",
    "#[allow(unused_imports)]",
    "// host(std)",
    "extern crate alloc;",
)
_CFG_TEST = re.compile(r"#\[cfg\(test\)\]")


def _line_inserter(lines: list[str]) -> set[int]:
    """返回被判定为「迁移插入行」的**行下标集合**（0-based）。

    规则：自首个 `//! 【2.3b` 标记行起，**其后连续的** `//!` 注释行、以及紧跟其后的
    白名单 `use`/属性行，都算插入；遇到首个"既非 `//!` 又非白名单"的实义行即结束。
    （源文件的模块注释**全部在标记行之前**，故不会被误判。）
    """
    ins: set[int] = set()
    started = False
    for i, raw in enumerate(lines):
        s = raw.strip()
        if not started:
            if s.startswith(MARKER):
                started = True
                ins.add(i)
            continue
        if s.startswith("//!"):
            ins.add(i)
            continue
        if s.startswith(_USE_WHITELIST):
            ins.add(i)
            continue
        if not s:  # 空行：既不算插入（会被单独过滤），也不结束插入区
            continue
        break
    return ins


def strip_test_blocks(txt: str) -> str:
    """剔除 `#[cfg(test)]` 起的整块（花括号配对），返回非测试正文。"""
    spans: list[tuple[int, int]] = []
    for m in _CFG_TEST.finditer(txt):
        j = txt.find("{", m.end())
        if j < 0:
            continue
        depth, k = 0, j
        while k < len(txt):
            if txt[k] == "{":
                depth += 1
            elif txt[k] == "}":
                depth -= 1
                if depth == 0:
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


def code_only(txt: str) -> str:
    """只留**代码行**：丢掉整行注释（`//`／`//!`／`///`）。

    ⚠️ 局限：不处理块注释 `/* */` 与字符串里的 `//`。故 **[3] 只是旁证**，
    **权威判据是裸机编译**（真有 `std` 依赖则 `x86_64-unknown-none` 编不过）。
    """
    return "\n".join(l for l in txt.split("\n") if not l.strip().startswith("//"))


def is_insertable(line: str) -> bool:
    """该行是否**允许作为迁移新增行**出现（插入的说明行 / 补的 use）。"""
    s = line.strip()
    return s.startswith(_USE_WHITELIST) or s.startswith("//!")


def effective(txt: str) -> list[str]:
    """有效行＝非空行（用于多重集比较；不做"插入行"猜测）。"""
    return [l.rstrip() for l in txt.split("\n") if l.strip()]


def check(orig_path: Path, new_path: Path) -> int:
    """判据：**源文件每一行必须原样出现在新文件中（含重数）**；新文件多出的每一行
    必须是**登记的插入行**。此法可同时抓出"静默丢行"与"未登记改写"。

    ⚠️ 前一版的写法（"先猜哪些是插入行、剔除后逐行比对"）**连续假红两次**：
      插入行白名单含 `use core::`，而源文件里 `use std::f32::consts::PI;`
      **经替换后正好变成** `use core::f32::consts::PI;` ⇒ 被判为"我插入的行"删掉。
      ⇒ 教训：**"猜插入行"这条路不可靠；改成"多重集差"就不需要猜**。
    """
    from collections import Counter

    orig = io.open(orig_path, encoding="utf-8").read()
    new = io.open(new_path, encoding="utf-8").read()
    print(f"=== 保真度判据 ===\n  源文件 = {orig_path}\n  迁移后 = {new_path}")

    # ——— [1] 多重集差：源行不许丢，新增行必须登记 ———
    expected = orig
    applied: list[str] = []
    for a, b in ALLOWED_SUBS.items():
        if a in expected:
            applied.append(f"{a.split('::')[-1]}×{expected.count(a)}")
            expected = expected.replace(a, b)
    src_c, new_c = Counter(effective(expected)), Counter(effective(new))
    missing = src_c - new_c          # 源有、新无 ⇒ **丢了内容**
    extra = new_c - src_c            # 新有、源无 ⇒ 必须是登记插入行
    unregistered = sorted(l for l in extra.elements() if not is_insertable(l))
    ok1 = (not missing) and (not unregistered)
    print(f"\n[1] 多重集差：替换={applied or '（无）'}｜源有效行={sum(src_c.values())} 新有效行={sum(new_c.values())}")
    print(f"    丢失行 = {sum(missing.values())}（必须 0）｜未登记新增行 = {len(unregistered)}（必须 0）"
          f" ⇒ {'✅ 通过' if ok1 else '❌ 不通过'}")
    if missing:
        print("    ❌ 丢失的行：")
        for l, n in list(missing.items())[:10]:
            print(f"        ×{n}  {l[:120]}")
    if unregistered:
        print("    ❌ 未登记的新增行：")
        for l in unregistered[:10]:
            print(f"        {l[:120]}")

    # ——— [2] 替换白名单（仅代码行）———
    found = sorted({m.group(0) for m in re.finditer(r"std::[a-z_0-9:]+", code_only(strip_test_blocks(new)))})
    bad = [s for s in found if s not in ALLOWED_SUBS]
    print(f"\n[2] 替换白名单：代码区 `std::` 残留={found or '（无）'}")

    # ——— [3] 代码区不得有未登记的 std:: ———
    ok3 = not bad
    print(f"[3] 代码区 std 残留：{'✅ 无（或全在白名单内）' if ok3 else '❌ ' + str(bad)}")

    # ——— [4] 登记插入行可枚举（供人工复核）———
    ex_lines = [l for l in extra.elements()]
    print(f"\n[4] 登记插入行 = {len(ex_lines)} 行（供人工复核）：")
    for l in ex_lines[:12]:
        print(f"        {l[:110]}")

    ok = ok1 and ok3
    print(f"\n结果：{'✅ 通过（语义中性）' if ok else '❌ 不通过'}")
    return 0 if ok else 1


def selftest() -> int:
    """正反对照：正常迁移应通过；未登记的替换应被抓到。"""
    print("=== 判据自检（正反两侧）===")
    d = Path(tempfile.mkdtemp(prefix="fid_"))
    src = d / "orig.rs"
    src.write_text(
        "//! 模块说明。\n//!\n//! 提到 std::fs 只是为了说明（注释不算依赖）。\n\n"
        "pub fn f(x: f32) -> f32 { x.abs() }\n"
        "#[cfg(test)]\nmod t { #[test] fn a() { use std::collections::HashMap; let _ = HashMap::<u8,u8>::new(); } }\n",
        encoding="utf-8",
    )
    good = d / "good.rs"
    good.write_text(
        "//! 模块说明。\n//!\n//! 提到 std::fs 只是为了说明（注释不算依赖）。\n\n"
        "//! 【2.3b 片X 迁移】同源，仅作适配：\n"
        "//! ① 补 use\n"
        "#[allow(unused_imports)]\n"
        "use crate::fmath::FloatOps;\n\n"
        "pub fn f(x: f32) -> f32 { x.abs() }\n"
        "#[cfg(test)]\nmod t { #[test] fn a() { use std::collections::HashMap; let _ = HashMap::<u8,u8>::new(); } }\n",
        encoding="utf-8",
    )
    bad = d / "bad.rs"
    bad.write_text(
        "//! 模块说明。\n//!\n//! 提到 std::fs 只是为了说明（注释不算依赖）。\n\n"
        "//! 【2.3b 片X 迁移】同源，仅作适配：\n\n"
        "pub fn f(x: f32) -> f32 { x.abs() }\n"
        "pub fn g() -> usize { std::collections::HashMap::<u8,u8>::new().len() }\n",
        encoding="utf-8",
    )
    print("\n--- ① 正常迁移（应 ✅ 通过）---")
    r1 = check(src, good)
    print("\n--- ② 未登记替换 std::collections::HashMap（应 ❌ 被抓）---")
    r2 = check(src, bad)
    print(f"\n自检结论：{'✅ 两侧都符合预期' if (r1 == 0 and r2 == 1) else '❌ 判据失效'}"
          f"（正常={r1} 应为0；异常={r2} 应为1）")
    return 0 if (r1 == 0 and r2 == 1) else 1


if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] == "--selftest":
        raise SystemExit(selftest())
    if len(sys.argv) != 3:
        print(__doc__)
        raise SystemExit(64)
    raise SystemExit(check(Path(sys.argv[1]), Path(sys.argv[2])))
