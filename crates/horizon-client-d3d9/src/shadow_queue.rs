//! Bounded lossless-compression backlog. Replacement never retains the obsolete bytes.
use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
};

pub struct ShadowQueue<K> {
    keys: VecDeque<K>,
    sizes: HashMap<K, usize>,
    bytes: usize,
    limit: usize,
}
impl<K: Copy + Eq + Hash> ShadowQueue<K> {
    pub fn new(limit: usize) -> Self {
        Self {
            keys: VecDeque::new(),
            sizes: HashMap::new(),
            bytes: 0,
            limit,
        }
    }
    pub fn admit(&mut self, key: K, bytes: usize) -> bool {
        let previous = self.sizes.get(&key).copied().unwrap_or(0);
        let Some(next) = (self.bytes - previous).checked_add(bytes) else {
            self.cancel(key);
            return false;
        };
        if bytes == 0 || next > self.limit || self.sizes.len() == 64 && previous == 0 {
            self.cancel(key);
            return false;
        }
        if self.sizes.insert(key, bytes).is_none() {
            self.keys.push_back(key);
        }
        self.bytes = next;
        true
    }
    pub fn cancel(&mut self, key: K) {
        if let Some(bytes) = self.sizes.remove(&key) {
            self.bytes -= bytes;
            self.keys.retain(|k| *k != key);
        }
    }
    pub fn pop(&mut self) -> Option<K> {
        let key = self.keys.pop_front()?;
        self.bytes -= self.sizes.remove(&key).unwrap();
        Some(key)
    }
    pub fn clear(&mut self) {
        self.keys.clear();
        self.sizes.clear();
        self.bytes = 0;
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn len(&self) -> usize {
        self.sizes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.sizes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_cancellation_and_pressure_bound_the_backlog() {
        let mut q = ShadowQueue::new(10);
        assert!(q.admit(1, 6));
        assert!(q.admit(2, 4));
        assert!(!q.admit(3, 1));
        assert!(q.admit(1, 3));
        assert_eq!((q.bytes(), q.len()), (7, 2));
        assert_eq!(q.pop(), Some(1));
        assert!(!q.admit(2, 11));
        assert_eq!((q.bytes(), q.pop()), (0, None));
        for key in 0..64 {
            assert!(q.admit(key, 1) == (key < 10));
        }
        q.clear();
        assert_eq!((q.len(), q.bytes(), q.pop()), (0, 0, None));
        let mut q = ShadowQueue::new(usize::MAX);
        for key in 0..64 {
            assert!(q.admit(key, 1));
        }
        assert!(!q.admit(64, 1));
        assert!(!q.admit(0, usize::MAX));
        assert_eq!((q.len(), q.bytes()), (63, 63));
    }
}
