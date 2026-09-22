#!/usr/bin/env python3
"""判据全跑运行器（**C-2 固化** · **T-048** 的可执行载体）

## 为什么需要（C-2 立条理由 · 逐字）

判据套件此前 **无统一运行器 ＋ 调用姿势不一** ⇒ **「用法错」会伪装成「判据红」**。
实证两例：
  ① `check_source_of_truth.py` 传 `--repo .` ⇒ **rc=1**（本机判红：MEMORY 指针读不到）；
  ② `check_migration_fidelity.py` **不带参数** ⇒ **rc=64**（usage 错）。

## 本运行器做什么

① 按「**各脚本自身**」的正确姿势调用（姿势表 = 下方 `CALLS`，**写死一处**）；
② 逐个打印 **rc ＋ 摘要行**（可读、可核）；
③ 汇总 PASS/FAIL；**任一 rc != 0 ⇒ 整体 FAIL**（rc=1）。

## ★ 边界（如实标注）

- **C18／C19**：本运行器 **不重算任何机器事实** —— 只 **复用** 各判据自身的输出，
  **禁止自写第二份**枚举／计数／解析。故它 **不是** 第 12 个判据，而是 **调度器**。
- `check_migration_fidelity.py` 的**真实**调用须给「源／目标」两个路径（分片专用）；
  在「全跑」语境下 **按 T-048 口径**取 `--selftest` ⇒ **如实记：该项在本运行器内只跑自检**。
- `check_id_set_diff.py` / `check_report_structure.py` 为 **只告警**（rc 恒 0）⇒ 其「告警数」须**另行读输出**，
  不能只看 rc（**R83 同族：空转必须与通过取不同值**）。

## 用法

    python coordination/tools/run_all_checks.py             # 全跑
    python coordination/tools/run_all_checks.py --list      # 只列将执行的命令
    python coordination/tools/run_all_checks.py --selftest  # 运行器自检（正反对照 · 回读真实仓库）
"""
import argparse
import subprocess
import sys
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
REPO = TOOLS.parents[1]

# ── 调用姿势表（★ 唯一出处；改动须同步 T-048 / §11.3） ───────────────────────
# 口径：`[]` = 无参数直跑；带参数者**必须**按各脚本自身要求给（否则 rc 会假红）。
CALLS = [
    ("check_doc_consistency.py", []),
    # ★ v0.328 新增（`T-052` 效力列判据 · 只告警）
    ("check_effect_column.py", []),
    ("check_endpoint_table.py", []),
    ("check_id_set_diff.py", []),
    # ★ v0.329 新增（治 `F-5` 的**批量**治法 · 只告警）
    ("check_judge_gaps.py", []),
    ("check_kernel_purity.py", []),
    ("check_memory_layers.py", []),
    ("check_migration_closure.py", []),
    # ★ 须带参数（源/目标路径，或 --selftest）—— 空调用 ⇒ rc=64
    ("check_migration_fidelity.py", ["--selftest"]),
    # ★ 须 --mode（默认即 syntax，但显式写出以防默认值漂移）
    ("check_private_pointers.py", ["--mode=syntax"]),
    ("check_reading_instruction.py", []),
    ("check_report_structure.py", []),
    # ★ v0.328 新增（`T-051` 五段流水线判据 · 只告警）
    ("check_rewrite_pipeline.py", []),
    # ★ 不得传 `--repo .`（其默认 `find_repo()` 才是对的）—— 传了 ⇒ rc=1 假红
    ("check_source_of_truth.py", []),
    # ★ v0.328 新增（`T-050` 规则⑤ 轮次号判据 · 只告警）
    ("check_version_label.py", []),
]

SUMMARY_RE = ("扫描", "自检结论", "告警", "PASS", "FAIL", "结论", "门禁", "收口")


def expected_files():
    """真实仓库里的判据脚本集合（回读真实目录，不手写第二份清单）。"""
    return sorted(p.name for p in TOOLS.glob("check_*.py"))


def run_one(name, args):
    exe = str(TOOLS / name)
    if not (TOOLS / name).is_file():
        return 127, "[MISSING] 脚本不存在：%s" % name
    p = subprocess.run([sys.executable, exe] + args, cwd=str(REPO),
                       capture_output=True, text=True)
    out = (p.stdout or "").splitlines()
    hits = [l.strip() for l in out if any(k in l for k in SUMMARY_RE)]
    tail = hits[-1][:180] if hits else (out[-1][:180] if out else "(无输出)")
    return p.returncode, tail


def do_list():
    print("将执行（cwd = 仓库根 %s）：" % REPO)
    for name, args in CALLS:
        print("  python coordination/tools/%s %s" % (name, " ".join(args)))
    return 0


def do_selftest():
    """正反对照 · 回读真实仓库（C18 三条）。"""
    ok = True

    # 侧① 真实仓库可解析：tools/ 下 check_*.py 应 >0，且与 CALLS **集合相等**（无漏、无多）
    real = expected_files()
    listed = sorted(n for n, _ in CALLS)
    print("[侧①] 真实仓库 check_*.py = %d 个｜CALLS 覆盖 = %d 个" % (len(real), len(listed)))
    if len(real) > 0 and real == listed:
        print("       ⇒ ✅ 集合相等（无漏判据、无幽灵条目）")
    else:
        ok = False
        print("       ⇒ ❌ 集合不等：只在此处=%s｜只在CALLS=%s"
              % (sorted(set(real) - set(listed)), sorted(set(listed) - set(real))))

    # 侧② 每个 CALLS 条目在真实仓库**存在**
    missing = [n for n, _ in CALLS if not (TOOLS / n).is_file()]
    print("[侧②] CALLS 条目在真实仓库缺失 = %d" % len(missing))
    if missing:
        ok = False

    # 侧③ 姿势断言（正例）：三条易错姿势必须写对
    d = dict(CALLS)
    checks = [
        ("migration_fidelity 带参数", d.get("check_migration_fidelity.py") == ["--selftest"]),
        ("private_pointers 带 --mode", d.get("check_private_pointers.py") == ["--mode=syntax"]),
        ("source_of_truth 不传 --repo", "--repo" not in d.get("check_source_of_truth.py", [])),
    ]
    print("[侧③] 易错姿势正例：")
    for label, good in checks:
        print("       %s %s" % ("✅" if good else "❌", label))
        ok = ok and good

    # 侧④ 反例：**空调用 migration_fidelity** 必须非 0（证明"姿势错 ⇒ rc 非 0"可被本运行器看见）
    rc_bad, _ = run_one("check_migration_fidelity.py", [])
    print("[侧④] 反例（migration_fidelity 空调用）rc = %d ⇒ %s"
          % (rc_bad, "✅ 非 0（姿势错会被抓）" if rc_bad != 0 else "❌ 竟为 0（反例失效）"))
    ok = ok and (rc_bad != 0)

    # 侧⑤ 反例：**不存在的脚本** 必须被本运行器判为 MISSING（127）
    rc_missing, _ = run_one("__no_such_check__.py", [])
    print("[侧⑤] 反例（不存在的脚本）rc = %d ⇒ %s"
          % (rc_missing, "✅ 127" if rc_missing == 127 else "❌ 应为 127"))
    ok = ok and (rc_missing == 127)

    print("\n运行器自检结论 =", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser(description="判据全跑运行器（C-2 固化）")
    ap.add_argument("--list", action="store_true", help="只列将执行的命令")
    ap.add_argument("--selftest", action="store_true", help="运行器自检（正反对照 · 回读真实仓库）")
    a = ap.parse_args()

    if a.selftest:
        return do_selftest()
    if a.list:
        return do_list()

    print("判据全跑（T-048）｜cwd = %s" % REPO)
    print("★ 口径：本运行器只**复用**各判据输出，不重算任何机器事实（C18／C19）\n")
    bad, rows = [], []
    for name, args in CALLS:
        rc, tail = run_one(name, args)
        rows.append((name, rc, tail))
        print("%-34s rc=%-3d | %s" % (name, rc, tail))
        if rc != 0:
            bad.append((name, rc))

    print("\n===== 汇总 =====")
    print("执行 %d 个判据｜PASS(rc=0) = %d｜FAIL = %d"
          % (len(rows), len(rows) - len(bad), len(bad)))
    if bad:
        for n, rc in bad:
            print("  FAIL: %s (rc=%d)" % (n, rc))
    print("全跑结论 =", "FAIL" if bad else "PASS")
    print("★ 提醒：`check_id_set_diff`／`check_report_structure` 为**只告警**（rc 恒 0）"
          "⇒ 其告警数须另行读输出（R83）")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
