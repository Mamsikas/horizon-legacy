//! A bounded cache of already-generated animation mip pixels. Cache misses keep
//! the existing generator; evicting a frame never drops its source animation.
use std::collections::HashMap;
struct Entry {
    bytes: Vec<u8>,
    raw_len: usize,
    packed: bool,
    used: u64,
}
pub struct AnimationCache {
    entries: HashMap<(u32, usize), Entry>,
    budget: usize,
    bytes: usize,
    clock: u64,
    pub hits: u64,
    pub misses: u64,
}
impl AnimationCache {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            budget,
            bytes: 0,
            clock: 0,
            hits: 0,
            misses: 0,
        }
    }
    pub fn bytes(&self) -> usize {
        self.bytes + self.entries.capacity() * size_of::<((u32, usize), Entry)>()
    }
    pub fn decode(&mut self, layer: u32, frame: usize, out: &mut Vec<u8>) -> Result<bool, String> {
        self.clock = self.clock.wrapping_add(1);
        let Some(entry) = self.entries.get_mut(&(layer, frame)) else {
            self.misses += 1;
            return Ok(false);
        };
        entry.used = self.clock;
        out.resize(entry.raw_len, 0);
        if entry.packed {
            let n =
                lz4_flex::block::decompress_into(&entry.bytes, out).map_err(|e| e.to_string())?;
            if n != out.len() {
                return Err("D3D9 animation cache length mismatch".into());
            }
        } else {
            out.copy_from_slice(&entry.bytes);
        }
        self.hits += 1;
        Ok(true)
    }
    pub fn insert(&mut self, layer: u32, frame: usize, rgba: &[u8]) {
        let packed = lz4_flex::block::compress(rgba);
        let compressed = packed.len() < rgba.len();
        let bytes = if compressed { packed } else { rgba.to_vec() };
        let cost = bytes.capacity();
        if cost > self.budget {
            return;
        }
        if let Some(old) = self.entries.remove(&(layer, frame)) {
            self.bytes -= old.bytes.capacity();
        }
        while self.bytes + cost > self.budget {
            let Some((&key, _)) = self.entries.iter().min_by_key(|(_, e)| e.used) else {
                break;
            };
            self.bytes -= self.entries.remove(&key).unwrap().bytes.capacity();
        }
        self.clock = self.clock.wrapping_add(1);
        self.entries.insert(
            (layer, frame),
            Entry {
                bytes,
                raw_len: rgba.len(),
                packed: compressed,
                used: self.clock,
            },
        );
        self.bytes += cost;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn animation_pixels_survive_reuse_and_lru_eviction_with_bounded_payload() {
        let mut cache = AnimationCache::new(32);
        let mut out = Vec::new();
        let frames = std::array::from_fn::<_, 3, _>(|n| {
            (0..16).map(|i| (n * 17 + i * 5) as u8).collect::<Vec<_>>()
        });
        cache.insert(0, 0, &frames[0]);
        cache.insert(0, 1, &frames[1]);
        assert!(cache.decode(0, 0, &mut out).unwrap());
        assert_eq!(out, frames[0]);
        cache.insert(0, 2, &frames[2]);
        assert!(cache.bytes <= 32);
        assert!(!cache.decode(0, 1, &mut out).unwrap());
        assert!(cache.decode(0, 0, &mut out).unwrap());
        assert_eq!(out, frames[0]);
        assert!(cache.decode(0, 2, &mut out).unwrap());
        assert_eq!(out, frames[2]);
        // Raw/noisy oversized frames keep working through the caller's generator.
        cache.insert(
            7,
            1,
            &(0..4096)
                .map(|i| (i * 71 + i / 3) as u8)
                .collect::<Vec<_>>(),
        );
        assert!(cache.bytes <= 32);
        let mut compressed = AnimationCache::new(128);
        let pixels = vec![7; 4096];
        compressed.insert(2, 3, &pixels);
        assert!(compressed.decode(2, 3, &mut out).unwrap());
        assert_eq!(out, pixels);
    }
}
