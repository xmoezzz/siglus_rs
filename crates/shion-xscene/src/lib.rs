pub use shion_xfile::repair::*;
pub use shion_xfile::validation::*;
pub use shion_xfile::semantic::*;

pub fn scene_from_bytes(bytes: &[u8]) -> shion_xfile::Result<Scene> {
    let file = shion_xfile::parse_x(bytes)?;
    Scene::from_xfile(&file)
}
