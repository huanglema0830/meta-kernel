# 协调文件夹 · 启动引导

每次对话开始，按顺序读以下文件：

1. BASELINE.md — 项目当前状态
2. CONSTRAINTS.md — 硬约束清单
3. TEMPLATES.md — 模板格式
4. ROADMAP.md — 总行动路线图
5. instructions/ 下最新文件 — 当前任务
6. reports/ 下最新文件 — 最近报告

读完再动手。

## 每次报告结束，必须

1. 更新 BASELINE.md（如果版本变了）
2. 把本次报告归档到 `reports/`
3. 把本次指令归档到 `instructions/`（如果是指令触发的）
4. 如有讨论记录，归档到 `discussions/`

## 接续话术（任何一方断裂时用）

> 继续云内核项目。读 coordination/ 下的最新状态。
> 告诉我：当前到哪一步、下一步是什么、需要我决策什么。

## 文件夹职责

| 文件/文件夹 | 职责 | 维护者 |
|---|---|---|
| README.md | 阅读顺序与归档纪律 | 顾问 |
| BASELINE.md | 项目当前状态的唯一权威描述 | WorkBuddy，每版本更新 |
| CONSTRAINTS.md | 硬约束清单（C1–C9） | 顾问定，WorkBuddy可补充 |
| TEMPLATES.md | 指令/报告/讨论模板 + 无响应推进 + 断裂接续 | 顾问 |
| ROADMAP.md | 总行动路线图（三阶段） | 顾问定，WorkBuddy更新进度 |
| unsafe_whitelist.txt | C9 门禁白名单（当前为空＝不放行任何真实 unsafe） | 顾问确认后登记 |
| instructions/ | 指令归档 | 顾问 |
| reports/ | 报告归档 | WorkBuddy |
| discussions/ | 讨论记录 | 顾问整理，用户审核 |
