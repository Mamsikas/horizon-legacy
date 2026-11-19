//! Return owned staging inputs to the existing mesher pool, off the frame thread.
use super::*;

#[derive(Clone)]
pub struct MeshRecycler(Arc<Shared>);

fn put(shared: &Shared, mesh: SectionMesh) {
    let rejected = shared.meshes.lock().unwrap().put(mesh);
    drop(rejected);
}

impl MeshRecycler {
    pub fn recycle(&self, mesh: SectionMesh) {
        put(&self.0, mesh);
    }
}

pub(super) fn many(shared: &Shared, meshes: Vec<((i32, i32, i32), SectionMesh)>) {
    for (_, mesh) in meshes {
        put(shared, mesh);
    }
}

impl Streaming {
    pub fn mesh_recycler(&self) -> MeshRecycler {
        MeshRecycler(self.shared.clone())
    }
}
