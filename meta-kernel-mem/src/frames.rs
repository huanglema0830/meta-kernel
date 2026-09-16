//! 物理帧分配器（纯逻辑，**零 unsafe**）。
//!
//! 单元 = **4 KiB 帧**；只认**帧索引**（不认识物理地址——地址换算在 boot 层的边界文件里）。
//! 分配策略：**first-fit**（确定性；不做碎片整理——"最简"的取舍）。

use crate::bitmap::UsedMap;
use crate::{MemErr, Stats, FRAME_SIZE};

/// 帧分配器。`base_frame` 用于把索引换算成"物理帧号"（仅作标识，**不参与分配逻辑**）。
#[derive(Debug)]
pub struct FrameAllocator<'a> {
    map: UsedMap<'a>,
    base_frame: usize,
}

impl<'a> FrameAllocator<'a> {
    /// `bits` 个帧；`base_frame` 是第 0 号索引对应的物理帧号（纯标识）。
    pub fn new(bitmap_bytes: &'a mut [u8], bits: usize, base_frame: usize) -> Result<Self, MemErr> {
        Ok(Self {
            map: UsedMap::new(bitmap_bytes, bits)?,
            base_frame,
        })
    }

    /// 帧总容量。
    pub fn capacity(&self) -> usize {
        self.map.capacity()
    }

    /// 已占用帧数。
    pub fn used(&self) -> usize {
        self.map.used_count()
    }

    /// 总字节容量。
    pub fn byte_len(&self) -> usize {
        self.map.capacity() * FRAME_SIZE
    }

    /// 分配一帧，返回**物理帧号**（`base_frame + 索引`）。
    pub fn alloc(&mut self) -> Option<usize> {
        self.map
            .alloc_first_fit(|_| true)
            .map(|i| self.base_frame + i)
    }

    /// 分配满足 `align` 的帧（`align` 必须是 2 的幂且 **≤ [`FRAME_SIZE`]**）。
    ///
    /// 判据（可测）：物理地址 `(base_frame+索引)*FRAME_SIZE` 能被 `align` 整除。
    ///
    /// ⚠️ **分层口径（勿误解为 bug）**：`align > FRAME_SIZE` 时本层**返回 `None`**
    /// ——因为用 4 KiB 帧满足更大的对齐需要"连续多帧且起始帧满足边界"，属更复杂的分配器范畴。
    /// 调用方（`GlobalAlloc` 边界层）必须**提前分流**：小对齐走小块、≤帧的对齐走帧、更大的对齐**明确拒绝**。
    pub fn alloc_aligned(&mut self, align: usize) -> Option<usize> {
        if align == 0 || !align.is_power_of_two() || align > FRAME_SIZE {
            return None;
        }
        let base_frame = self.base_frame;
        self.map
            .alloc_first_fit(|i| ((base_frame + i) * FRAME_SIZE) % align == 0)
            .map(|i| base_frame + i)
    }

    /// 释放帧（按物理帧号）。**重复释放 / 越界都会报错**，不静默。
    pub fn free(&mut self, frame: usize) -> Result<(), MemErr> {
        if frame < self.base_frame {
            return Err(MemErr::OutOfRange);
        }
        self.map.mark_free(frame - self.base_frame)
    }

    /// 统计快照。
    pub fn stats(&self) -> Stats {
        Stats {
            frames_used: self.used(),
            frames_total: self.capacity(),
            blocks_used: 0,
            blocks_total: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alloc(nbits: usize) -> (Vec<u8>, usize) {
        (vec![0u8; nbits.div_ceil(8)], nbits)
    }

    #[test]
    fn alloc_returns_sequential_frames_from_base() {
        let (mut bm, n) = alloc(64);
        let mut f = FrameAllocator::new(&mut bm, n, 512).unwrap();
        assert_eq!(f.alloc(), Some(512));
        assert_eq!(f.alloc(), Some(513));
        assert_eq!(f.used(), 2);
    }

    #[test]
    fn free_then_alloc_reuses_frame() {
        let (mut bm, n) = alloc(64);
        let mut f = FrameAllocator::new(&mut bm, n, 0).unwrap();
        let a = f.alloc().unwrap();
        let _b = f.alloc().unwrap();
        f.free(a).unwrap();
        assert_eq!(f.alloc(), Some(a), "释放的帧应被复用（否则会线性泄漏）");
    }

    #[test]
    fn double_free_is_error() {
        let (mut bm, n) = alloc(64);
        let mut f = FrameAllocator::new(&mut bm, n, 0).unwrap();
        let a = f.alloc().unwrap();
        f.free(a).unwrap();
        assert_eq!(f.free(a).unwrap_err(), MemErr::AlreadyFree);
    }

    #[test]
    fn free_below_base_is_out_of_range() {
        let (mut bm, n) = alloc(64);
        let mut f = FrameAllocator::new(&mut bm, n, 100).unwrap();
        assert_eq!(f.free(99).unwrap_err(), MemErr::OutOfRange);
    }

    #[test]
    fn exhaustion_then_capacity_reflects_state() {
        let (mut bm, n) = alloc(16);
        let mut f = FrameAllocator::new(&mut bm, n, 0).unwrap();
        for _ in 0..16 {
            assert!(f.alloc().is_some());
        }
        assert_eq!(f.alloc(), None);
        assert_eq!(f.used(), f.capacity());
        assert_eq!(f.byte_len(), 16 * FRAME_SIZE);
    }

    #[test]
    fn aligned_alloc_satisfies_alignment() {
        let (mut bm, n) = alloc(64);
        // base_frame=1 ⇒ 第 0 号帧物理地址 = 4096（=1*4096）
        let mut f = FrameAllocator::new(&mut bm, n, 1).unwrap();
        let a = f.alloc_aligned(2048).unwrap();
        assert_eq!(a, 1, "1 号帧地址 4096 已满足 2048 对齐");
        assert_eq!((a * FRAME_SIZE) % 2048, 0, "必须满足对齐");
        // 再来一次：应选到下一个满足 2048 的帧
        let b = f.alloc_aligned(2048).unwrap();
        assert_eq!((b * FRAME_SIZE) % 2048, 0);
        assert_ne!(a, b, "不能重复分配同一帧");
    }

    #[test]
    fn align_larger_than_frame_is_refused_by_policy() {
        let (mut bm, n) = alloc(64);
        let mut f = FrameAllocator::new(&mut bm, n, 0).unwrap();
        // **分层口径**：超过帧大小的对齐不由本层满足（须由调用方分流），
        // 这**不是**"尽力而为后返回一个不满足对齐的地址"——那种才是真 bug。
        assert_eq!(f.alloc_aligned(FRAME_SIZE * 2), None);
        assert_eq!(f.used(), 0, "拒绝时不得占用任何帧");
    }

    #[test]
    fn bad_alignment_is_rejected() {
        let (mut bm, n) = alloc(64);
        let mut f = FrameAllocator::new(&mut bm, n, 0).unwrap();
        assert_eq!(f.alloc_aligned(0), None);
        assert_eq!(f.alloc_aligned(3), None, "非 2 的幂");
    }
}
