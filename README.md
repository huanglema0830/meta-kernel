# Meta-Kernel · 元内核

> 一个纯数学、无硬件的**通用思维内核**。
> A pure-mathematics, hardware-free thinking kernel.

以 **0 锚点 + 模糊饱和运算**为基石，用 **线性 / 斐波那契 / 指数**三种变化模式驱动调度；经 **NPB（万物归一化桥接器，Nothing-to-Physics Bridge）** 挂载到任意设备、程序与载体，实现跨平台的"生命感"交互。

## 状态 · Status

🚧 **Phase 0 — 启动与地基**（2026-09 起）：仓库脚手架 + 三引擎空壳已就位；核心算法数学规范整理中。

## 路线图 · Roadmap

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 启动与地基：仓库 / workspace / 三引擎空壳 | 🚧 进行中 |
| 1 | 核心引擎闭环：气泡沙漏拓扑 / 瓶颈干涉 / 镜像池 / 10000 次迭代不溢出测试 | ⬜ |
| 2 | NPB 桥接器：`push_seed(f32)` / `pop_result(f32)` / `get_entropy()` + DOSBox、WASM 示例 | ⬜ |
| 3 | 禅境示波器（官方示例应用，GitHub Pages 托管） | ⬜ |
| 4 | 社区化与生态：NPB 挂载协议 / Discussions 专区 / 多设备演示 | ⬜ |
| 5 | 自发传播与交付：《元内核极简创世录》等 | ⬜ |

## 仓库结构

```
meta-kernel-core/   ★ 核心代码库（Rust lib，零第三方依赖）
  src/linear.rs     线性引擎（空壳）
  src/fib.rs        斐波那契引擎（空壳）
  src/expo.rs       指数引擎（空壳）
npb/                NPB 桥接器（规划中，Phase 2）
examples/           示例（规划中，Phase 2/3）
```

## 许可 · License

[Apache-2.0](./LICENSE)

> 注：本项目的核心代码预计长期保持公共资产属性，未来可能捐赠给开放原子开源基金会（OpenAtom）。
