//! 气泡沙漏拓扑（Bubble Hourglass）。
//!
//! 结构（假设 A4，见 docs/MATH_SPEC.md）：
//!
//! ```text
//!          ┌───────────────┐
//!          │   上锥体 Upper  │  种子从顶部注入（容量受限 FIFO）
//!          └───────┬───────┘
//!                  │ 排空（同 tick 成对种子在此发生破坏性干涉）
//!          ┌───────▼───────┐
//!          │  瓶颈 RingBuffer │  环形缓冲区（窄口，容量受限）
//!          └───────┬───────┘
//!                  │ 逐个放行（节流：每 tick 至多 1 粒）
//!          ┌───────▼───────┐
//!          │   下锥体 Lower  │  输出蓄积（容量受限 FIFO）
//!          └───────┬───────┘
//!                  ▼
//!              output（每 tick 至多 1 粒）
//! ```
//!
//! **瓶颈干涉（破坏性干涉）**：同一 tick 内成对到达瓶颈的种子互相湮灭，
//! 产出 `0.0`（回到 0 锚点/真空），即"两粒气泡对撞破裂"。

use std::collections::VecDeque;

/// 容量受限的**环形缓冲区**（瓶颈本体）。
#[derive(Debug, Clone)]
pub struct RingBuffer {
    buf: Vec<Option<f32>>,
    head: usize,
    len: usize,
    cap: usize,
}

impl RingBuffer {
    /// 以给定容量构造空环形缓冲区。
    pub fn new(cap: usize) -> Self {
        Self { buf: vec![None; cap], head: 0, len: 0, cap }
    }

    /// 当前元素个数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否已满。
    pub fn is_full(&self) -> bool {
        self.len == self.cap
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 尾插；满则返回 false 且不改变状态。
    pub fn push(&mut self, v: f32) -> bool {
        if self.is_full() {
            return false;
        }
        let idx = (self.head + self.len) % self.cap;
        self.buf[idx] = Some(v);
        self.len += 1;
        true
    }

    /// 头取；空返回 None，并清空槽位（"气泡破裂后槽位归空"）。
    pub fn pop(&mut self) -> Option<f32> {
        if self.is_empty() {
            return None;
        }
        let v = self.buf[self.head].take();
        self.head = (self.head + 1) % self.cap;
        self.len -= 1;
        v
    }

    /// 从旧到新遍历（用于测试/调试）。
    pub fn iter(&self) -> impl Iterator<Item = f32> + '_ {
        (0..self.len).map(move |k| {
            self.buf[(self.head + k) % self.cap].expect("ring slot occupied")
        })
    }
}

/// 气泡沙漏：上锥体 → 瓶颈（环形缓冲 + 破坏性干涉）→ 下锥体 → 输出。
#[derive(Debug, Clone)]
pub struct BubbleHourglass {
    /// 上锥体（容量受限 FIFO）。
    upper: VecDeque<f32>,
    /// 瓶颈环形缓冲区。
    ring: RingBuffer,
    /// 下锥体（容量受限 FIFO）。
    lower: VecDeque<f32>,
    /// 上锥容量。
    top_cap: usize,
    /// 下锥容量。
    bot_cap: usize,
    /// 每 tick 从上锥排空到瓶颈的最大粒数（>1 会触发成对干涉）。
    drain_rate: usize,
    /// 破坏性干涉次数（成对湮灭计数）。
    pub interference_events: u64,
    /// 溢出丢弃粒数（锥体/瓶颈已满）。
    pub dropped: u64,
    /// 成功放行到输出的粒数。
    pub emitted: u64,
}

impl BubbleHourglass {
    /// 默认上锥容量。
    pub const DEFAULT_TOP_CAP: usize = 8;
    /// 默认瓶颈容量。
    pub const DEFAULT_RING_CAP: usize = 4;
    /// 默认下锥容量。
    pub const DEFAULT_BOT_CAP: usize = 8;
    /// 默认排空率。
    pub const DEFAULT_DRAIN_RATE: usize = 2;

    /// 以默认容量构造沙漏（8-4-8，排空率 2）。
    pub fn new() -> Self {
        Self::with_caps(
            Self::DEFAULT_TOP_CAP,
            Self::DEFAULT_RING_CAP,
            Self::DEFAULT_BOT_CAP,
            Self::DEFAULT_DRAIN_RATE,
        )
    }

    /// 以指定容量构造。
    pub fn with_caps(top_cap: usize, ring_cap: usize, bot_cap: usize, drain_rate: usize) -> Self {
        assert!(top_cap > 0 && ring_cap > 0 && bot_cap > 0 && drain_rate >= 1);
        Self {
            upper: VecDeque::with_capacity(top_cap),
            ring: RingBuffer::new(ring_cap),
            lower: VecDeque::with_capacity(bot_cap),
            top_cap,
            bot_cap,
            drain_rate,
            interference_events: 0,
            dropped: 0,
            emitted: 0,
        }
    }

    /// 从顶部注入一粒种子（0-1）；上锥已满则丢弃并计数。
    pub fn push(&mut self, seed: f32) {
        let s = crate::math::clamp01(seed);
        if self.upper.len() < self.top_cap {
            self.upper.push_back(s);
        } else {
            self.dropped += 1;
        }
    }

    /// 推进一步并返回本 tick 放行到输出的种子（至多 1 粒）。
    ///
    /// 每 tick：可选外部注入 → 上锥排空（成对发生破坏性干涉，奇数余粒单行）
    /// → 瓶颈放行 1 粒至下锥 → 下锥输出 1 粒。
    pub fn tick(&mut self, external: Option<f32>) -> Vec<f32> {
        if let Some(s) = external {
            self.push(s);
        }

        // 上锥 → 瓶颈（含破坏性干涉）
        let mut batch = Vec::with_capacity(self.drain_rate);
        while batch.len() < self.drain_rate {
            match self.upper.pop_front() {
                Some(v) => batch.push(v),
                None => break,
            }
        }
        let mut iter = batch.into_iter();
        while let Some(a) = iter.next() {
            match iter.next() {
                // 成对 → 破坏性干涉：对湮灭，产出 0（回锚点）
                Some(_b) => {
                    self.interference_events += 1;
                    if !self.ring.push(0.0) {
                        self.dropped += 1;
                    }
                }
                // 奇数余粒单行通过瓶颈
                None => {
                    if !self.ring.push(a) {
                        self.dropped += 1;
                    }
                }
            }
        }

        // 瓶颈 → 下锥（节流：每 tick 至多 1 粒）
        if self.lower.len() < self.bot_cap {
            if let Some(v) = self.ring.pop() {
                self.lower.push_back(v);
            }
        }

        // 下锥 → 输出（每 tick 至多 1 粒）
        let mut out = Vec::new();
        if let Some(v) = self.lower.pop_front() {
            out.push(v);
            self.emitted += 1;
        }
        out
    }

    /// 当前内部滞留粒数（上锥 + 瓶颈 + 下锥）。
    pub fn backlog(&self) -> usize {
        self.upper.len() + self.ring.len() + self.lower.len()
    }
}

impl Default for BubbleHourglass {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::is_valid;

    #[test]
    fn ring_buffer_roundtrip() {
        let mut r = RingBuffer::new(3);
        assert!(r.push(0.1) && r.push(0.2) && r.push(0.3));
        assert!(r.is_full());
        assert!(!r.push(0.4));
        assert_eq!(r.pop(), Some(0.1));
        assert!(r.push(0.4)); // 环形复用空槽
        assert_eq!(r.pop(), Some(0.2));
        assert_eq!(r.pop(), Some(0.3));
        assert_eq!(r.pop(), Some(0.4));
        assert!(r.is_empty());
        assert_eq!(r.pop(), None);
    }

    #[test]
    fn single_seed_trickles_through_in_one_tick() {
        let mut h = BubbleHourglass::new();
        let out = h.tick(Some(0.5));
        assert_eq!(out, vec![0.5]);
        assert_eq!(h.emitted, 1);
        assert_eq!(h.interference_events, 0);
    }

    #[test]
    fn burst_pair_destructively_interferes() {
        let mut h = BubbleHourglass::new();
        h.push(0.8);
        h.push(0.7); // 同 tick 成对
        let out = h.tick(None);
        assert_eq!(h.interference_events, 1);
        assert_eq!(out, vec![0.0]); // 对湮灭 → 0 锚点
    }

    #[test]
    fn odd_burst_keeps_one_survivor() {
        let mut h = BubbleHourglass::new();
        h.push(0.9);
        h.push(0.8);
        h.push(0.7); // 3 粒 → 一对湮灭 + 1 粒单行
        let out = h.tick(None);
        assert_eq!(h.interference_events, 1);
        assert_eq!(out, vec![0.0]); // 幸存粒会在下一 tick 放行
    }

    #[test]
    fn overflow_drops_are_counted() {
        let mut h = BubbleHourglass::with_caps(2, 2, 2, 1);
        for _ in 0..10 {
            h.push(1.0); // 上锥容量 2，后续被丢弃
        }
        assert!(h.dropped >= 8);
    }

    #[test]
    fn long_run_stays_valid_and_bounded() {
        let mut h = BubbleHourglass::new();
        for i in 0..10_000 {
            let ext = if i % 7 == 0 { Some(0.9) } else { None };
            let out = h.tick(ext);
            for v in out {
                assert!(is_valid(v), "iter {i}: {v}");
            }
        }
        assert!(h.interference_events > 0);
        assert!(h.emitted > 0);
        assert_eq!(h.backlog(), h.upper.len() + h.ring.len() + h.lower.len());
    }
}
