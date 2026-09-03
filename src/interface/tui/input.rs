//! Crossterm 事件到统一动作的映射。 / Mapping Crossterm events to unified actions.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use super::{
    EditorMove, MoveDirection, UiAction,
    model::{Mode, Pane, PreviewTab},
};

/// @brief view 发布的可点击语义目标。 / Semantic hit target published by the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    Pane(Pane),
    Node(usize),
    Preview(PreviewTab),
    OpenSelected,
}

/// @brief 矩形与语义目标的绑定。 / Binding between a rectangle and semantic target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitRegion {
    /// @brief 可点击矩形。 / Clickable rectangle.
    pub rect: Rect,
    /// @brief 矩形代表的动作目标。 / Action target represented by the rectangle.
    pub target: HitTarget,
}

/// @brief 集中处理键盘、鼠标、粘贴与 resize 的输入映射器。 / Central mapper for keyboard, mouse, paste, and resize.
#[derive(Debug, Clone, Default)]
pub struct InputMapper {
    regions: Vec<HitRegion>,
}

impl InputMapper {
    /// @brief 创建没有命中区域的输入映射器。 / Create an input mapper without hit regions.
    pub fn new() -> Self {
        Self::default()
    }

    /// @brief 替换为最近一次 view 生成的命中区域。 / Replace hit regions with those from the latest view pass.
    /// @param regions 最新区域；不会自行重算几何。 / Latest regions; geometry is never independently recomputed.
    pub fn set_regions(&mut self, regions: Vec<HitRegion>) {
        self.regions = regions;
    }

    /// @brief 将一个终端事件映射成至多一个用户动作。 / Map a terminal event to at most one user action.
    /// @param event Crossterm 终端事件。 / Crossterm terminal event.
    /// @param mode 事件发生时的明确模式。 / Explicit mode at event time.
    /// @return 忽略 release 事件，否则返回统一动作。 / No action for release events, otherwise a unified action.
    pub fn map(&self, event: Event, mode: Mode) -> Option<UiAction> {
        match event {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                Some(map_key(key, mode))
            }
            Event::Key(_) => None,
            Event::Mouse(mouse) => self.map_mouse(mouse),
            Event::Paste(text) => Some(UiAction::InsertText(text)),
            Event::Resize(width, height) => Some(UiAction::Resize { width, height }),
            Event::FocusGained => Some(UiAction::CheckExternalChanges),
            Event::FocusLost => Some(UiAction::Noop),
        }
    }

    fn map_mouse(&self, event: MouseEvent) -> Option<UiAction> {
        match event.kind {
            MouseEventKind::ScrollUp => Some(UiAction::ScrollPane(-3)),
            MouseEventKind::ScrollDown => Some(UiAction::ScrollPane(3)),
            MouseEventKind::Down(MouseButton::Left) => self
                .regions
                .iter()
                .find(|region| contains(region.rect, event.column, event.row))
                .map(|region| action_for_target(region.target)),
            _ => None,
        }
    }
}

fn contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && row >= rect.y
        && column < rect.x.saturating_add(rect.width)
        && row < rect.y.saturating_add(rect.height)
}

fn action_for_target(target: HitTarget) -> UiAction {
    match target {
        HitTarget::Pane(pane) => UiAction::FocusPane(pane),
        HitTarget::Node(index) => UiAction::SelectNode(index),
        HitTarget::Preview(tab) => UiAction::SelectPreview(tab),
        HitTarget::OpenSelected => UiAction::OpenSelected,
    }
}

fn map_key(key: KeyEvent, mode: Mode) -> UiAction {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => {
                if mode == Mode::Browse {
                    UiAction::Quit
                } else {
                    UiAction::Cancel
                }
            }
            KeyCode::Char('s') if matches!(mode, Mode::FragmentEdit | Mode::MetadataEdit) => {
                UiAction::Submit
            }
            KeyCode::Char('z') if mode == Mode::FragmentEdit => UiAction::UndoEditor,
            KeyCode::Char('y') if mode == Mode::FragmentEdit => UiAction::RedoEditor,
            KeyCode::Char('f') if mode == Mode::FragmentEdit => UiAction::StartEditorFind,
            KeyCode::Left if mode == Mode::FragmentEdit => {
                UiAction::MoveEditorCursor(EditorMove::WordLeft)
            }
            KeyCode::Right if mode == Mode::FragmentEdit => {
                UiAction::MoveEditorCursor(EditorMove::WordRight)
            }
            KeyCode::Char('d') if mode == Mode::Browse => UiAction::ScrollPane(10),
            KeyCode::Char('u') if mode == Mode::Browse => UiAction::ScrollPane(-10),
            _ => UiAction::Noop,
        };
    }
    match (mode, key.code) {
        (_, KeyCode::Esc) => UiAction::Cancel,
        (Mode::Confirm(_), KeyCode::Enter | KeyCode::Char('y')) => UiAction::Confirm,
        (Mode::Confirm(_), KeyCode::Char('n')) => UiAction::Reject,
        (Mode::Browse, KeyCode::Char('j') | KeyCode::Down) => {
            UiAction::MoveSelection(MoveDirection::Next)
        }
        (Mode::Browse, KeyCode::Char('k') | KeyCode::Up) => {
            UiAction::MoveSelection(MoveDirection::Previous)
        }
        (Mode::Browse, KeyCode::Enter | KeyCode::Right) => UiAction::OpenSelected,
        (Mode::Browse, KeyCode::Char('/')) => UiAction::StartSearch,
        (Mode::Browse, KeyCode::Char(':')) => UiAction::StartCommand,
        (Mode::Browse, KeyCode::Char('e')) => UiAction::StartFragmentEdit,
        (Mode::Browse, KeyCode::Char('m')) => UiAction::StartMetadataEdit,
        (Mode::Browse, KeyCode::Char('r')) => UiAction::StartRename,
        (Mode::Browse, KeyCode::Char('d')) => UiAction::RequestDelete,
        (Mode::Browse, KeyCode::Char('y')) => UiAction::CopyCanonicalXml,
        (Mode::Browse, KeyCode::Char('p')) if key.modifiers.contains(KeyModifiers::SHIFT) => {
            UiAction::CyclePreview(MoveDirection::Previous)
        }
        (Mode::Browse, KeyCode::Char('p')) => UiAction::CyclePreview(MoveDirection::Next),
        (Mode::Browse, KeyCode::Char('?')) => UiAction::ToggleHelp,
        (Mode::Browse, KeyCode::Char('q')) => UiAction::Quit,
        (Mode::Command | Mode::Search, KeyCode::Enter) => UiAction::Submit,
        (Mode::FragmentFind, KeyCode::Enter) => UiAction::FindNext,
        (Mode::FragmentEdit, KeyCode::Enter) => UiAction::InsertText("\n".into()),
        (Mode::FragmentEdit, KeyCode::Up) => UiAction::MoveEditorCursor(EditorMove::Up),
        (Mode::FragmentEdit, KeyCode::Down) => UiAction::MoveEditorCursor(EditorMove::Down),
        (Mode::FragmentEdit, KeyCode::Left) => UiAction::MoveEditorCursor(EditorMove::Left),
        (Mode::FragmentEdit, KeyCode::Right) => UiAction::MoveEditorCursor(EditorMove::Right),
        (Mode::FragmentEdit, KeyCode::Home) => UiAction::MoveEditorCursor(EditorMove::LineStart),
        (Mode::FragmentEdit, KeyCode::End) => UiAction::MoveEditorCursor(EditorMove::LineEnd),
        (
            Mode::Command
            | Mode::Search
            | Mode::FragmentEdit
            | Mode::FragmentFind
            | Mode::MetadataEdit,
            KeyCode::Backspace,
        ) => UiAction::DeleteBackward,
        (
            Mode::Command
            | Mode::Search
            | Mode::FragmentEdit
            | Mode::FragmentFind
            | Mode::MetadataEdit,
            KeyCode::Char(character),
        ) => UiAction::InsertText(character.to_string()),
        _ => UiAction::Noop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_is_ignored_and_repeat_is_accepted() {
        let mapper = InputMapper::new();
        assert_eq!(
            mapper.map(
                Event::Key(KeyEvent::new_with_kind(
                    KeyCode::Char('j'),
                    KeyModifiers::NONE,
                    KeyEventKind::Release
                )),
                Mode::Browse
            ),
            None
        );
        assert_eq!(
            mapper.map(
                Event::Key(KeyEvent::new_with_kind(
                    KeyCode::Char('j'),
                    KeyModifiers::NONE,
                    KeyEventKind::Repeat
                )),
                Mode::Browse
            ),
            Some(UiAction::MoveSelection(MoveDirection::Next))
        );
    }

    #[test]
    fn focus_gain_checks_external_changes_immediately() {
        assert_eq!(
            InputMapper::new().map(Event::FocusGained, Mode::Browse),
            Some(UiAction::CheckExternalChanges)
        );
        assert_eq!(
            InputMapper::new().map(Event::FocusLost, Mode::Browse),
            Some(UiAction::Noop)
        );
    }

    #[test]
    fn paste_inserts_but_never_submits() {
        let mapper = InputMapper::new();
        assert_eq!(
            mapper.map(Event::Paste("x\ny".into()), Mode::Command),
            Some(UiAction::InsertText("x\ny".into()))
        );
    }

    #[test]
    fn mouse_and_keyboard_have_action_parity() {
        let mut mapper = InputMapper::new();
        mapper.set_regions(vec![HitRegion {
            rect: Rect::new(0, 0, 4, 1),
            target: HitTarget::OpenSelected,
        }]);
        let mouse = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        let key = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            mapper.map(mouse, Mode::Browse),
            mapper.map(key, Mode::Browse)
        );
    }

    #[test]
    fn specific_row_target_wins_over_its_parent_pane() {
        let mut mapper = InputMapper::new();
        mapper.set_regions(vec![
            HitRegion {
                rect: Rect::new(1, 1, 8, 1),
                target: HitTarget::Node(4),
            },
            HitRegion {
                rect: Rect::new(0, 0, 10, 10),
                target: HitTarget::Pane(Pane::Catalog),
            },
        ]);
        let mouse = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            mapper.map(mouse, Mode::Browse),
            Some(UiAction::SelectNode(4))
        );
    }
}
