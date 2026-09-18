#!/usr/bin/env python3
"""check_source_of_truth.py —— 源一致性判据（机制 22 / D53，2026-09-18）

干什么：防止「我读的是自己写的脚手架，不是用户的底图」再次发生。
它是机器跑的，不靠我自觉。

判据：
  A. CHARTER.md  必须出现底图硬指针（同时含 `README.md` 与 `docs/`）
  B. MEMORY.md   必须出现同一硬指针（CI 模式下该文件不在仓库内 ⇒ 降级为 warn）
  C. CHARTER.md  必须声明「与底图冲突时以底图为准」
  D. 核心底图集（7 份）在 CHARTER ∪ MEMORY 中全部出现
  E. README.md 的四阶段（阶段一~四）与 ROADMAP.md 的阶段口径必须有显式映射声明
  F. 底图哈希基线：`--update-baseline` 生成；内容变动或文件消失 ⇒ fail（不得静默改底图）

用法：
  python check_source_of_truth.py                      本机检查
  python check_source_of_truth.py --ci                 CI 模式（MEMORY 缺失只 warn）
  python check_source_of_truth.py --update-baseline    重新生成哈希基线（须人工确认）
  python check_source_of_truth.py --selftest           门禁自检（阳性 1 + 阴性 2）
"""

import argparse
import hashlib
import pathlib
import sys
import tempfile

# 核心底图集（**7 份**）。
# ⚠️ **R61 口径统一（2026-09-18 裁定）**：本表原为 **6 份**（漏 `LAYER_BASEMAP_L0_L6.md`），
#    而 `CHARTER.md` 机制 22 行**逐份列了 7 份** ⇒ **同一机制两处口径不符**。
#    裁定：**统一为 7 份**（以 CHARTER 为准）⇒ 本节补入第 7 份，docstring 同步改 7。
#    判据 D 从此**也要求** `LAYER_BASEMAP_L0_L6.md` 在 CHARTER ∪ MEMORY 中出现。
CORE = [
    "README.md",
    "VISION.md",
    "LAYER_ARCHITECTURE.md",
    "MATH_SPEC.md",
    "GENE_LIBRARY_DESIGN.md",
    "COSMIC_COMPUTING.md",
    "LAYER_BASEMAP_L0_L6.md",
]

MAP_KEYWORDS = ("阶段路线（主线）", "四阶段", "底图四阶段", "底图阶段")


def find_repo():
    # coordination/tools/x.py -> coordination -> repo root
    return pathlib.Path(__file__).resolve().parents[2]


def read(p):
    p = pathlib.Path(p)
    return p.read_text(encoding="utf-8", errors="ignore") if p.exists() else ""


def _check(repo, memory_path, ci=False):
    errs, warns = [], []
    repo = pathlib.Path(repo)
    charter = read(repo / "coordination" / "CHARTER.md")
    roadmap = read(repo / "coordination" / "ROADMAP.md")
    readme = read(repo / "README.md")
    memory = read(memory_path) if memory_path else ""

    # A / B 硬指针
    if not charter:
        errs.append("[CHARTER.md] 文件不存在或为空")
    elif "README.md" not in charter or "docs/" not in charter:
        errs.append("[CHARTER.md] 缺底图硬指针（须同时出现 `README.md` 与 `docs/`）")

    if not memory:
        msg = "[MEMORY.md] 未找到（本机路径在仓库外）"
        (warns if ci else errs).append(msg)
    elif "README.md" not in memory or "docs/" not in memory:
        errs.append("[MEMORY.md] 缺底图硬指针（须同时出现 `README.md` 与 `docs/`）")

    # C 冲突以底图为准
    if not (("冲突" in charter) and ("底图" in charter) and ("为准" in charter)):
        errs.append("[CHARTER.md] 未声明「与底图冲突时以底图为准」")

    # D 核心底图集
    blob = charter + "\n" + memory
    miss = [c for c in CORE if c not in blob]
    if miss:
        errs.append("[CHARTER∪MEMORY] 核心底图未出现：%s" % miss)

    # E 阶段口径映射
    if not all(k in readme for k in ("阶段一", "阶段二", "阶段三", "阶段四")):
        errs.append("[README.md] 未找到四阶段（阶段一~阶段四）")
    if not any(k in roadmap for k in MAP_KEYWORDS):
        errs.append("[ROADMAP.md] 未声明与底图（README）阶段口径的映射")

    # F 哈希基线
    base = repo / "coordination" / "security" / "basemap_hashes.txt"
    docs = sorted((repo / "docs").glob("*.md"))
    hashes = {d.name: hashlib.sha256(d.read_bytes()).hexdigest() for d in docs}
    if not base.exists():
        warns.append("底图哈希基线不存在（%s），用 --update-baseline 生成" % base.name)
    else:
        old = {}
        for line in base.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "  " in line:
                h, n = line.split("  ", 1)
                old[n.strip()] = h.strip()
        for n, h in hashes.items():
            if n not in old:
                warns.append("[基线] 新增底图未登记：%s" % n)
            elif old[n] != h:
                errs.append("[基线] 底图内容已变：%s（须 --update-baseline 显式确认）" % n)
        for n in old:
            if n not in hashes:
                errs.append("[基线] 底图文件消失：%s（不得静默删除）" % n)

    return errs, warns, hashes


def update_baseline(repo, hashes):
    repo = pathlib.Path(repo)
    base = repo / "coordination" / "security" / "basemap_hashes.txt"
    base.parent.mkdir(parents=True, exist_ok=True)
    lines = ["# 底图哈希基线（SHA-256）· 由 check_source_of_truth.py --update-baseline 生成",
             "# 改动底图内容后须重新生成，并说明改动理由；文件消失一律 fail。", ""]
    for n in sorted(hashes):
        lines.append("%s  %s" % (hashes[n], n))
    base.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("基线已写入：%s（%d 份）" % (base, len(hashes)))


def selftest():
    """门禁自检：阳性对照 1 个（应 pass）＋ 阴性对照 2 个（应 fail）"""
    with tempfile.TemporaryDirectory() as td:
        repo = pathlib.Path(td)
        (repo / "docs").mkdir()
        (repo / "coordination" / "security").mkdir(parents=True)
        (repo / "coordination" / "tools").mkdir(parents=True)

        (repo / "README.md").write_text("阶段一 阶段二 阶段三 阶段四", encoding="utf-8")
        for c in CORE[1:]:
            (repo / "docs" / c).write_text("x", encoding="utf-8")

        charter = repo / "coordination" / "CHARTER.md"
        # ⚠️ 夹具必须与 CORE **逐份同步**（R61 口径统一后 CORE 由 6 份升为 7 份）：
        #    漏一份 ⇒ 阳性对照会假红。这是"**改白名单必须同步夹具**"（R34 精神）在自检上的落点。
        good_charter = ("底图 = README.md + docs/ ；冲突以底图为准；"
                        "VISION.md LAYER_ARCHITECTURE.md MATH_SPEC.md "
                        "GENE_LIBRARY_DESIGN.md COSMIC_COMPUTING.md LAYER_BASEMAP_L0_L6.md")
        charter.write_text(good_charter, encoding="utf-8")
        mem = repo / "MEMORY.md"
        mem.write_text("先读底图 README.md + docs/", encoding="utf-8")
        (repo / "coordination" / "ROADMAP.md").write_text("阶段路线（主线）", encoding="utf-8")

        # 阳性对照：全部合规 ⇒ 应无 error
        errs, _, _ = _check(repo, mem)
        assert not errs, "阳性对照应通过，实际报错：%s" % errs
        print("  阳性对照 PASS（合规样例无 error）")

        # 阴性 1：CHARTER 去掉 docs/ 指针 ⇒ 应 fail
        charter.write_text(good_charter.replace("docs/", ""), encoding="utf-8")
        e1, _, _ = _check(repo, mem)
        assert e1, "阴性对照 1 应失败（CHARTER 缺底图指针）"
        print("  阴性对照1 PASS（去掉 docs/ 指针 ⇒ 判红）")
        charter.write_text(good_charter, encoding="utf-8")

        # 阴性 2：ROADMAP 去掉阶段映射 ⇒ 应 fail
        (repo / "coordination" / "ROADMAP.md").write_text("无映射声明", encoding="utf-8")
        e2, _, _ = _check(repo, mem)
        assert e2, "阴性对照 2 应失败（ROADMAP 缺阶段映射）"
        print("  阴性对照2 PASS（去掉阶段映射 ⇒ 判红）")
        (repo / "coordination" / "ROADMAP.md").write_text("阶段路线（主线）", encoding="utf-8")

        # 阳性对照 2（**CI 场景**）：MEMORY 在仓库外、CI 上不存在 ⇒ 仅凭合规 CHARTER 仍应 PASS。
        # 为什么必须有这一条：2026-09-18 v0.191 CI 首次判红——判据 D 查 CHARTER∪MEMORY，
        # 而 CI 上没有 MEMORY，CHARTER 又只写了 `docs/` 泛指、没列核心底图文件名 ⇒ 本机绿、CI 红。
        # 教训：**"本机绿"不等于"CI 绿"**；凡是依赖仓库外文件的判据，必须有 CI 场景的对照。
        e3, w3, _ = _check(repo, str(repo / "NO_SUCH_MEMORY.md"), ci=True)
        assert not e3, "阳性对照 2 应通过（CI 场景：无 MEMORY 但 CHARTER 合规），实际：%s" % e3
        assert w3, "阳性对照 2 应产生 MEMORY 缺失 warn"
        print("  阳性对照2 PASS（CI 场景：无 MEMORY 仍 PASS，仅 warn）")

        # 阴性 3（**CI 场景**）：CHARTER 只写 `docs/` 泛指、不列核心底图文件名 ⇒ 应 fail
        vague = "底图 = README.md + docs/ ；冲突以底图为准"
        charter.write_text(vague, encoding="utf-8")
        e4, _, _ = _check(repo, str(repo / "NO_SUCH_MEMORY.md"), ci=True)
        assert e4, "阴性对照 3 应失败（CI 场景：CHARTER 无核心底图名单 ⇒ 判据 D 判红）"
        print("  阴性对照3 PASS（CI 场景：CHARTER 无核心底图名单 ⇒ 判红）")
        charter.write_text(good_charter, encoding="utf-8")

    print("selftest: PASS（阳性 2 ＋ 阴性 3）")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=str(find_repo()))
    ap.add_argument("--memory", default=None, help="仓库外 MEMORY.md 路径")
    ap.add_argument("--ci", action="store_true", help="CI 模式：MEMORY 缺失只 warn")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()

    if a.selftest:
        selftest()
        return 0

    repo = pathlib.Path(a.repo)
    memory_path = a.memory
    if memory_path is None:
        # 默认：研发开发/.workbuddy/memory/MEMORY.md（仓库根往上两级）
        guess = repo.parents[0] / ".workbuddy" / "memory" / "MEMORY.md"
        if not guess.exists():
            guess = repo / ".." / ".." / ".workbuddy" / "memory" / "MEMORY.md"
        memory_path = str(guess)

    errs, warns, hashes = _check(repo, memory_path, ci=a.ci)

    if a.update_baseline:
        update_baseline(repo, hashes)
        return 0

    print("源一致性判据（D53）· repo=%s" % repo)
    print("  MEMORY: %s%s" % (memory_path, "" if pathlib.Path(memory_path).exists() else "  (不存在)"))
    print("  底图 .md 数：%d" % len(hashes))
    for w in warns:
        print("  WARN  %s" % w)
    for e in errs:
        print("  FAIL  %s" % e)
    if errs:
        print("结果：FAIL（%d 项）" % len(errs))
        return 1
    print("结果：PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
