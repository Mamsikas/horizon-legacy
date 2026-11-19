//! Keep draw data in the existing dense column store. Empty sections need only
//! presence for particle/entity occlusion, not a Section/Arc/mesh allocation.
use super::*;
use std::ops::{Deref, DerefMut, Index};

#[derive(Default)]
pub(super) struct Sections {
    dense: gfx::resident::Resident<Section>,
    empty: FxHashSet<SectionKey>,
}
impl Sections {
    pub fn get(&self, key: &SectionKey) -> Option<&Section> {
        self.dense.find(*key)
    }
    pub fn get_mut(&mut self, key: &SectionKey) -> Option<&mut Section> {
        self.dense.find_mut(*key)
    }
    pub fn at(&self, slot: u32) -> &Section {
        self.dense.get(slot)
    }
    pub fn contains_key(&self, key: &SectionKey) -> bool {
        self.empty.contains(key) || self.dense.contains(*key)
    }
    pub fn insert(&mut self, key: SectionKey, value: Section, bounds: Option<[Vec3; 2]>) {
        self.empty.remove(&key);
        let drawable = !value.vertices.is_empty();
        self.dense.insert_marked(key, value, bounds, drawable);
    }
    pub fn insert_empty(&mut self, key: SectionKey) {
        debug_assert!(!self.dense.contains(key));
        self.empty.insert(key);
    }
    pub fn remove(&mut self, key: &SectionKey) -> Option<Section> {
        self.empty.remove(key);
        self.dense.remove(*key)
    }
    pub fn keys(&self) -> impl Iterator<Item = SectionKey> + '_ {
        self.dense
            .iter()
            .map(|(k, _, _)| k)
            .chain(self.empty.iter().copied())
    }
    pub fn iter(&self) -> impl Iterator<Item = (SectionKey, &Section)> {
        self.dense.iter().map(|(k, v, _)| (k, v))
    }
    pub fn len(&self) -> usize {
        self.dense.len() + self.empty.len()
    }
    pub fn heap_bytes(&self) -> usize {
        self.dense.heap_bytes() + self.empty.capacity() * (size_of::<SectionKey>() + 1)
    }
    pub fn clear(&mut self) {
        self.dense.clear();
        self.empty.clear();
    }
}
impl Deref for Sections {
    type Target = gfx::resident::Resident<Section>;
    fn deref(&self) -> &Self::Target {
        &self.dense
    }
}
impl DerefMut for Sections {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.dense
    }
}
impl Index<&SectionKey> for Sections {
    type Output = Section;
    fn index(&self, key: &SectionKey) -> &Section {
        self.get(key).expect("resident DX9 section")
    }
}
