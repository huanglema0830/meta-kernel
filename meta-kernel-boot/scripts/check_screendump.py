#!/usr/bin/env python3
"""断言 QEMU 截屏（PPM/P6）像素颜色 —— **支持整屏单色 / 区域 / 多色 / 多帧对比**。

判定协议见 `kernel/src/verify.rs`（**每条断言的正反两侧见 --selftest**）：
  整屏 **绿** = 内核自检全过（且证明引导器确实加载并进入了内核入口）
  整屏 **红** = 自检失败
  其它      = 未刷屏（无帧缓冲 / 未进内核 / panic）⇒ **一律判失败**

用法：
  # ① 整屏单色（**原口径，向后兼容**）
  check_screendump.py shot.ppm --expect 0,255,0

  # ② 区域断言（2026-09-19 新增 · 帧缓冲稿 Q6）
  check_screendump.py shot.ppm --expect-region 0,0,64,64=255,0,0 \
                               --expect-region 64,64,64,64=0,0,255

  # ③ 多色白名单（整屏只允许出现这些颜色的组合）
  check_screendump.py shot.ppm --allow-colors 0,255,0 --allow-colors 255,0,0

  # ④ 多帧对比（断言两帧**必须不同** —— 防"呈现写死"）
  check_screendump.py shot2.ppm --compare shot1.ppm --min-diff 20

  # ⑤ 判据自检（正反两侧；**新增，使 CI 可跑**）
  check_screendump.py --selftest

退出码：0 = 通过；1 = 不通过；2 = 文件或格式问题（也算失败）
"""

from __future__ import annotations

import argparse
import collections
import os
import sys
import tempfile


def read_ppm_p6(path: str) -> tuple[int, int, int, bytes]:
    with open(path, "rb") as f:
        data = f.read()
    if not data.startswith(b"P6"):
        raise ValueError(f"不是 P6 格式（前 2 字节 = {data[:2]!r}）")

    idx = 2
    fields: list[int] = []
    while len(fields) < 3:
        # 跳过空白
        while idx < len(data) and data[idx : idx + 1].isspace():
            idx += 1
        # 跳过注释行
        if data[idx : idx + 1] == b"#":
            while idx < len(data) and data[idx : idx + 1] != b"\n":
                idx += 1
            continue
        start = idx
        while idx < len(data) and not data[idx : idx + 1].isspace():
            idx += 1
        if start == idx:
            raise ValueError("PPM 头部解析失败（字段为空）")
        fields.append(int(data[start:idx]))
    idx += 1  # maxval 后的单个空白

    w, h, maxval = fields
    pixels = data[idx:]
    need = w * h * 3
    if len(pixels) < need:
        raise ValueError(f"像素数据不足：need={need} got={len(pixels)}")
    return w, h, maxval, pixels


def parse_rgb(text: str) -> tuple[int, int, int]:
    parts = [int(x) for x in text.split(",")]
    if len(parts) != 3:
        raise ValueError(f"需为 R,G,B 三个数，得到 {text!r}")
    return parts[0], parts[1], parts[2]


def parse_region(text: str) -> tuple[int, int, int, int, tuple[int, int, int]]:
    """`X,Y,W,H=R,G,B`"""
    if "=" not in text:
        raise ValueError(f"--expect-region 需形如 X,Y,W,H=R,G,B，得到 {text!r}")
    lhs, rhs = text.split("=", 1)
    nums = [int(x) for x in lhs.split(",")]
    if len(nums) != 4:
        raise ValueError(f"区域需 X,Y,W,H 四个数，得到 {lhs!r}")
    return nums[0], nums[1], nums[2], nums[3], parse_rgb(rhs)


class Shot:
    def __init__(self, path: str) -> None:
        self.path = path
        self.w, self.h, self.maxval, self.px = read_ppm_p6(path)

    def at(self, x: int, y: int) -> tuple[int, int, int]:
        o = (y * self.w + x) * 3
        return self.px[o], self.px[o + 1], self.px[o + 2]

    def grid(self, n: int) -> list[tuple[int, int, tuple[int, int, int]]]:
        """采样网格：10%..90%（避开最外圈，防止边缘伪影）。"""
        n = max(1, n)
        xs = [int(self.w * (i + 1) / (n + 1)) for i in range(n)]
        ys = [int(self.h * (i + 1) / (n + 1)) for i in range(n)]
        return [(x, y, self.at(x, y)) for y in ys for x in xs]

    def region_grid(
        self, x0: int, y0: int, w: int, h: int, n: int
    ) -> list[tuple[int, int, tuple[int, int, int]]]:
        """区域内部采样（同样避开该区域最外圈）。"""
        n = max(1, n)
        if w <= 0 or h <= 0 or x0 < 0 or y0 < 0 or x0 + w > self.w or y0 + h > self.h:
            raise ValueError(f"区域越界：({x0},{y0},{w},{h}) vs 画面 {self.w}x{self.h}")
        xs = [x0 + int(w * (i + 1) / (n + 1)) for i in range(n)]
        ys = [y0 + int(h * (i + 1) / (n + 1)) for i in range(n)]
        return [(x, y, self.at(x, y)) for y in ys for x in xs]


def run(args: argparse.Namespace) -> int:
    try:
        shot = Shot(args.shot)
        prev = Shot(args.compare) if args.compare else None
    except Exception as e:  # noqa: BLE001
        print(f"[FAIL] 无法读取截屏：{e}")
        return 2

    print(f"[info] 截屏 {shot.w}x{shot.h} maxval={shot.maxval}")
    n = max(1, args.sample_grid)
    fails: list[str] = []

    # ---- ① 整屏单色（原口径） ----
    if args.expect is not None:
        expect = parse_rgb(args.expect)
        samples = shot.grid(n)
        bad = [(x, y, c) for (x, y, c) in samples if c != expect]
        dist = collections.Counter(c for (_, _, c) in samples)
        print(f"[info] 整屏采样 {len(samples)} 点，颜色分布（前 5）：{dist.most_common(5)}")
        if bad:
            fails.append(f"整屏：{len(bad)}/{len(samples)} 个采样点不是 {expect}")
            for (x, y, c) in bad[:8]:
                print(f"       ({x},{y}) = {c}")

    # ---- ② 多色白名单 ----
    if args.allow_colors:
        allowed = {parse_rgb(t) for t in args.allow_colors}
        samples = shot.grid(n)
        bad = [(x, y, c) for (x, y, c) in samples if c not in allowed]
        print(f"[info] 多色采样 {len(samples)} 点，允许色 {sorted(allowed)}")
        if bad:
            fails.append(f"多色：{len(bad)}/{len(samples)} 个采样点不在允许色内")
            for (x, y, c) in bad[:8]:
                print(f"       ({x},{y}) = {c}")

    # ---- ③ 区域断言 ----
    for spec in args.expect_region or []:
        try:
            x0, y0, w, h, color = parse_region(spec)
            samples = shot.region_grid(x0, y0, w, h, n)
        except ValueError as e:
            fails.append(f"区域 {spec}：{e}")
            continue
        bad = [(x, y, c) for (x, y, c) in samples if c != color]
        print(f"[info] 区域 ({x0},{y0},{w},{h}) 采样 {len(samples)} 点，期望 {color}")
        if bad:
            fails.append(f"区域 ({x0},{y0},{w},{h})：{len(bad)}/{len(samples)} 不为 {color}")
            for (x, y, c) in bad[:8]:
                print(f"       ({x},{y}) = {c}")

    # ---- ④ 多帧对比（断言两帧必须不同） ----
    if prev is not None:
        if (prev.w, prev.h) != (shot.w, shot.h):
            fails.append(f"多帧对比：尺寸不同 {prev.w}x{prev.h} vs {shot.w}x{shot.h}")
        else:
            samples = shot.grid(n)
            diff = sum(1 for (x, y, c) in samples if c != prev.at(x, y))
            print(f"[info] 多帧对比：{diff}/{len(samples)} 个采样点不同（要求 ≥ {args.min_diff}）")
            if diff < args.min_diff:
                fails.append(
                    f"多帧对比：仅 {diff} 点不同 < {args.min_diff} ⇒ 两帧几乎相同（呈现可能写死）"
                )

    if fails:
        for f in fails:
            print(f"[FAIL] {f}")
        print("[hint] 全屏非期望色 ⇒ 自检失败(红) / 未刷屏(引导器画面) / 未进入内核")
        return 1

    print("[PASS] 全部断言通过")
    return 0


# ============================ 判据自检（正反两侧） ============================
# 口径：**自检与实跑同源**（C18）—— 自检直接调用 run() 用的同一套解析与判定函数，
#       不另写一份"期望值"。
#       （原脚本的自检是**本地手工验证过**、**未内建**；2026-09-19 补上，使 CI 可跑。）


def _write_ppm(path: str, w: int, h: int, color) -> None:
    with open(path, "wb") as f:
        f.write(b"P6\n%d %d\n255\n" % (w, h))
        f.write(bytes(color) * (w * h))


def _half_ppm(path: str, w: int, h: int, c1, c2) -> None:
    row = bytes(c1) * (w // 2) + bytes(c2) * (w - w // 2)
    with open(path, "wb") as f:
        f.write(b"P6\n%d %d\n255\n" % (w, h))
        for _ in range(h):
            f.write(row)


def selftest() -> int:
    W, H = 60, 60
    ok = 0
    bad = 0
    with tempfile.TemporaryDirectory() as d:
        green = os.path.join(d, "green.ppm")
        red = os.path.join(d, "red.ppm")
        black = os.path.join(d, "black.ppm")
        half = os.path.join(d, "half.ppm")
        blue = os.path.join(d, "blue.ppm")
        _write_ppm(green, W, H, (0, 255, 0))
        _write_ppm(red, W, H, (255, 0, 0))
        _write_ppm(black, W, H, (0, 0, 0))
        _write_ppm(blue, W, H, (0, 0, 255))
        _half_ppm(half, W, H, (0, 255, 0), (0, 0, 0))

        def case(name, expect_code, argv):
            nonlocal ok, bad
            ns = build_parser().parse_args(argv)
            got = run(ns)
            good = got == expect_code
            print(f"[SELFTEST] {name}: 期望 exit={expect_code} 实测={got} {'OK' if good else 'BAD'}")
            if good:
                ok += 1
            else:
                bad += 1

        case("阳性 整屏绿", 0, [green, "--expect", "0,255,0"])
        case("阴性 整屏红", 1, [red, "--expect", "0,255,0"])
        case("阴性 整屏黑", 1, [black, "--expect", "0,255,0"])
        case("阴性 半绿半黑", 1, [half, "--expect", "0,255,0"])
        case("多色 半绿半黑∈{绿,黑}", 0,
             [half, "--allow-colors", "0,255,0", "--allow-colors", "0,0,0"])
        case("多色 半绿半黑∈{绿}", 1, [half, "--allow-colors", "0,255,0"])
        case("区域 绿图取左半=绿", 0, [green, "--expect-region", "0,0,30,60=0,255,0"])
        case("区域 绿图取区域=红", 1, [green, "--expect-region", "0,0,30,60=255,0,0"])
        case("区域 越界不崩且判红", 1, [green, "--expect-region", "50,50,40,40=0,255,0"])
        case("多帧 绿vs蓝 必不同", 0, [blue, "--compare", green, "--min-diff", "20"])
        case("多帧 绿vs绿 应判红", 1, [green, "--compare", green, "--min-diff", "20"])
        case("格式 文件不存在", 2, [os.path.join(d, "nope.ppm"), "--expect", "0,255,0"])

    print(f"[SELFTEST] 合计 通过={ok} 失败={bad}（应为 通过=12 失败=0）")
    return 0 if bad == 0 else 1


def build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser()
    ap.add_argument("shot", nargs="?", help="截屏 PPM 路径（--selftest 时可省略）")
    ap.add_argument("--expect", default=None, help="整屏期望 RGB，如 0,255,0（省略则不查整屏）")
    ap.add_argument("--sample-grid", type=int, default=5, help="采样网格边长（默认 5×5=25 点）")
    ap.add_argument("--expect-region", action="append", default=None,
                    help="区域断言 X,Y,W,H=R,G,B（可重复）")
    ap.add_argument("--allow-colors", action="append", default=None,
                    help="整屏允许色 R,G,B（可重复；与 --expect 择一使用）")
    ap.add_argument("--compare", default=None, help="对比帧 PPM；断言当前帧与之**不同**")
    ap.add_argument("--min-diff", type=int, default=1, help="多帧对比的最少不同采样点数（默认 1）")
    ap.add_argument("--selftest", action="store_true", help="跑判据自检（正反两侧）")
    return ap


def main() -> int:
    args = build_parser().parse_args()
    if args.selftest:
        return selftest()
    if not args.shot:
        print("[FAIL] 缺少 shot 参数（或使用 --selftest）")
        return 2
    if args.expect is None and not args.allow_colors and not args.expect_region:
        print("[FAIL] 至少给出一种断言：--expect / --allow-colors / --expect-region")
        return 2
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
