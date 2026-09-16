//! 占用位图（纯逻辑，**零 unsafe**）。
//!
//! 一位代表一个单元（帧或小块）的占用状态。提供 **first-fit** 分配：从低位往高位找第一个
//! 满足谓词的**空闲**单元。O(n) 但**确定、可测、无隐藏状态**——这正是"最简"的取舍。

use crate::MemErr;

/// 借用调用方缓冲区的占用位图。
#[derive(Debug)]
pub struct UsedMap<'a> {
    bytes: &'a mut [u8],
    bits: usize,
    used: usize,
}

impl<'a> UsedMap<'a> {
    /// `bytes` 是位图存储；`bits` 是可用单元数。
    ///
    /// **判据**：`bits` 必须 ≥1 且 `≤ bytes.len()*8`；否则返回 [`MemErr::BadCapacity`]。
    /// 初始化时**全部位清零**（全空闲）——避免把上一轮的残留状态当成"已占用"。
    pub fn new(bytes: &'a mut [u8], bits: usize) -> Result<Self, MemErr> {
        if bits == 0 || bits > bytes.len() * 8 {
            return Err(MemErr::BadCapacity);
        }
        for b in bytes.iter_mut() {
            *b = 0;
        }
        Ok(Self { bytes, bits, used: 0 })
    }

    /// 单元总数。
    pub fn capacity(&self) -> usize {
        self.bits
    }

    /// 已占用数。
    pub fn used_count(&self) -> usize {
        self.used
    }

    fn get(&self, i: usize) -> Result<bool, MemErr> {
        if i >= self.bits {
            return Err(MemErr::OutOfRange);
        }
        Ok(self.bytes[i / 8] & (1 << (i % 8)) != 0)
    }

    fn set(&mut self, i: usize, used: bool) {
        let mask = 1u8 << (i % 8);
        if used {
            self.bytes[i / 8] |= mask;
        } else {
            self.bytes[i / 8] &= !mask;
        }
    }

    /// 该单元是否已占用（越界返回 `Err`，**不静默当空闲**）。
    pub fn is_used(&self, i: usize) -> Result<bool, MemErr> {
        self.get(i)
    }

    /// 标记占用；已占用 ⇒ [`MemErr::AlreadyUsed`]。
    pub fn mark_used(&mut self, i: usize) -> Result<(), MemErr> {
        if self.get(i)? {
            return Err(MemErr::AlreadyUsed);
        }
        self.set(i, true);
        self.used += 1;
        Ok(())
    }

    /// 标记空闲；本就空闲 ⇒ [`MemErr::AlreadyFree`]（**重复释放可判定**）。
    pub fn mark_free(&mut self, i: usize) -> Result<(), MemErr> {
        if !self.get(i)? {
            return Err(MemErr::AlreadyFree);
        }
        self.set(i, false);
        self.used -= 1;
        Ok(())
    }

    /// **first-fit**：返回第一个"空闲 **且** 被 `accept` 接受"的单元索引，并标记为占用。
    ///
    /// `accept` 里放的是**与地址有关的约束**（例如对齐）——由调用方（知道基址的一侧）提供，
    /// 本层只认**索引**，故仍然零 unsafe、零地址知识。
    pub fn alloc_first_fit<F>(&mut self, accept: F) -> Option<usize>
    where
        F: Fn(usize) -> bool,
    {
        for i in 0..self.bits {
            if let Ok(false) = self.get(i) {
                if accept(i) {
                    // 这里一定成功：刚判过是空闲且 i 在界内
                    if self.mark_used(i).is_ok() {
                        return Some(i);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(buf: &mut [u8], bits: usize) -> UsedMap<'_> {
        UsedMap::new(buf, bits).expect("容量合法")
    }

    #[test]
    fn new_rejects_zero_and_oversized_bits() {
        let mut b = [0u8; 4];
        assert_eq!(UsedMap::new(&mut b, 0).unwrap_err(), MemErr::BadCapacity);
        assert_eq!(UsedMap::new(&mut b, 33).unwrap_err(), MemErr::BadCapacity);
        assert!(UsedMap::new(&mut b, 32).is_ok());
    }

    #[test]
    fn new_zeroes_old_garbage() {
        let mut b = [0xFFu8; 2];
        let m = UsedMap::new(&mut b, 16).unwrap();
        assert_eq!(m.used_count(), 0, "初始化须清零，不能把残留当已占用");
    }

    #[test]
    fn out_of_range_is_error_not_silent_free() {
        let mut b = [0u8; 1];
        let mut m = map(&mut b, 8);
        assert_eq!(m.mark_used(8).unwrap_err(), MemErr::OutOfRange);
        assert_eq!(m.is_used(99).unwrap_err(), MemErr::OutOfRange);
    }

    #[test]
    fn double_free_is_detected() {
        let mut b = [0u8; 1];
        let mut m = map(&mut b, 8);
        m.mark_used(3).unwrap();
        m.mark_free(3).unwrap();
        assert_eq!(m.mark_free(3).unwrap_err(), MemErr::AlreadyFree);
    }

    #[test]
    fn double_use_is_detected() {
        let mut b = [0u8; 1];
        let mut m = map(&mut b, 8);
        m.mark_used(3).unwrap();
        assert_eq!(m.mark_used(3).unwrap_err(), MemErr::AlreadyUsed);
    }

    #[test]
    fn first_fit_returns_lowest_index_and_skips_used() {
        let mut b = [0u8; 2];
        let mut m = map(&mut b, 16);
        assert_eq!(m.alloc_first_fit(|_| true), Some(0));
        m.mark_used(1).unwrap();
        assert_eq!(m.alloc_first_fit(|_| true), Some(2));
        assert_eq!(m.used_count(), 3);
    }

    #[test]
    fn first_fit_respects_predicate() {
        let mut b = [0u8; 2];
        let mut m = map(&mut b, 16);
        // 只接受偶数索引
        assert_eq!(m.alloc_first_fit(|i| i % 2 == 0), Some(0));
        assert_eq!(m.alloc_first_fit(|i| i % 2 == 0), Some(2));
        assert_eq!(m.alloc_first_fit(|i| i % 2 == 0), Some(4));
    }

    #[test]
    fn exhaustion_returns_none_and_does_not_overcount() {
        let mut b = [0u8; 1];
        let mut m = map(&mut b, 8);
        for _ in 0..8 {
            assert!(m.alloc_first_fit(|_| true).is_some());
        }
        assert_eq!(m.alloc_first_fit(|_| true), None, "满时应返回 None");
        assert_eq!(m.used_count(), 8);
    }

    #[test]
    fn free_then_reuse_returns_same_index() {
        let mut b = [0u8; 1];
        let mut m = map(&mut b, 8);
        let a = m.alloc_first_fit(|_| true).unwrap();
        let c = m.alloc_first_fit(|_| true).unwrap();
        assert_ne!(a, c);
        m.mark_free(a).unwrap();
        assert_eq!(m.alloc_first_fit(|_| true), Some(a), "释放后应可复用");
    }
}
