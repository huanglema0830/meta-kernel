# L5 设计：应用框架 · 审计层（号脉能力）DESIGN v1.0

> 状态：**v1.0（2026-09-09）** ｜ 版本号：设计完成提交 → **v0.078**（git 提交计数自动）
> 依据：发起人指令（基于 L4 已定型且实测定型通过：6 用例 7/7 全过，`docs/L4_VALIDATION_REPORT.md`）
> 定位：**L5 = 云内核真正执行诊断、生成结论的层级（云操作系统 · 审计层）**；输出供给 L6 空海浏览器。
> 前置固化：场域状态向量 `S = (t, f, a, φ, x, H, τ)` 为 L4→L5 传递接口；版本号 = `git rev-list --count HEAD`。

## 1. 设计基座

| 固化项 | 内容 |
|---|---|
| L4 判定引擎 | `check_state` 纯函数（N_high≥3 → 不非时食；N_low≥3 → 不捉持；否则通过）已实测定型 |
| 传递接口 | `S`（7 维 Vec<f64> / FieldState）+ 基线 + L4 判定结论（Pass / RejectReason） |
| L4→L5 预留 | `l4_router::L5Payload { state: Vec<f64>, extra: Option<Vec<f64>> }`（本设计正式消费 state 并定义 extra=判定结论载体） |
| 阈值 | GOLDEN_LOW 0.618… / GOLDEN_HIGH 1.618… |

## 2. 功能模块划分

| 模块（规划落点 meta-kernel-core/src/l5/） | 职责 | 输入 → 输出 |
|---|---|---|
| `l5_senses.rs` 四诊分析 | **望闻问切**四角度的场域数据分析 | S+基线 → 体征观察集 |
| `l5_trace.rs` 痕迹对比（只读） | 与基线对比生成偏差模式；**痕迹不存储**（读基线+当前，即算即弃） | S+基线 → DeviationPattern |
| `l5_diagnosis.rs` 结论生成 | 偏差模式 → 可读诊断结论（标题/描述/步骤） | Pattern+体征 → Diagnosis |
| `l5_router.rs` 审计出口 | 结论序列化（JSON 子集，零依赖）供 L6；**不缓存不持有** | Diagnosis → L6 契约 JSON |

### 望闻问切 → 维度映射与体征定义

| 四诊 | 探测角度 | 维度 | 体征（偏离倍率带 → 语义） |
|---|---|---|---|
| **望** | 空间分布与形态 | 空间 x、拓扑 τ | x↓<0.618 分布收缩 / x↑>1.618 弥散；τ↓ 结构松散断连 / τ↑ 过密耦结 |
| **闻** | 振动与频率特征 | 频率 f、时间 t | f↑ 高频抖动（忙碌/毛刺）、f↓ 迟滞；t↑ 时序漂移/延迟累积、t↓ 过快变化 |
| **问** | 扰动-响应分析 | 相位 φ、时间 t | φ<0.618 失同步（部件各行其是）、φ↑ 过度同步（共振僵化）；结合 t 判响应延迟 |
| **切** | 电脉冲与动态 | 幅度 a、熵 H、时间 t | a<0.618 活性枯弱 / a>1.618 能量过载；H↑ 混乱失控 / H↓ 死寂凝滞；t 定脉冲节奏 |

体征统一为带内语言（健康带 [0.618, 1.618]）：**平（正常）/ 亢（高亢越界）/ 枯（枯弱越界）** × 维度语义 → 可读观察句。四诊**统一由判定链处理，不分开调用**（继承 L4 口径）。

## 3. 数据流（L3 → L4 → L5 → L6）

```
L3 采集/探针源 ── S 当前向量(7) ──▶ L4 check_state
                                      │ 通过 → S + L4 判定(Pass) ──▶ L5
                                      │ 拒绝 → 丢弃（L4 不缓存）    │
                                      │                            ├─ 四诊分析(senses)
                                      │                            ├─ 痕迹对比只读(trace→pattern)
                                      │                            ├─ 诊断结论(diagnosis)
                                      │                            └─ 序列化 → L6（预留）
                             基线(健康态向量) 由调用方提供/加载（只读常量）
```

- **通过路径**：L4 Pass → L5 生成 Diagnosis（含审计语义）；L4 拒绝路径不进入 L5 号脉（被拒即弃，符合"不符合节律的自行消散"）。
- L5 诊断**即时生成、不缓存、不持有**；调用方（L6/CLI）拿到即展示，L5 自身零状态。

## 4. 接口定义

### 4.1 L4 → L5 契约（消费既有 L5Payload）

```rust
// l4_router::L5Payload 正式化：
pub struct L5Payload {
    pub state: Vec<f64>,          // 通过戒律的 7 维当前向量
    pub extra: Option<L4Verdict>, // extra 载体：L4 判定结论
}
pub enum L4Verdict { Pass }       // 拒绝路径不产生 L5Payload（l4_router 已实现：Err 即弃）
```

### 4.2 L5 内部纯函数入口（规划签名，实测定型后定）

```rust
pub fn diagnose(state: &[f64; 7], baseline: &[f64; 7]) -> Diagnosis;
// 过程：四诊体征(senses) + 偏差模式(trace) → 结论(diagnosis)；无 IO、无存储、无副作用。
// 说明：L4 判定 Pass 才进入；L5 内不再重复判定（审计层只解释，不重判——戒律边界在 L4）。
```

### 4.3 诊断结论数据结构（L5 → L6 契约草案）

```rust
pub struct Diagnosis {
    pub schema: u8,             // 契约版本 = 1
    pub intake: Option<String>, // 触发输入（用户/CLI 提供的描述，可为空；原文零修改）
    pub s: [f64; 7],            // 号脉时的场域向量（可溯源）
    pub pattern: [Band; 7],     // 偏差模式：每维 Band = Low/Ok/High（<0.618 / 中 / >1.618）
    pub senses: Senses,         // 望闻问切体征观察（见 §4.4）
    pub conclusion: Conclusion, // 结论：标题 + 描述 + 排查步骤
    pub source: String,         // 结论来源：builtin / external:<url> / future
}
pub struct Senses { pub look: String, pub listen: String, pub ask: String, pub feel: String }
pub struct Conclusion { pub title: String, pub description: String, pub steps: Vec<String> }
```

**JSON 契约草案**（手写序列化，零依赖，供 L6 空海浏览器 / CLI / 诊断链路消费）：

```json
{
  "schema": 1,
  "intake": "按键盘时部分按键没有反应，其他按键正常",
  "s": [1.0, 1.0, 0.5, 1.0, 0.5, 0.5, 1.0],
  "pattern": ["Ok","Ok","Low","Ok","Low","Low","Ok"],
  "senses": {
    "look":   "空间分布收缩(x)，连接结构平(τ)——分布形态向局部聚拢",
    "listen": "频率平稳(f)，时序平(t)——无抖动",
    "ask":    "相位平(φ)——部件协同正常",
    "feel":   "活性枯弱(a)，熵低(H)——核心动态衰减"
  },
  "conclusion": {
    "title": "场域枯竭（不捉持向）",
    "description": "能量、分布与活性三维同时低于节律下限……",
    "steps": ["注入 0.6–0.9 活性扰动 ×3，观察恢复", "…"]
  },
  "source": "builtin"
}
```

### 4.4 L5 → L6 交互（预留）

- L6 空海浏览器消费 `Diagnosis` JSON 展示（现有「设备诊断」卡可接——诊断结论区）。
- CLI/诊断链路 `manifest-journal diagnose` 输出同构结构（现有 external/builtin steps 并入 conclusion.steps）。
- 契约稳定后方接入 cloud-probe 上报（见 §5）。

## 5. cloud-probe 数据格式映射（L5 设计直接决定 · 草案）

探针采集原始系统量 → 归一为七维当前值（相对健康基线倍率），**映射表（实现探针时据此填充）**：

| 维度 | 采集源（Windows 一期） | 归一法（草案） |
|---|---|---|
| t 时间 | 采样窗口内 CPU 占用率 / 响应抖动 | rate ≈ 占用率×系数；基线 1.0=空闲 30% 参考 |
| f 频率 | 上下文切换/调度抖动（GetSystemTimes 差分） | 抖动比 = 当前窗口均值 / 基线均值 |
| a 幅度 | 内存占用（GlobalMemoryStatusEx） | mem 使用比 / 基线 |
| φ 相位 | 关键服务/线程响应延迟偏差 | 延迟比：中位延迟 / 基线延迟 |
| x 空间 | 磁盘剩余分布（GetDiskFreeSpaceExW） | 余量比：余量 / 总容量（低于阈值 → <0.618） |
| H 熵 | 系统事件日志计数近似 | 事件率归一（过高 → >1.618 混乱；过低 → 死寂 <0.618） |
| τ 拓扑 | 进程/句柄/连接数 | 数量比 / 基线 |

- 探针输出 = `{ s: [7 值], baseline: [7 值], ts }` POST `/v1/probe`（阶段二实现）；
- L5/L4 判定链直接消费该结构——**接口先于探针固化**。

## 6. 验证用例设计（后续实测定型参考）

接续 L4 六用例，输入同型向量，期望**诊断结论**（Pattern/Senses/Conclusion 三层断言）：

| 用例 | 向量特征（同 L4） | L4 判定 | 期望诊断结论 |
|---|---|---|---|
| P1 空闲平稳 | a 单维低 | Pass | 结论"平稳运行"；四诊基本为平 |
| P2 高负载 | t/f/a 高 ×3 | 不非时食（拒绝） | 不进入 L5（审计只解释 Pass）→ 验证：拒绝路径无诊断 |
| P3 卡顿凝滞 | t/f/H 高+φ 低 | 不非时食（拒绝） | 同上（拒绝无诊断） |
| P4 枯竭 | a/x/H 低 ×3 | 不捉持（拒绝） | 同上 |
| P5 混合通过 | 2 高 2 低 | Pass | 结论"混合偏移"：按高/低带描述（亢：t/f；枯：x/H） |
| P6 边界 | 恰黄金阈值 | Pass | 结论"临界通过"：边界带标记 Ok |
| P7 只读/无缓存 | 任意 Pass 向量 | — | 重复 diagnose 5 次结果恒定；痕迹不存储（无状态泄漏：调用后内存无新增持有） |

验证方法（实测定型时）：`diagnose` 纯函数断言 + 报告 `docs/L5_VALIDATION_REPORT.md`（沿用 L4 报告模板）。

## 7. 设计原则落实

1. **戒律是内在边界，不是外部规则**：L5 不设外部白名单/黑名单——结论完全由场域自身状态（七维）推导；
2. **诊断结论不缓存、不持有**（继承 L4 不捉持）：`diagnose` 即时生成，L5 零状态；
3. **痕迹对比执行，但痕迹不存储（只读）**：`l5_trace` 只读基线常量与当前向量，即算即弃；
4. **审计不重判**：L4 是戒律边界（通过/拒绝），L5 只解释通过者——分工单一。
5. 零第三方依赖（手写 JSON 子集序列化）；native + wasm 双目标可编（纯数学/字符串）。

## 8. 落点与实现顺序（后续按指令推进）

1. `docs/L5_DESIGN.md` 定稿（本文件）→ 提交 = **v0.078**；
2. 实现 `meta-kernel-core/src/l5/`（senses → trace → diagnosis → router 序列化）+ 单测；
3. 实测定型（§6 用例 → `docs/L5_VALIDATION_REPORT.md`）；
4. cloud-probe 按 §5 映射表实现并真实采集 → L4/L5 链二次验证 → 老笔记本续测。

## 9. 版本记录

- v0.078（2026-09-09）：L5 设计定稿（本文件，提交 #78）。
