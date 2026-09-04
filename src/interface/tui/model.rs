//! Elm 风格的确定性状态机。 / Elm-style deterministic state machine.

use ratatui::layout::Rect;
use ratatui_textarea::{CursorMove, Scrolling, TextArea};

use crate::application::NodeView;
use crate::infrastructure::editor::EditTarget;
use crate::{application::CanonicalXml, diagnostic::Diagnostic};

use super::{
    action::{EditorMove, MoveDirection, UiAction},
    layout::{Layout, LayoutClass},
};

/// 可聚焦窗格。 / Focusable pane.
///
/// <!-- @brief 可聚焦窗格。 / Focusable pane. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// 节点目录窗格。 / Node catalog pane.
    Catalog,
    /// 类型化预览窗格。 / Typed preview pane.
    Preview,
    /// 节点元数据窗格。 / Node metadata pane.
    Metadata,
}

/// 预览投影标签。 / Preview projection tab.
///
/// <!-- @brief 预览投影标签。 / Preview projection tab. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewTab {
    /// 命名图的有界树投影。 / Bounded tree projection of the named graph.
    Tree,
    /// 规范 XML 投影。 / Canonical XML projection.
    Xml,
    /// Fragment 文本内容投影。 / Fragment text-content projection.
    Content,
    /// 节点元数据投影。 / Node metadata projection.
    Metadata,
}

impl PreviewTab {
    fn cycle(self, direction: MoveDirection) -> Self {
        let index = match self {
            Self::Tree => 0,
            Self::Xml => 1,
            Self::Content => 2,
            Self::Metadata => 3,
        };
        let next = match direction {
            MoveDirection::Next => (index + 1) % 4,
            MoveDirection::Previous => (index + 3) % 4,
        };
        [Self::Tree, Self::Xml, Self::Content, Self::Metadata][next]
    }
}

/// 待确认操作类型。 / Kind of operation awaiting confirmation.
///
/// <!-- @brief 待确认操作类型。 / Kind of operation awaiting confirmation. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    /// 丢弃已修改的 Fragment 草稿。 / Discard a modified Fragment draft.
    DiscardFragmentDraft,
    /// 丢弃已修改的元数据草稿。 / Discard a modified metadata draft.
    DiscardMetadataDraft,
    /// 删除当前节点。 / Delete the current node.
    DeleteNode,
    /// 带未保存草稿退出。 / Quit with an unsaved draft.
    QuitWithDraft,
}

/// 当前交互模式。 / Current interaction mode.
///
/// <!-- @brief 当前交互模式。 / Current interaction mode. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 浏览目录和预览。 / Browse the catalog and previews.
    Browse,
    /// 输入 DSL 命令。 / Enter a DSL command.
    Command,
    /// 输入增量搜索查询。 / Enter an incremental search query.
    Search,
    /// 编辑 Fragment 正文。 / Edit Fragment content.
    FragmentEdit,
    /// 在 Fragment 正文中查找。 / Find within Fragment content.
    FragmentFind,
    /// 编辑节点元数据。 / Edit node metadata.
    MetadataEdit,
    /// 等待指定破坏性操作的确认。 / Await confirmation of the given destructive operation.
    Confirm(Confirmation),
    /// 显示帮助覆盖层。 / Display the help overlay.
    Help,
}

/// 未持有存储引用的编辑草稿。 / Edit draft that owns no store reference.
///
/// <!-- @brief 未持有存储引用的编辑草稿。 / Edit draft that owns no store reference. -->
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Draft {
    /// 打开时的文本。 / Text when opened.
    pub original: String,
    /// 当前暂存文本。 / Current staged text.
    pub text: String,
    /// 编辑开始时的持久化身份。 / Persistent identity captured when editing began.
    pub target: Option<EditTarget>,
    /// 编辑开始时的符号。 / Symbol captured when editing began.
    pub symbol: Option<String>,
}

/// 由 ratatui-textarea 驱动的拥有权编辑器状态。 / Owned editor state driven by ratatui-textarea.
///
/// <!-- @brief 由 ratatui-textarea 驱动的拥有权编辑器状态。 / Owned editor state driven by ratatui-textarea. -->
#[derive(Clone, Debug)]
pub struct EditorState {
    original: String,
    area: TextArea<'static>,
}

impl PartialEq for EditorState {
    fn eq(&self, other: &Self) -> bool {
        self.original == other.original
            && self.area.lines() == other.area.lines()
            && self.area.cursor() == other.area.cursor()
    }
}
impl Eq for EditorState {}

impl EditorState {
    /// 从原始文本建立编辑器。 / Build an editor from original text.
    ///
    /// <!-- @brief 从原始文本建立编辑器。 / Build an editor from original text. -->
    pub fn new(text: String) -> Self {
        let lines = text.split('\n').map(str::to_owned).collect();
        Self {
            original: text,
            area: TextArea::new(lines),
        }
    }

    /// 返回未修改的原文。 / Return the unmodified source text.
    ///
    /// <!-- @brief 返回未修改的原文。 / Return the unmodified source text. -->
    pub fn original(&self) -> &str {
        &self.original
    }
    /// 返回当前精确文本。 / Return the current exact text.
    ///
    /// <!-- @brief 返回当前精确文本。 / Return the current exact text. -->
    pub fn text(&self) -> String {
        self.area.lines().join("\n")
    }
    /// 返回可由视图克隆并配置的文本区。 / Return a textarea clone configurable by the view.
    ///
    /// <!-- @brief 返回可由视图克隆并配置的文本区。 / Return a textarea clone configurable by the view. -->
    pub fn textarea(&self) -> TextArea<'static> {
        self.area.clone()
    }
    fn insert(&mut self, text: &str) {
        self.area.insert_str(text);
    }
    fn backspace(&mut self) {
        self.area.delete_char();
    }
    fn undo(&mut self) {
        self.area.undo();
    }
    fn redo(&mut self) {
        self.area.redo();
    }
    fn scroll(&mut self, rows: i16) {
        self.area.scroll(Scrolling::Delta { rows, cols: 0 });
    }
    fn move_cursor(&mut self, movement: EditorMove) {
        self.area.move_cursor(match movement {
            EditorMove::Up => CursorMove::Up,
            EditorMove::Down => CursorMove::Down,
            EditorMove::Left => CursorMove::Back,
            EditorMove::Right => CursorMove::Forward,
            EditorMove::WordLeft => CursorMove::WordBack,
            EditorMove::WordRight => CursorMove::WordForward,
            EditorMove::LineStart => CursorMove::Head,
            EditorMove::LineEnd => CursorMove::End,
        });
    }
    fn set_find(&mut self, query: &str) {
        let _ = self.area.set_search_pattern(regex::escape(query));
    }
    fn find_next(&mut self) {
        self.area.search_forward(false);
    }
}

/// 当前预览的类型化宿主投影。 / Typed host projection for the current preview.
///
/// <!-- @brief 当前预览的类型化宿主投影。 / Typed host projection for the current preview. -->
#[derive(Clone, Debug, PartialEq)]
pub enum PreviewPayload {
    /// 已布局的出现树行。 / Pre-laid-out occurrence-tree lines.
    Tree(Vec<String>),
    /// 规范 XML 及其有界文本预览。 / Canonical XML and its bounded text preview.
    Xml {
        /// 可流式读取的完整规范 XML。 / Complete streamable canonical XML.
        value: CanonicalXml,
        /// 适合终端渲染的有界前缀。 / Bounded prefix suitable for terminal rendering.
        preview: String,
        /// 预览是否省略了剩余字节。 / Whether the preview omits remaining bytes.
        truncated: bool,
    },
    /// Fragment 正文的有界投影。 / Bounded projection of Fragment content.
    Content {
        /// 适合终端渲染的文本。 / Text suitable for terminal rendering.
        text: String,
        /// 投影是否省略了剩余文本。 / Whether the projection omits remaining text.
        truncated: bool,
    },
    /// 完整节点视图用于元数据展示。 / Complete node view for metadata display.
    Metadata(NodeView),
}

impl Draft {
    /// 草稿是否被修改。 / Whether the draft has changed.
    ///
    /// <!-- @brief 草稿是否被修改。 / Whether the draft has changed. -->
    pub fn is_dirty(&self) -> bool {
        self.text != self.original
    }
}

/// reducer 请求宿主执行的外部效果。 / External effect requested from the host by the reducer.
///
/// <!-- @brief reducer 请求宿主执行的外部效果。 / External effect requested from the host by the reducer. -->
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// 执行 DSL 语句。 / Execute a DSL statement.
    Execute(String),
    /// 执行增量搜索。 / Execute an incremental search.
    Search(String),
    /// 保存 Fragment 草稿。 / Save a Fragment draft.
    SaveFragment(Draft),
    /// 从共享运行时加载 Fragment 原文草稿。 / Load original Fragment text through the shared runtime.
    LoadFragmentDraft(String),
    /// 从共享运行时加载现有描述草稿。 / Load the existing description draft through the shared runtime.
    LoadMetadataDraft(String),
    /// 保存元数据草稿。 / Save a metadata draft.
    SaveMetadata(Draft),
    /// 删除当前符号。 / Delete the current symbol.
    Delete(String),
    /// 加载或刷新预览。 / Load or refresh a preview.
    LoadPreview {
        /// 要投影的节点符号。 / Symbol of the node to project.
        symbol: String,
        /// 要生成的预览投影。 / Preview projection to generate.
        tab: PreviewTab,
    },
    /// 获取可复制的规范 XML 类型值。 / Fetch a typed canonical XML value for copying.
    CopyCanonicalXml(String),
    /// 比较持久化适配器的变化令牌。 / Compare the persistence adapter's change token.
    CheckExternalChanges,
    /// 退出事件循环。 / Exit the event loop.
    Exit,
}

/// TUI 的全部可序列化会话状态。 / Complete serializable-style TUI session state.
///
/// <!-- @brief TUI 的全部可序列化会话状态。 / Complete serializable-style TUI session state. -->
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// 当前交互模式。 / Current interaction mode.
    ///
    /// <!-- @brief 当前交互模式。 / Current interaction mode. -->
    pub mode: Mode,
    /// 当前聚焦窗格。 / Currently focused pane.
    ///
    /// <!-- @brief 当前聚焦窗格。 / Currently focused pane. -->
    pub focus: Pane,
    /// 当前预览标签。 / Current preview tab.
    ///
    /// <!-- @brief 当前预览标签。 / Current preview tab. -->
    pub preview_tab: PreviewTab,
    /// 最近一次终端矩形。 / Most recent terminal rectangle.
    ///
    /// <!-- @brief 最近一次终端矩形。 / Most recent terminal rectangle. -->
    pub terminal: Rect,
    /// 派生的响应式布局等级。 / Derived responsive layout class.
    ///
    /// <!-- @brief 派生的响应式布局等级。 / Derived responsive layout class. -->
    pub layout_class: LayoutClass,
    /// 有序目录符号投影。 / Ordered catalog symbol projection.
    ///
    /// <!-- @brief 有序目录符号投影。 / Ordered catalog symbol projection. -->
    pub catalog: Vec<String>,
    /// 目录中的选中索引。 / Selected catalog index.
    ///
    /// <!-- @brief 目录中的选中索引。 / Selected catalog index. -->
    pub selected: Option<usize>,
    /// 目录首个可见行索引。 / Index of the first visible catalog row.
    ///
    /// <!-- @brief 目录首个可见行索引。 / Index of the first visible catalog row. -->
    pub scroll: usize,
    /// 命令或搜索的暂存输入。 / Staged command or search input.
    ///
    /// <!-- @brief 命令或搜索的暂存输入。 / Staged command or search input. -->
    pub input: String,
    /// 可选编辑草稿。 / Optional editing draft.
    ///
    /// <!-- @brief 可选编辑草稿。 / Optional editing draft. -->
    pub draft: Option<Draft>,
    /// Fragment 编辑时的真实多行编辑器。 / Real multiline editor used for Fragment editing.
    ///
    /// <!-- @brief Fragment 编辑时的真实多行编辑器。 / Real multiline editor used for Fragment editing. -->
    pub editor: Option<EditorState>,
    /// 当前类型化预览。 / Current typed preview.
    ///
    /// <!-- @brief 当前类型化预览。 / Current typed preview. -->
    pub preview: Option<PreviewPayload>,
    /// 与预览分离的结构化诊断。 / Structured diagnostic kept separate from preview content.
    ///
    /// <!-- @brief 与预览分离的结构化诊断。 / Structured diagnostic kept separate from preview content. -->
    pub preview_diagnostic: Option<Diagnostic>,
    /// 每个预览标签记忆的滚动位置。 / Remembered scroll offset for each preview tab.
    ///
    /// <!-- @brief 每个预览标签记忆的滚动位置。 / Remembered scroll offset for each preview tab. -->
    pub preview_scroll: [usize; 4],
    /// LIST 返回的节点投影，用于有界树展开。 / LIST node projections used for bounded tree expansion.
    ///
    /// <!-- @brief LIST 返回的节点投影，用于有界树展开。 / LIST node projections used for bounded tree expansion. -->
    pub nodes: Vec<NodeView>,
    /// 最近观察到的连接局部变化令牌。 / Most recently observed connection-local change token.
    ///
    /// <!-- @brief 最近观察到的连接局部变化令牌。 / Most recently observed connection-local change token. -->
    pub change_token: Option<u64>,
    /// 宿主是否应结束事件循环。 / Whether the host should end its event loop.
    ///
    /// <!-- @brief 宿主是否应结束事件循环。 / Whether the host should end its event loop. -->
    pub should_quit: bool,
    /// 最近一条可观察状态消息。 / Most recent observable status message.
    ///
    /// <!-- @brief 最近一条可观察状态消息。 / Most recent observable status message. -->
    pub notice: Option<String>,
}

impl Default for Model {
    fn default() -> Self {
        Self::new(80, 24)
    }
}

impl Model {
    /// 创建可用于任意终端尺寸的空模型。 / Create an empty model for any terminal size.
    ///
    /// <!-- @brief 创建可用于任意终端尺寸的空模型。 / Create an empty model for any terminal size. -->
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            mode: Mode::Browse,
            focus: Pane::Catalog,
            preview_tab: PreviewTab::Tree,
            terminal: Rect::new(0, 0, width, height),
            layout_class: LayoutClass::for_size(width, height),
            catalog: Vec::new(),
            selected: None,
            scroll: 0,
            input: String::new(),
            draft: None,
            editor: None,
            preview: None,
            preview_diagnostic: None,
            preview_scroll: [0; 4],
            nodes: Vec::new(),
            change_token: None,
            should_quit: false,
            notice: None,
        }
    }

    /// 安装可恢复的编辑草稿。 / Install a recoverable edit draft.
    ///
    /// <!-- @brief 安装可恢复的编辑草稿。 / Install a recoverable edit draft. -->
    pub fn set_draft(&mut self, original: String, text: String) {
        self.draft = Some(Draft {
            original: original.clone(),
            text: text.clone(),
            target: None,
            symbol: None,
        });
        self.editor = (self.mode == Mode::FragmentEdit).then(|| EditorState::new(text));
    }

    fn sync_draft(&mut self) {
        if let (Some(editor), Some(draft)) = (&self.editor, &mut self.draft) {
            draft.text = editor.text();
        }
    }

    /// 返回当前选中符号。 / Return the currently selected symbol.
    ///
    /// <!-- @brief 返回当前选中符号。 / Return the currently selected symbol. -->
    pub fn selected_symbol(&self) -> Option<&str> {
        self.selected
            .and_then(|index| self.catalog.get(index))
            .map(String::as_str)
    }

    fn viewport_rows(&self) -> usize {
        Layout::compute(self.terminal)
            .content
            .height
            .saturating_sub(2)
            .max(1) as usize
    }

    fn clamp_navigation(&mut self) {
        self.selected = if self.catalog.is_empty() {
            None
        } else {
            Some(self.selected.unwrap_or(0).min(self.catalog.len() - 1))
        };
        let max_scroll = self.catalog.len().saturating_sub(self.viewport_rows());
        self.scroll = self.scroll.min(max_scroll);
        if let Some(selected) = self.selected {
            if selected < self.scroll {
                self.scroll = selected;
            }
            let rows = self.viewport_rows();
            if selected >= self.scroll + rows {
                self.scroll = selected + 1 - rows;
            }
        }
    }
}

/// 以纯函数方式应用动作并产生效果。 / Apply an action as a pure function and produce effects.
///
/// # Arguments / 参数
///
/// - `model` — 旧模型，按值传入以阻止隐式共享状态。 / Old model passed by value to prevent implicit shared state.
/// - `action` — 设备无关动作。 / Device-independent action.
///
/// # Returns / 返回值
///
/// 新模型与待执行效果。 / New model and effects to execute.
///
/// <!-- @brief 以纯函数方式应用动作并产生效果。 / Apply an action as a pure function and produce effects. -->
/// <!-- @param model 旧模型，按值传入以阻止隐式共享状态。 / Old model passed by value to prevent implicit shared state. -->
/// <!-- @param action 设备无关动作。 / Device-independent action. -->
/// <!-- @return 新模型与待执行效果。 / New model and effects to execute. -->
pub fn update(mut model: Model, action: UiAction) -> (Model, Vec<Effect>) {
    let mut effects = Vec::new();
    match action {
        UiAction::Resize { width, height } => {
            model.terminal.width = width;
            model.terminal.height = height;
            model.layout_class = LayoutClass::for_size(width, height);
            model.clamp_navigation();
        }
        UiAction::CheckExternalChanges => effects.push(Effect::CheckExternalChanges),
        UiAction::MoveSelection(direction) if model.mode == Mode::Browse => {
            if !model.catalog.is_empty() {
                let current = model.selected.unwrap_or(0);
                model.selected = Some(match direction {
                    MoveDirection::Previous => current.saturating_sub(1),
                    MoveDirection::Next => (current + 1).min(model.catalog.len() - 1),
                });
                model.clamp_navigation();
                if let Some(symbol) = model.selected_symbol() {
                    effects.push(Effect::LoadPreview {
                        symbol: symbol.to_owned(),
                        tab: model.preview_tab,
                    });
                }
            }
        }
        UiAction::ScrollPane(delta) => {
            if model.mode == Mode::FragmentEdit {
                if let Some(editor) = &mut model.editor {
                    editor.scroll(delta);
                }
                model.sync_draft();
                return (model, effects);
            }
            if model.focus == Pane::Preview {
                let index = match model.preview_tab {
                    PreviewTab::Tree => 0,
                    PreviewTab::Xml => 1,
                    PreviewTab::Content => 2,
                    PreviewTab::Metadata => 3,
                };
                model.preview_scroll[index] = if delta < 0 {
                    model.preview_scroll[index].saturating_sub(delta.unsigned_abs() as usize)
                } else {
                    model.preview_scroll[index].saturating_add(delta as usize)
                };
                return (model, effects);
            }
            let max = model.catalog.len().saturating_sub(model.viewport_rows());
            model.scroll = if delta < 0 {
                model.scroll.saturating_sub(delta.unsigned_abs() as usize)
            } else {
                model.scroll.saturating_add(delta as usize).min(max)
            };
        }
        UiAction::FocusPane(pane) => model.focus = pane,
        UiAction::SelectNode(index) if index < model.catalog.len() => {
            model.selected = Some(index);
            model.clamp_navigation();
            if let Some(symbol) = model.selected_symbol() {
                effects.push(Effect::LoadPreview {
                    symbol: symbol.to_owned(),
                    tab: model.preview_tab,
                });
            }
        }
        UiAction::SelectPreview(tab) => {
            model.preview_tab = tab;
            if let Some(symbol) = model.selected_symbol() {
                effects.push(Effect::LoadPreview {
                    symbol: symbol.to_owned(),
                    tab,
                });
            }
        }
        UiAction::CyclePreview(direction) => {
            model.preview_tab = model.preview_tab.cycle(direction);
            if let Some(symbol) = model.selected_symbol() {
                effects.push(Effect::LoadPreview {
                    symbol: symbol.to_owned(),
                    tab: model.preview_tab,
                });
            }
        }
        UiAction::StartCommand => {
            model.mode = Mode::Command;
            model.input.clear();
        }
        UiAction::StartSearch => {
            model.mode = Mode::Search;
            model.input.clear();
        }
        UiAction::StartFragmentEdit => {
            model.mode = Mode::FragmentEdit;
            model.set_draft(String::new(), String::new());
            if let Some(symbol) = model.selected_symbol() {
                effects.push(Effect::LoadFragmentDraft(symbol.to_owned()));
            }
        }
        UiAction::StartMetadataEdit => {
            model.mode = Mode::MetadataEdit;
            model.draft.get_or_insert_with(Draft::default);
            if let Some(symbol) = model.selected_symbol() {
                effects.push(Effect::LoadMetadataDraft(symbol.to_owned()));
            }
        }
        UiAction::StartRename => {
            if let Some(symbol) = model.selected_symbol().map(str::to_owned) {
                model.mode = Mode::Command;
                model.input = format!("RENAME {symbol} TO ");
            }
        }
        UiAction::RequestDelete if model.selected_symbol().is_some() => {
            model.mode = Mode::Confirm(Confirmation::DeleteNode)
        }
        UiAction::CopyCanonicalXml => {
            if let Some(symbol) = model.selected_symbol() {
                effects.push(Effect::CopyCanonicalXml(symbol.to_owned()));
            }
        }
        UiAction::InsertText(text) => match model.mode {
            Mode::FragmentEdit => {
                model
                    .editor
                    .get_or_insert_with(|| EditorState::new(String::new()))
                    .insert(&text);
                model.sync_draft();
            }
            Mode::MetadataEdit => model
                .draft
                .get_or_insert_with(Draft::default)
                .text
                .push_str(&text),
            Mode::Command | Mode::Search | Mode::FragmentFind => {
                model.input.push_str(&text);
                if model.mode == Mode::Search {
                    effects.push(Effect::Search(model.input.clone()));
                }
                if model.mode == Mode::FragmentFind
                    && let Some(editor) = &mut model.editor
                {
                    editor.set_find(&model.input);
                }
            }
            _ => {}
        },
        UiAction::DeleteBackward => match model.mode {
            Mode::FragmentEdit => {
                if let Some(editor) = &mut model.editor {
                    editor.backspace();
                }
                model.sync_draft();
            }
            Mode::MetadataEdit => {
                if let Some(draft) = &mut model.draft {
                    draft.text.pop();
                }
            }
            Mode::Command | Mode::Search | Mode::FragmentFind => {
                model.input.pop();
                if model.mode == Mode::Search {
                    effects.push(Effect::Search(model.input.clone()));
                }
                if model.mode == Mode::FragmentFind
                    && let Some(editor) = &mut model.editor
                {
                    editor.set_find(&model.input);
                }
            }
            _ => {}
        },
        UiAction::Submit => match model.mode {
            Mode::Command if !model.input.is_empty() => {
                effects.push(Effect::Execute(std::mem::take(&mut model.input)));
                model.mode = Mode::Browse;
            }
            Mode::Search if !model.input.is_empty() => {
                effects.push(Effect::Search(std::mem::take(&mut model.input)));
                model.mode = Mode::Browse;
            }
            Mode::FragmentEdit => {
                model.sync_draft();
                if let Some(draft) = &model.draft {
                    effects.push(Effect::SaveFragment(draft.clone()));
                    model.mode = Mode::Browse;
                }
            }
            Mode::MetadataEdit => {
                if let Some(draft) = &model.draft {
                    effects.push(Effect::SaveMetadata(draft.clone()));
                    model.mode = Mode::Browse;
                }
            }
            _ => {}
        },
        UiAction::Cancel => match model.mode {
            Mode::FragmentEdit if model.draft.as_ref().is_some_and(Draft::is_dirty) => {
                model.mode = Mode::Confirm(Confirmation::DiscardFragmentDraft)
            }
            Mode::MetadataEdit if model.draft.as_ref().is_some_and(Draft::is_dirty) => {
                model.mode = Mode::Confirm(Confirmation::DiscardMetadataDraft)
            }
            Mode::FragmentEdit | Mode::MetadataEdit => {
                model.draft = None;
                model.editor = None;
                model.mode = Mode::Browse;
            }
            Mode::FragmentFind => {
                model.input.clear();
                model.mode = Mode::FragmentEdit;
            }
            Mode::Confirm(_) | Mode::Command | Mode::Search | Mode::Help => {
                model.input.clear();
                model.mode = Mode::Browse;
            }
            Mode::Browse => {}
        },
        UiAction::Confirm => match model.mode {
            Mode::Confirm(
                Confirmation::DiscardFragmentDraft | Confirmation::DiscardMetadataDraft,
            ) => {
                model.draft = None;
                model.editor = None;
                model.mode = Mode::Browse;
            }
            Mode::Confirm(Confirmation::DeleteNode) => {
                if let Some(symbol) = model.selected_symbol() {
                    effects.push(Effect::Delete(symbol.to_owned()));
                }
                model.mode = Mode::Browse;
            }
            Mode::Confirm(Confirmation::QuitWithDraft) => {
                model.should_quit = true;
                effects.push(Effect::Exit);
            }
            _ => {}
        },
        UiAction::Reject => match model.mode {
            Mode::Confirm(Confirmation::DiscardFragmentDraft) => model.mode = Mode::FragmentEdit,
            Mode::Confirm(Confirmation::DiscardMetadataDraft) => model.mode = Mode::MetadataEdit,
            Mode::Confirm(_) => model.mode = Mode::Browse,
            _ => {}
        },
        UiAction::ToggleHelp => {
            model.mode = if model.mode == Mode::Help {
                Mode::Browse
            } else {
                Mode::Help
            }
        }
        UiAction::Quit => {
            if model.draft.as_ref().is_some_and(Draft::is_dirty) {
                model.mode = Mode::Confirm(Confirmation::QuitWithDraft);
            } else {
                model.should_quit = true;
                effects.push(Effect::Exit);
            }
        }
        UiAction::OpenSelected => {
            if let Some(symbol) = model.selected_symbol() {
                effects.push(Effect::LoadPreview {
                    symbol: symbol.to_owned(),
                    tab: model.preview_tab,
                });
            }
        }
        UiAction::MoveEditorCursor(movement) if model.mode == Mode::FragmentEdit => {
            if let Some(editor) = &mut model.editor {
                editor.move_cursor(movement);
            }
            model.sync_draft();
        }
        UiAction::UndoEditor if model.mode == Mode::FragmentEdit => {
            if let Some(editor) = &mut model.editor {
                editor.undo();
            }
            model.sync_draft();
        }
        UiAction::RedoEditor if model.mode == Mode::FragmentEdit => {
            if let Some(editor) = &mut model.editor {
                editor.redo();
            }
            model.sync_draft();
        }
        UiAction::StartEditorFind if model.mode == Mode::FragmentEdit => {
            model.mode = Mode::FragmentFind;
            model.input.clear();
        }
        UiAction::FindNext if model.mode == Mode::FragmentFind => {
            if let Some(editor) = &mut model.editor {
                editor.find_next();
            }
        }
        UiAction::Noop
        | UiAction::RequestDelete
        | UiAction::MoveSelection(_)
        | UiAction::SelectNode(_) => {}
        UiAction::MoveEditorCursor(_)
        | UiAction::UndoEditor
        | UiAction::RedoEditor
        | UiAction::StartEditorFind
        | UiAction::FindNext => {}
    }
    (model, effects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_draft_cancel_requires_confirmation() {
        let model = Model {
            mode: Mode::FragmentEdit,
            draft: Some(Draft {
                original: "old".into(),
                text: "new".into(),
                ..Default::default()
            }),
            ..Model::default()
        };
        let (model, effects) = update(model, UiAction::Cancel);
        assert_eq!(
            model.mode,
            Mode::Confirm(Confirmation::DiscardFragmentDraft)
        );
        assert!(model.draft.is_some());
        assert!(effects.is_empty());
    }

    #[test]
    fn resize_clamps_scroll_and_selection() {
        let mut model = Model::new(140, 40);
        model.catalog = (0..10).map(|n| n.to_string()).collect();
        model.selected = Some(99);
        model.scroll = 99;
        let (model, _) = update(
            model,
            UiAction::Resize {
                width: 2,
                height: 1,
            },
        );
        assert_eq!(model.selected, Some(9));
        assert!(model.scroll <= 9);
        assert_eq!(model.layout_class, LayoutClass::Focused);
    }

    #[test]
    fn fragment_editor_uses_cursor_and_undo_not_append_only_text() {
        let mut model = Model {
            mode: Mode::FragmentEdit,
            ..Model::default()
        };
        model.set_draft("ab".into(), "ab".into());
        let (model, _) = update(model, UiAction::MoveEditorCursor(EditorMove::Right));
        let (model, _) = update(model, UiAction::InsertText("X".into()));
        assert_eq!(model.draft.as_ref().unwrap().text, "aXb");
        let (model, _) = update(model, UiAction::UndoEditor);
        assert_eq!(model.draft.as_ref().unwrap().text, "ab");
    }

    #[test]
    fn search_changes_request_incremental_results_and_paste_does_not_submit() {
        let (model, _) = update(Model::default(), UiAction::StartSearch);
        let (model, effects) = update(model, UiAction::InsertText("needle\ntext".into()));
        assert_eq!(model.mode, Mode::Search);
        assert_eq!(model.input, "needle\ntext");
        assert_eq!(effects, vec![Effect::Search("needle\ntext".into())]);
    }

    #[test]
    fn preview_scroll_is_remembered_per_tab() {
        let model = Model {
            focus: Pane::Preview,
            ..Model::default()
        };
        let (model, _) = update(model, UiAction::ScrollPane(4));
        let (model, _) = update(model, UiAction::SelectPreview(PreviewTab::Xml));
        let (model, _) = update(model, UiAction::ScrollPane(2));
        assert_eq!(model.preview_scroll, [4, 2, 0, 0]);
    }

    #[test]
    fn fragment_find_uses_textarea_search_and_returns_to_editor() {
        let mut model = Model {
            mode: Mode::FragmentEdit,
            ..Model::default()
        };
        model.set_draft("one two one".into(), "one two one".into());
        let (model, _) = update(model, UiAction::StartEditorFind);
        let (model, _) = update(model, UiAction::InsertText("one".into()));
        let before = model.editor.as_ref().unwrap().area.cursor();
        let (model, _) = update(model, UiAction::FindNext);
        assert_ne!(model.editor.as_ref().unwrap().area.cursor(), before);
        let (model, _) = update(model, UiAction::Cancel);
        assert_eq!(model.mode, Mode::FragmentEdit);
        assert!(model.draft.is_some());
    }

    #[test]
    fn external_change_check_is_an_explicit_effect() {
        let (model, effects) = update(Model::default(), UiAction::CheckExternalChanges);
        assert_eq!(effects, vec![Effect::CheckExternalChanges]);
        assert_eq!(model.mode, Mode::Browse);
    }
}
