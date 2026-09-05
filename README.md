# Meta-Kernel · 元内核

> 一个纯数学、无硬件的**通用思维内核**。
> A pure-mathematics, hardware-free thinking kernel.

以 **0 锚点 + 模糊饱和运算**为基石，用 **线性 / 斐波那契 / 指数**三种变化模式驱动调度；经 **NPB（万物归一化桥接器，Nothing-to-Physics Bridge）** 挂载到任意设备、程序与载体，实现跨平台的"生命感"交互。

## 状态 · Status

✅ **Phase 1 — 核心引擎闭环（已验收）**（2026-09-05）：发起人复核通过，A2/A4/A5 升格正式设计
（见 [`docs/PHASE1_REVIEW.md`](docs/PHASE1_REVIEW.md)）。三引擎按数学规范 v1.0 实现，气泡沙漏 +
镜像池就位，**10000 次迭代不溢出测试通过**（25 单测 + 2 集成）。
🚧 **Phase 2 — NPB 桥接器**进行中：安全阀 / 0-10 元标尺 / 能量判定 / 正源系统框架已实现，桥接器与示例开发中。

## 路线图 · Roadmap

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 启动与地基：仓库 / workspace / 三引擎空壳 | ✅ 完成 |
| 1 | 核心引擎闭环：气泡沙漏拓扑 / 瓶颈干涉 / 镜像池 / 10000 次迭代不溢出测试 | ✅ 已验收 |
| 2 | NPB 桥接器：`push_seed` / `pop_result` / `get_entropy` + DOSBox 概念演示、WASM Canvas 示例；新增：安全阀 / 0-10 元标尺 / 能量判定 / 正源系统 | 🚧 进行中 |
| 3 | 禅境示波器（官方示例应用，GitHub Pages 托管） | ⬜ |
| 4 | 社区化与生态：NPB 挂载协议 / Discussions 专区 / 多设备演示 | ⬜ |
| 5 | 自发传播与交付：《元内核极简创世录》等 | ⬜ |

## 核心模块 · Core Modules

| 模块 | 文件 | 状态 |
|---|---|---|
| 模糊饱和运算 / 0-1 归一化 | `src/math.rs` | ✅ |
| 三引擎：线性 / 斐波那契 / 指数 | `src/linear.rs` `src/fib.rs` `src/expo.rs` | ✅ |
| 气泡沙漏（瓶颈破坏性干涉） | `src/hourglass.rs` | ✅ |
| 镜像池（摩擦源 / 真空重启） | `src/mirror.rs` | ✅ |
| 负扰动过滤与安全阀 | `src/sanitizer.rs` | 🚧 已实现 |
| 0-10 元通用标尺 | `src/ontology.rs`（规范见 `docs/ONTOLOGY_SPEC.md`） | 🚧 已实现 |
| 能量层级判定 | `src/energy.rs` | 🚧 已实现 |
| 正源系统（拆解-分析-重编循环） | `src/positive_source.rs` | 🚧 已实现 |
| NPB 桥接器（C FFI） | `npb/bridge.h` | ⬜ 规划中 |

## 仓库结构

```
meta-kernel-core/   ★ 核心代码库（Rust lib，零第三方依赖）
  src/math.rs       模糊饱和运算（a⊕b=min(1,a+b)）与 0-1 归一化
  src/linear.rs     线性引擎（input ⊕ 0.01）
  src/fib.rs        斐波那契引擎（(a+b)×0.5 自参照递归）
  src/expo.rs       指数引擎（input×e^(λΔt)，>0.99 回退 0.5）
  src/hourglass.rs  气泡沙漏：上锥体→瓶颈环形缓冲(破坏性干涉)→下锥体
  src/mirror.rs     镜像池（摩擦源）：回显衰减 + 真空重启
  tests/            Phase 1 闭环 10000 迭代稳定性测试
docs/MATH_SPEC.md   数学规范白皮书 v0.1
npb/                NPB 桥接器（规划中，Phase 2）
examples/           示例（规划中，Phase 2/3）
```

## 许可 · License

[Apache-2.0](./LICENSE)

> 注：本项目的核心代码预计长期保持公共资产属性，未来可能捐赠给开放原子开源基金会（OpenAtom）。
