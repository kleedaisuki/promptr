//! 终端界面的纯模型、输入映射与渲染投影。 / Pure TUI model, input mapping, and view projection.

pub mod action;
pub mod app;
pub mod input;
pub mod layout;
pub mod model;
pub mod terminal;
pub mod theme;
pub mod view;

pub use action::{EditorMove, MoveDirection, UiAction};
pub use app::run;
pub use input::{HitRegion, HitTarget, InputMapper};
pub use layout::{Layout, LayoutClass};
pub use model::{Effect, Mode, Model, Pane, PreviewTab, update};
pub use theme::{CustomPalette, GlyphMode, Glyphs, StyleToken, Theme};
