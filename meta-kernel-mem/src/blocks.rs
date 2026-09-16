//! 小块分配器（纯逻辑，**零 unsafe**）。
//!
//! 单元 = **64 B 块**；服务 `GlobalAlloc` 里"小于一帧"的请求。
//! 对齐：块基址必须由调用方给定（`base_addr`），本层用 `(base_addr + i*BLOCK_SIZE) % align == 0` 判定——
//! 这样**对齐逻辑也是纯函数**，可在 host 上穷举验证。

use crate::bitmap::UsedMap;
use crate::{MemErr, Stats, BLOCK_SIZE};

/// 小块分配器（≤ [`BLOCK_SIZE`] 的请求专用）。
#[derive(Debug)]
pub struct BlockAllocator<'a> {
    map: UsedMap<'a>,
    /// 第 0 号块的**绝对地址**（由边界层给出；本层只做算术）。
    base_addr: usize,
}

impl<'a> BlockAllocator<'a> {
    /// `bits` 个块；`base_addr` 是第 0 号块的绝对地址。
    pub fn new(bitmap_bytes: &'a mut [u8], bits: usize, base_addr: usize) -> Result<Self, MemErr> {
        Ok(Self {
            map: UsedMap::new(bitmap_bytes, bits)?,
            base_addr,
        })
    }

    /// 块总容量。
    pub fn capacity(&self) -> usize {
        self.map.capacity()
    }

    /// 已占用块数。
    pub fn used(&self) -> usize {
        self.map.used_count()
    }

    /// 总字节容量。
    pub fn byte_len(&self) -> usize {
        self.map.capacity() * BLOCK_SIZE
    }

    /// 分配 `bytes` 字节、`align` 对齐的块，返回**块绝对地址**。
    ///
    /// **拒绝条件（都是可判定的）**：
    /// - `bytes == 0` 或 `bytes > BLOCK_SIZE` ⇒ `None`（应走帧分配）
    /// - `align` 非 2 的幂或 `> BLOCK_SIZE` ⇒ `None`
    /// - 无满足条件的空闲块 ⇒ `None`（容量耗尽）
    pub fn alloc(&mut self, bytes: usize, align: usize) -> Option<usize> {
        if bytes == 0 || bytes > BLOCK_SIZE {
            return None;
        }
        if align == 0 || !align.is_power_of_two() || align > BLOCK_SIZE {
            return None;
        }
        let base = self.base_addr;
        self.map
            .alloc_first_fit(|i| (base + i * BLOCK_SIZE) % align == 0)
            .map(|i| base + i * BLOCK_SIZE)
    }

    /// 释放**块绝对地址**。
    ///
    /// 判据（可测）：地址必须落在本区且**块对齐**；否则 [`MemErr::OutOfRange`]。
    pub fn free(&mut self, addr: usize) -> Result<(), MemErr> {
        if addr < self.base_addr {
            return Err(MemErr::OutOfRange);
        }
        let off = addr - self.base_addr;
        if off % BLOCK_SIZE != 0 {
            return Err(MemErr::BadAlignment);
        }
        self.map.mark_free(off / BLOCK_SIZE)
    }

    /// 统计快照。
    pub fn stats(&self) -> Stats {
        Stats {
            frames_used: 0,
            frames_total: 0,
            blocks_used: self.used(),
            blocks_total: self.capacity(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_returns_absolute_address_inside_region() {
        let mut bm = [0u8; 16];
        let mut a = BlockAllocator::new(&mut bm, 128, 0x1000).unwrap();
        let p = a.alloc(16, 8).unwrap();
        assert_eq!(p, 0x1000);
        assert!(p >= 0x1000 && p < 0x1000 + a.byte_len());
        assert_eq!(p % 8, 0);
    }

    #[test]
    fn alignment_is_enforced_via_base_addr() {
        let mut bm = [0u8; 16];
        // 基址 64（0x40）：第 0 块=64 只满足 64 对齐；要 64 对齐即命中第 0 块
        let mut a = BlockAllocator::new(&mut bm, 128, 64).unwrap();
        let p = a.alloc(8, 64).unwrap();
        assert_eq!(p, 64);
        assert_eq!(p % 64, 0);
    }

    #[test]
    fn align_larger_than_block_is_refused_by_policy() {
        let mut bm = [0u8; 16];
        let mut a = BlockAllocator::new(&mut bm, 128, 64).unwrap();
        // **分层口径**：超过块粒度的对齐不由小块层满足（须由调用方分流到帧层），
        // 且**绝不**返回一个不满足对齐的地址（那才是真 bug）。
        assert_eq!(a.alloc(8, 128), None);
        assert_eq!(a.used(), 0, "拒绝时不得占用任何块");
    }

    #[test]
    fn oversized_request_is_routed_away_not_truncated() {
        let mut bm = [0u8; 16];
        let mut a = BlockAllocator::new(&mut bm, 128, 0).unwrap();
        assert_eq!(a.alloc(0, 8), None, "0 字节应拒绝");
        assert_eq!(a.alloc(BLOCK_SIZE + 1, 8), None, "超过块大小应交给帧层");
    }

    #[test]
    fn bad_alignment_rejected() {
        let mut bm = [0u8; 16];
        let mut a = BlockAllocator::new(&mut bm, 128, 0).unwrap();
        assert_eq!(a.alloc(8, 3), None, "非 2 的幂");
        assert_eq!(a.alloc(8, BLOCK_SIZE * 2), None, "超过块大小");
    }

    #[test]
    fn free_requires_block_aligned_addr() {
        let mut bm = [0u8; 16];
        let mut a = BlockAllocator::new(&mut bm, 128, 0x2000).unwrap();
        let p = a.alloc(8, 1).unwrap();
        assert_eq!(a.free(p + 1).unwrap_err(), MemErr::BadAlignment);
        assert_eq!(a.free(0x1000).unwrap_err(), MemErr::OutOfRange);
        a.free(p).unwrap();
        assert_eq!(a.used(), 0);
    }

    #[test]
    fn free_then_reuse_same_block() {
        let mut bm = [0u8; 16];
        let mut a = BlockAllocator::new(&mut bm, 128, 0x1000).unwrap();
        let p = a.alloc(8, 1).unwrap();
        let _q = a.alloc(8, 1).unwrap();
        a.free(p).unwrap();
        assert_eq!(a.alloc(8, 1), Some(p), "释放后应复用同块");
    }

    #[test]
    fn exhaustion_returns_none() {
        let mut bm = [0u8; 2]; // 16 bits
        let mut a = BlockAllocator::new(&mut bm, 16, 0).unwrap();
        for _ in 0..16 {
            assert!(a.alloc(8, 1).is_some());
        }
        assert_eq!(a.alloc(8, 1), None);
    }
}
