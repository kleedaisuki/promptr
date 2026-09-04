//! 与输入设备无关的用户意图。 / Input-device-independent user intentions.

use super::model::{Pane, PreviewTab};

/// 移动方向。 / Movement direction.
///
/// <!-- @brief 移动方向。 / Movement direction. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDirection {
    /// 向前或向上。 / Previous or up.
    Previous,
    /// 向后或向下。 / Next or down.
    Next,
}

/// 键盘和鼠标最终汇聚到的用户动作。 / User action shared by keyboard and mouse.
///
/// <!-- @brief 键盘和鼠标最终汇聚到的用户动作。 / User action shared by keyboard and mouse. -->
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    /// 移动目录选择。 / Move catalog selection.
    MoveSelection(MoveDirection),
    /// 按可见行滚动当前窗格。 / Scroll the active pane by visible rows.
    ScrollPane(i16),
    /// 聚焦指定窗格。 / Focus a pane.
    FocusPane(Pane),
    /// 按稳定列表索引选择节点。 / Select a node by stable list index.
    SelectNode(usize),
    /// 打开当前选择。 / Open the current selection.
    OpenSelected,
    /// 选择预览标签。 / Select a preview tab.
    SelectPreview(PreviewTab),
    /// 循环切换预览标签。 / Cycle preview tabs.
    CyclePreview(MoveDirection),
    /// 进入命令模式。 / Enter command mode.
    StartCommand,
    /// 进入搜索模式。 / Enter search mode.
    StartSearch,
    /// 开始编辑 Fragment 草稿。 / Start editing a Fragment draft.
    StartFragmentEdit,
    /// 开始编辑元数据。 / Start editing metadata.
    StartMetadataEdit,
    /// 通过命令输入开始重命名。 / Begin a rename through command input.
    StartRename,
    /// 请求删除当前节点。 / Request deletion of the selected node.
    RequestDelete,
    /// 请求规范 XML 的类型化复制输出。 / Request typed canonical-XML copy output.
    CopyCanonicalXml,
    /// 插入文本；粘贴也只产生此动作。 / Insert text; paste produces only this action.
    InsertText(String),
    /// 删除当前输入末尾的一个 Unicode 标量。 / Delete one Unicode scalar from the end of current input.
    DeleteBackward,
    /// 移动内置编辑器光标。 / Move the built-in editor cursor.
    MoveEditorCursor(EditorMove),
    /// 撤销内置编辑器修改。 / Undo a built-in editor change.
    UndoEditor,
    /// 重做内置编辑器修改。 / Redo a built-in editor change.
    RedoEditor,
    /// 打开 Fragment 内查找输入。 / Open find input inside a Fragment.
    StartEditorFind,
    /// 跳到下一个 Fragment 匹配。 / Move to the next Fragment match.
    FindNext,
    /// 提交当前输入或保存编辑。 / Submit input or save an edit.
    Submit,
    /// 取消当前模式。 / Cancel the current mode.
    Cancel,
    /// 接受确认。 / Accept a confirmation.
    Confirm,
    /// 拒绝确认。 / Reject a confirmation.
    Reject,
    /// 显示或隐藏帮助。 / Show or hide help.
    ToggleHelp,
    /// 请求退出。 / Request application exit.
    Quit,
    /// 更新终端大小。 / Update terminal size.
    Resize {
        /// 新终端宽度（列）。 / New terminal width in columns.
        width: u16,
        /// 新终端高度（行）。 / New terminal height in rows.
        height: u16,
    },
    /// 检查其他数据库连接是否提交了变化。 / Check whether another database connection committed changes.
    CheckExternalChanges,
    /// 无语义输入。 / Input with no semantic action.
    Noop,
}

/// 内置编辑器光标移动语义。 / Built-in editor cursor movement semantics.
///
/// <!-- @brief 内置编辑器光标移动语义。 / Built-in editor cursor movement semantics. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMove {
    /// 向上一行。 / Move one line up.
    Up,
    /// 向下一行。 / Move one line down.
    Down,
    /// 向左一个字符。 / Move one character left.
    Left,
    /// 向右一个字符。 / Move one character right.
    Right,
    /// 向左一个单词。 / Move one word left.
    WordLeft,
    /// 向右一个单词。 / Move one word right.
    WordRight,
    /// 移至当前行开头。 / Move to the start of the current line.
    LineStart,
    /// 移至当前行末尾。 / Move to the end of the current line.
    LineEnd,
}
