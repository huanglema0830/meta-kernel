#!/usr/bin/env python3
"""断言 QEMU 截屏（PPM/P6）中的像素颜色 == 期望值。

判定协议见 `kernel/src/verify.rs`：
  整屏 **绿** = 内核自检全过（且证明引导器确实加载并进入了内核入口）
  整屏 **红** = 自检失败
  其它      = 未刷屏（无帧缓冲 / 未进内核 / panic）⇒ **一律判失败**

用法：
  check_screendump.py <shot.ppm> [--expect R,G,B] [--sample-grid N]

退出码：0 = 通过；1 = 不通过；2 = 文件或格式问题（也算失败）
"""

from __future__ import annotations

import argparse
import collections
import sys


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


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("shot")
    ap.add_argument("--expect", default="0,255,0", help="期望 RGB，如 0,255,0")
    ap.add_argument("--sample-grid", type=int, default=5, help="采样网格边长（默认 5×5）")
    args = ap.parse_args()

    expect = tuple(int(x) for x in args.expect.split(","))
    if len(expect) != 3:
        print(f"[FAIL] --expect 需为 R,G,B 三个数，得到 {args.expect}")
        return 1

    try:
        w, h, maxval, px = read_ppm_p6(args.shot)
    except Exception as e:  # noqa: BLE001
        print(f"[FAIL] 无法读取截屏 {args.shot}：{e}")
        return 2

    print(f"[info] 截屏 {w}x{h} maxval={maxval} expect={expect}")

    def at(x: int, y: int) -> tuple[int, int, int]:
        o = (y * w + x) * 3
        return px[o], px[o + 1], px[o + 2]

    # 采样网格：10%..90%（避开最外圈，防止边缘伪影）
    n = max(1, args.sample_grid)
    xs = [int(w * (i + 1) / (n + 1)) for i in range(n)]
    ys = [int(h * (i + 1) / (n + 1)) for i in range(n)]

    samples = [(x, y, at(x, y)) for y in ys for x in xs]
    bad = [(x, y, c) for (x, y, c) in samples if c != expect]

    counter = collections.Counter(c for (_, _, c) in samples)
    print(f"[info] 采样 {len(samples)} 点，颜色分布（前 5）：{counter.most_common(5)}")

    if bad:
        print(f"[FAIL] {len(bad)}/{len(samples)} 个采样点不是期望色 {expect}")
        for (x, y, c) in bad[:8]:
            print(f"       ({x},{y}) = {c}")
        print("[hint] 全屏非期望色 ⇒ 自检失败(红) / 未刷屏(引导器画面) / 未进入内核")
        return 1

    print(f"[PASS] 全部 {len(samples)} 个采样点 == {expect} ⇒ 引导成功且内核自检通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
