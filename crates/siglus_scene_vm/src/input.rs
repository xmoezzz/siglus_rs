//! Input mapping for the viewer.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerAction {
    PrevFrame,
    NextFrame,
    Reload,
    Quit,
}
