//! TUI 同步宿主与共享应用运行时的适配。 / Adapter between the synchronous TUI host and shared application runtime.

use std::{
    io,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crossterm::event;
use ratatui::style::Color;
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    application::{InvocationPolicy, NodePrecondition, Promptr, Value},
    diagnostic::{Diagnostic, DiagnosticCategory, Result},
    infrastructure::config::{
        ColorMode as ConfigColorMode, EditorMode, GlyphMode as ConfigGlyphMode,
    },
    infrastructure::editor::{
        EditRequest, EditTarget, EditorError, EditorOutcome, ExternalTextProvider, PreparedEdit,
        TextProvider,
    },
};

use super::{
    CustomPalette, Effect, GlyphMode, InputMapper, Model, PreviewTab, Theme,
    model::PreviewPayload,
    terminal::{CrosstermOps, TerminalOps, TerminalSession},
    update, view,
};

/// 可单元测试的运行时请求计划。 / Unit-testable runtime request plan.
///
/// <!-- @brief 可单元测试的运行时请求计划。 / Unit-testable runtime request plan. -->
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeRequest {
    /// 通过共享应用运行时求值 DSL。 / Evaluate DSL through the shared application runtime.
    Eval(String),
    /// 加载 Fragment 的正文与持久化身份。 / Load Fragment content and persistent identity.
    LoadFragmentDraft {
        /// 获取正文所需的 DSL。 / DSL used to fetch the content.
        source: String,
        /// 正在编辑的 Fragment 符号。 / Symbol of the Fragment being edited.
        symbol: String,
    },
    /// 加载元数据草稿所需的 DSL。 / DSL used to load a metadata draft.
    LoadMetadataDraft(String),
    /// 使用可恢复草稿编辑 Fragment。 / Edit a Fragment with a recoverable draft.
    EditFragment {
        /// 建立编辑上下文的 DSL。 / DSL that establishes the edit context.
        source: String,
        /// 要交给编辑器的拥有权草稿。 / Owned draft passed to the editor.
        draft: super::model::Draft,
    },
    /// 使用乐观并发前置条件编辑元数据。 / Edit metadata with an optimistic-concurrency precondition.
    EditMetadata {
        /// 要执行的元数据 DSL。 / Metadata DSL to execute.
        source: String,
        /// 编辑开始时捕获的持久化目标。 / Persistent target captured when editing began.
        target: crate::infrastructure::editor::EditTarget,
        /// 编辑开始时捕获的节点符号。 / Node symbol captured when editing began.
        symbol: String,
    },
    /// 结束宿主事件循环。 / End the host event loop.
    Exit,
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static SIGNAL_HANDLER: OnceLock<std::result::Result<(), String>> = OnceLock::new();

fn install_signal_handler() -> Result<()> {
    let installed = SIGNAL_HANDLER.get_or_init(|| {
        ctrlc::set_handler(|| INTERRUPTED.store(true, Ordering::Release))
            .map_err(|error| error.to_string())
    });
    installed
        .as_ref()
        .map_err(|cause| {
            Diagnostic::error(
                "E_TERMINAL_SIGNAL",
                DiagnosticCategory::External,
                "terminal signal handler installation failed",
            )
            .with_cause(cause.clone())
        })
        .copied()
}

/// 可替换的系统剪贴板边界。 / Replaceable system clipboard boundary.
///
/// <!-- @brief 可替换的系统剪贴板边界。 / Replaceable system clipboard boundary. -->
pub trait Clipboard {
    /// 写入完整 UTF-8 文本。 / Write complete UTF-8 text.
    ///
    /// <!-- @brief 写入完整 UTF-8 文本。 / Write complete UTF-8 text. -->
    ///
    /// # Errors
    ///
    /// 当平台剪贴板不可用或拒绝写入时返回后端错误文本。 /
    /// Returns the backend error text when the platform clipboard is unavailable or rejects the write.
    fn set_text(&mut self, text: String) -> std::result::Result<(), String>;
}

struct SystemClipboard(Option<arboard::Clipboard>);

impl Clipboard for SystemClipboard {
    fn set_text(&mut self, text: String) -> std::result::Result<(), String> {
        if self.0.is_none() {
            self.0 = Some(arboard::Clipboard::new().map_err(|error| error.to_string())?);
        }
        self.0
            .as_mut()
            .expect("initialized above")
            .set_text(text)
            .map_err(|error| error.to_string())
    }
}

/// 将 reducer 效果转换为唯一应用运行时请求。 / Convert a reducer effect into one application-runtime request.
///
/// # Arguments / 参数
///
/// - `effect` — reducer 产生的效果。 / Effect produced by the reducer.
/// - `selected` — 当前选中符号。 / Currently selected symbol.
///
/// # Returns / 返回值
///
/// 请求；纯 UI 预览也统一落到 DSL eval。 / Request; UI projections also converge on DSL eval.
///
/// <!-- @brief 将 reducer 效果转换为唯一应用运行时请求。 / Convert a reducer effect into one application-runtime request. -->
/// <!-- @param effect reducer 产生的效果。 / Effect produced by the reducer. -->
/// <!-- @param selected 当前选中符号。 / Currently selected symbol. -->
/// <!-- @return 请求；纯 UI 预览也统一落到 DSL eval。 / Request; UI projections also converge on DSL eval. -->
pub fn runtime_request(effect: Effect, _selected: Option<&str>) -> Option<RuntimeRequest> {
    match effect {
        Effect::Execute(source) => Some(RuntimeRequest::Eval(source)),
        Effect::Search(query) => Some(RuntimeRequest::Eval(format!("SEARCH {};", quote(&query)))),
        Effect::SaveFragment(draft) => {
            let symbol = draft.symbol.clone()?;
            Some(RuntimeRequest::EditFragment {
                source: format!("FRAGMENT {symbol};"),
                draft,
            })
        }
        Effect::LoadFragmentDraft(symbol) => Some(RuntimeRequest::LoadFragmentDraft {
            source: format!("PRINT {symbol}; OUTPUT {symbol};"),
            symbol,
        }),
        Effect::LoadMetadataDraft(symbol) => Some(RuntimeRequest::LoadMetadataDraft(format!(
            "PRINT {symbol};"
        ))),
        Effect::SaveMetadata(draft) => match (draft.symbol, draft.target) {
            (Some(symbol), Some(target)) => Some(RuntimeRequest::EditMetadata {
                source: format!("METADATA {symbol} DESCRIPTION {};", quote(&draft.text)),
                target,
                symbol,
            }),
            _ => None,
        },
        Effect::Delete(symbol) => Some(RuntimeRequest::Eval(format!("DELETE {symbol};"))),
        Effect::LoadPreview { symbol, tab } => Some(RuntimeRequest::Eval(match tab {
            PreviewTab::Xml | PreviewTab::Content => format!("PRINT {symbol}; OUTPUT {symbol};"),
            _ => format!("PRINT {symbol};"),
        })),
        Effect::CopyCanonicalXml(symbol) => Some(RuntimeRequest::Eval(format!("OUTPUT {symbol};"))),
        Effect::CheckExternalChanges => None,
        Effect::Exit => Some(RuntimeRequest::Exit),
    }
}

fn quote(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

struct OneShotProvider(Option<PreparedEdit>);

impl TextProvider for OneShotProvider {
    fn edit(&mut self, _request: EditRequest) -> std::result::Result<EditorOutcome, EditorError> {
        Ok(match self.0.take() {
            Some(edit) => EditorOutcome::Save(edit),
            None => EditorOutcome::Cancel,
        })
    }
}

/// 配置选择的 Fragment 编辑提供者类别。 / Fragment editor provider kind selected by configuration.
///
/// <!-- @brief 配置选择的 Fragment 编辑提供者类别。 / Fragment editor provider kind selected by configuration. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderRoute {
    Builtin,
    External,
}

fn provider_route(mode: EditorMode) -> ProviderRoute {
    match mode {
        EditorMode::Builtin => ProviderRoute::Builtin,
        EditorMode::External => ProviderRoute::External,
    }
}

struct SuspendedProvider<'a, O: TerminalOps, P: TextProvider> {
    session: &'a mut TerminalSession<O>,
    provider: P,
}

impl<O: TerminalOps, P: TextProvider> TextProvider for SuspendedProvider<'_, O, P> {
    fn edit(&mut self, request: EditRequest) -> std::result::Result<EditorOutcome, EditorError> {
        let provider = &mut self.provider;
        self.session
            .suspend(|| provider.edit(request))
            .map_err(|source| EditorError::Io {
                operation: "terminal suspension or resumption",
                source,
            })?
    }
}

struct DraftSeedProvider<P> {
    provider: P,
    request: Option<EditRequest>,
    original_text: String,
}

impl<P: TextProvider> TextProvider for DraftSeedProvider<P> {
    fn edit(&mut self, request: EditRequest) -> std::result::Result<EditorOutcome, EditorError> {
        let seeded = self.request.take().unwrap_or(request);
        let original_request = seeded.clone();
        Ok(match self.provider.edit(seeded)? {
            EditorOutcome::Save(edit) => EditorOutcome::Save(PreparedEdit {
                target: original_request.target,
                original_text: self.original_text.clone(),
                edited_text: edit.edited_text,
            }),
            EditorOutcome::Cancel => EditorOutcome::Cancel,
        })
    }
}

/// 在当前终端运行同步 TUI。 / Run the synchronous TUI in the current terminal.
///
/// # Arguments / 参数
///
/// - `app` — 已打开的共享应用门面。 / Open shared application facade.
///
/// # Returns / 返回值
///
/// 正常退出或结构化终端诊断。 / Normal exit or structured terminal diagnostic.
///
/// # Errors
///
/// 当信号处理器、终端生命周期、配置的编辑器或共享应用运行时失败时，返回结构化诊断。 /
/// Returns a structured diagnostic when signal handling, terminal lifecycle, the configured editor,
/// or the shared application runtime fails.
///
/// <!-- @brief 在当前终端运行同步 TUI。 / Run the synchronous TUI in the current terminal. -->
/// <!-- @param app 已打开的共享应用门面。 / Open shared application facade. -->
/// <!-- @return 正常退出或结构化终端诊断。 / Normal exit or structured terminal diagnostic. -->
pub fn run(app: &mut Promptr) -> Result<()> {
    install_signal_handler()?;
    INTERRUPTED.store(false, Ordering::Release);
    let mouse = app.config().ui.mouse;
    let (theme, glyph_mode) = terminal_capabilities(app);
    let mut session = TerminalSession::start(CrosstermOps, mouse).map_err(terminal_error)?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).map_err(terminal_error)?;
    terminal.clear().map_err(terminal_error)?;
    let size = terminal.size().map_err(terminal_error)?;
    let mut model = Model::new(size.width, size.height);
    model.change_token = app.change_token()?;
    model.preview_tab = configured_preview(app.config().ui.preview);
    apply_values(
        &mut model,
        app.eval("LIST;", InvocationPolicy::interactive())?,
    );
    if let Some(symbol) = model.selected_symbol().map(str::to_owned) {
        let values = app.eval(
            &preview_source(&symbol, model.preview_tab),
            InvocationPolicy::interactive(),
        )?;
        apply_preview_values(&mut model, values)?;
    }
    let mut clipboard = SystemClipboard(None);
    let mut mapper = InputMapper::new();
    let mut last_change_check = Instant::now();

    while !model.should_quit {
        if INTERRUPTED.load(Ordering::Acquire) {
            break;
        }
        let mut regions = Vec::new();
        terminal
            .draw(|frame| {
                regions = view::render(&model, frame.buffer_mut(), theme, glyph_mode);
            })
            .map_err(terminal_error)?;
        mapper.set_regions(regions);

        // 有界等待允许异步信号通过正常退出路径触发 RAII 恢复。
        // Bounded waiting lets asynchronous signals trigger RAII restoration through normal exit.
        if !event::poll(Duration::from_millis(200)).map_err(terminal_error)? {
            if last_change_check.elapsed() >= Duration::from_secs(1) {
                refresh_if_changed(app, &mut model)?;
                last_change_check = Instant::now();
            }
            continue;
        }
        let terminal_event = event::read().map_err(terminal_error)?;
        let Some(action) = mapper.map(terminal_event, model.mode) else {
            continue;
        };
        let (next, effects) = update(model, action);
        model = next;
        for effect in effects {
            let failed_draft = match &effect {
                Effect::SaveFragment(draft) => Some(draft.text.clone()),
                _ => None,
            };
            let failed_metadata = match &effect {
                Effect::SaveMetadata(draft) => Some(draft.text.clone()),
                _ => None,
            };
            let preview_effect = matches!(effect, Effect::LoadPreview { .. });
            if let Err(diagnostic) =
                execute_effect(app, &mut model, effect, &mut session, &mut clipboard)
            {
                if let Some(draft) = failed_draft {
                    restore_fragment_draft(&mut model, draft);
                }
                if let Some(draft) = failed_metadata {
                    restore_metadata_draft(&mut model, draft);
                }
                if preview_effect {
                    model.preview_diagnostic = Some(diagnostic.clone());
                }
                model.notice = Some(format!("{}: {}", diagnostic.code, diagnostic.message));
            }
            terminal.clear().map_err(terminal_error)?;
        }
    }
    terminal.show_cursor().map_err(terminal_error)?;
    session.restore().map_err(terminal_error)
}

fn terminal_capabilities(app: &Promptr) -> (Theme, GlyphMode) {
    let config = app.config();
    let dumb = std::env::var("TERM").is_ok_and(|term| term == "dumb");
    terminal_capabilities_from(config, std::env::var_os("NO_COLOR").is_some(), dumb)
}

/// 从配置与已探测环境计算终端能力。 / Compute terminal capabilities from config and already-probed environment.
///
/// # Arguments / 参数
///
/// - `config` — 已验证配置。 / Validated configuration.
/// - `no_color` — 是否存在 NO_COLOR。 / Whether NO_COLOR is present.
/// - `dumb` — TERM 是否为 dumb。 / Whether TERM is dumb.
///
/// # Returns / 返回值
///
/// 降级后的主题与字形。 / Degraded theme and glyphs.
///
/// <!-- @brief 从配置与已探测环境计算终端能力。 / Compute terminal capabilities from config and already-probed environment. -->
/// <!-- @param config 已验证配置。 / Validated configuration. -->
/// <!-- @param no_color 是否存在 NO_COLOR。 / Whether NO_COLOR is present. -->
/// <!-- @param dumb TERM 是否为 dumb。 / Whether TERM is dumb. -->
/// <!-- @return 降级后的主题与字形。 / Degraded theme and glyphs. -->
fn terminal_capabilities_from(
    config: &crate::infrastructure::config::Config,
    no_color: bool,
    dumb: bool,
) -> (Theme, GlyphMode) {
    let monochrome = config.ui.color == ConfigColorMode::None
        || config.ui.theme.eq_ignore_ascii_case("monochrome")
        || (config.ui.color == ConfigColorMode::Auto && no_color)
        || dumb;
    let theme = if monochrome {
        Theme::Monochrome
    } else if config.ui.theme.eq_ignore_ascii_case("light") {
        Theme::Light
    } else if let Some(theme) = config.themes.get(&config.ui.theme) {
        Theme::Custom(CustomPalette {
            surface: parse_color(&theme.surface),
            text: parse_color(&theme.text),
            muted: parse_color(&theme.muted),
            selection: parse_color(&theme.selection),
            fragment: parse_color(&theme.fragment),
            prompt: parse_color(&theme.prompt),
            tag: parse_color(&theme.tag),
            success: parse_color(&theme.success),
            warning: parse_color(&theme.warning),
            error: parse_color(&theme.error),
        })
    } else {
        Theme::Dark
    };
    let glyphs = if config.ui.glyphs == ConfigGlyphMode::Ascii || dumb {
        GlyphMode::Ascii
    } else {
        GlyphMode::Unicode
    };
    (theme, glyphs)
}

/// 将配置预览模式转换为 TUI 标签。 / Convert configured preview mode to a TUI tab.
///
/// <!-- @brief 将配置预览模式转换为 TUI 标签。 / Convert configured preview mode to a TUI tab. -->
fn configured_preview(preview: crate::infrastructure::config::PreviewMode) -> PreviewTab {
    match preview {
        crate::infrastructure::config::PreviewMode::Tree => PreviewTab::Tree,
        crate::infrastructure::config::PreviewMode::Xml => PreviewTab::Xml,
        crate::infrastructure::config::PreviewMode::Content => PreviewTab::Content,
        crate::infrastructure::config::PreviewMode::Metadata => PreviewTab::Metadata,
    }
}

/// 将已验证配置颜色降低为 Ratatui 颜色。 / Lower a validated config color to a Ratatui color.
///
/// <!-- @brief 将已验证配置颜色降低为 Ratatui 颜色。 / Lower a validated config color to a Ratatui color. -->
fn parse_color(value: &str) -> Color {
    if let Some(hex) = value.strip_prefix('#').filter(|hex| hex.len() == 6)
        && let Ok(rgb) = u32::from_str_radix(hex, 16)
    {
        return Color::Rgb(
            ((rgb >> 16) & 255) as u8,
            ((rgb >> 8) & 255) as u8,
            (rgb & 255) as u8,
        );
    }
    match value.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" => Color::Gray,
        "darkgray" => Color::DarkGray,
        _ => Color::Reset,
    }
}

fn execute_effect<O: TerminalOps>(
    app: &mut Promptr,
    model: &mut Model,
    effect: Effect,
    session: &mut TerminalSession<O>,
    clipboard: &mut dyn Clipboard,
) -> Result<()> {
    if matches!(effect, Effect::CheckExternalChanges) {
        return refresh_if_changed(app, model);
    }
    if let Effect::CopyCanonicalXml(symbol) = &effect {
        let values = app.eval(
            &format!("OUTPUT {symbol};"),
            InvocationPolicy::interactive(),
        )?;
        let xml = values
            .into_iter()
            .find_map(|value| match value {
                Value::Xml(xml) => Some(xml),
                _ => None,
            })
            .ok_or_else(|| {
                Diagnostic::error(
                    "E_TUI_VALUE",
                    DiagnosticCategory::Internal,
                    "canonical output returned no XML",
                )
            })?;
        copy_xml(&xml, clipboard)?;
        model.notice = Some(format!("copied {} canonical XML bytes", xml.len()));
        return Ok(());
    }
    let is_preview = matches!(effect, Effect::LoadPreview { .. });
    let Some(request) = runtime_request(effect, model.selected_symbol()) else {
        return Ok(());
    };
    let saves_fragment = matches!(request, RuntimeRequest::EditFragment { .. });
    let saves_metadata = matches!(&request, RuntimeRequest::EditMetadata { .. });
    let refresh_catalog = match &request {
        RuntimeRequest::EditFragment { .. } | RuntimeRequest::EditMetadata { .. } => true,
        RuntimeRequest::Eval(source) => {
            let source = source.trim_start();
            !source.starts_with("SEARCH ")
                && !source.starts_with("PRINT ")
                && !source.starts_with("OUTPUT ")
        }
        RuntimeRequest::LoadFragmentDraft { .. }
        | RuntimeRequest::LoadMetadataDraft(_)
        | RuntimeRequest::Exit => false,
    };
    let preferred_selection = mutation_selection_hint(&request, model.selected_symbol());
    let values = match request {
        RuntimeRequest::Eval(source) => app.eval(&source, InvocationPolicy::interactive())?,
        RuntimeRequest::EditFragment { source, draft } => {
            let target = draft.target.ok_or_else(|| {
                Diagnostic::error(
                    "E_TUI_EDIT_TARGET",
                    DiagnosticCategory::Internal,
                    "fragment draft has no captured edit identity",
                )
            })?;
            let node_id = target
                .node_id
                .ok_or_else(|| missing_edit_identity("fragment"))?;
            let base_revision = target
                .base_revision
                .ok_or_else(|| missing_edit_identity("fragment"))?;
            let captured = EditRequest {
                target,
                original_text: draft.original.clone(),
            };
            let symbol = crate::domain::Symbol::new(draft.symbol.clone().unwrap_or_default())
                .map_err(|error| {
                    Diagnostic::error(
                        "E_TUI_EDIT_TARGET",
                        DiagnosticCategory::Internal,
                        "fragment draft has an invalid captured symbol",
                    )
                    .with_cause(error.to_string())
                })?;
            let precondition = NodePrecondition::new(symbol, node_id, base_revision);
            let values = match provider_route(app.config().editor.mode) {
                ProviderRoute::Builtin => {
                    let mut provider = OneShotProvider(Some(PreparedEdit::from_request(
                        captured,
                        draft.text.clone(),
                    )));
                    app.eval_with_provider_preconditioned(
                        &source,
                        InvocationPolicy::interactive(),
                        &mut provider,
                        std::slice::from_ref(&precondition),
                    )?
                }
                ProviderRoute::External => {
                    let argv = app.config().editor.external.clone().ok_or_else(|| {
                        Diagnostic::error(
                            "E_EDITOR_CONFIG",
                            DiagnosticCategory::Configuration,
                            "external editor mode requires a configured argv",
                        )
                    })?;
                    let provider = ExternalTextProvider::new(argv).map_err(editor_diagnostic)?;
                    let seeded = DraftSeedProvider {
                        provider,
                        request: Some(EditRequest {
                            target,
                            original_text: draft.text.clone(),
                        }),
                        original_text: draft.original.clone(),
                    };
                    let mut provider = SuspendedProvider {
                        session,
                        provider: seeded,
                    };
                    app.eval_with_provider_preconditioned(
                        &source,
                        InvocationPolicy::interactive(),
                        &mut provider,
                        &[precondition],
                    )?
                }
            };
            if values.is_empty() {
                restore_fragment_draft(model, draft.text);
                model.notice = Some("editor canceled; draft preserved".into());
                return Ok(());
            }
            values
        }
        RuntimeRequest::EditMetadata {
            source,
            target,
            symbol,
        } => {
            let symbol = crate::domain::Symbol::new(symbol).map_err(|error| {
                Diagnostic::error(
                    "E_TUI_EDIT_TARGET",
                    DiagnosticCategory::Internal,
                    "metadata draft has an invalid captured symbol",
                )
                .with_cause(error.to_string())
            })?;
            let precondition = NodePrecondition::new(
                symbol,
                target
                    .node_id
                    .ok_or_else(|| missing_edit_identity("metadata"))?,
                target
                    .base_revision
                    .ok_or_else(|| missing_edit_identity("metadata"))?,
            );
            app.eval_preconditioned(&source, InvocationPolicy::interactive(), &[precondition])?
        }
        RuntimeRequest::LoadFragmentDraft { source, symbol } => {
            let values = app.eval(&source, InvocationPolicy::interactive())?;
            load_fragment_draft(model, &symbol, &values)?;
            return Ok(());
        }
        RuntimeRequest::LoadMetadataDraft(source) => {
            let values = app.eval(&source, InvocationPolicy::interactive())?;
            let node = values
                .iter()
                .find_map(|value| match value {
                    Value::Node(node) => Some(node),
                    _ => None,
                })
                .ok_or_else(|| {
                    Diagnostic::error(
                        "E_TUI_VALUE",
                        DiagnosticCategory::Internal,
                        "metadata draft query returned no node",
                    )
                })?;
            let description = node.metadata.description().unwrap_or_default();
            model.set_draft(description.to_owned(), description.to_owned());
            if let Some(draft) = &mut model.draft {
                draft.target = Some(EditTarget {
                    node_id: Some(node.id),
                    base_revision: Some(node.revision),
                });
                draft.symbol = Some(node.symbol.to_string());
            }
            return Ok(());
        }
        RuntimeRequest::Exit => return Ok(()),
    };
    if is_preview {
        apply_preview_values(model, values)?;
    } else {
        apply_values(model, values);
    }
    if saves_fragment || saves_metadata {
        model.draft = None;
        model.editor = None;
    }
    // 成功 mutation 后重读目录，保留符号选择而不是假设旧索引仍有效。
    // After a successful mutation, reload the catalog and preserve the symbol rather than a stale index.
    if refresh_catalog && !model.should_quit {
        refresh_catalog_and_preview(app, model, preferred_selection.as_deref())?;
    }
    Ok(())
}

fn missing_edit_identity(kind: &str) -> Diagnostic {
    Diagnostic::error(
        "E_TUI_EDIT_TARGET",
        DiagnosticCategory::Internal,
        format!("{kind} draft has an incomplete captured edit identity"),
    )
}

fn refresh_catalog_and_preview(
    app: &mut Promptr,
    model: &mut Model,
    preferred_selection: Option<&str>,
) -> Result<()> {
    let values = app.eval("LIST;", InvocationPolicy::interactive())?;
    apply_catalog_values_preferred(model, &values, preferred_selection);
    if let Some(symbol) = model.selected_symbol().map(str::to_owned) {
        let values = app.eval(
            &preview_source(&symbol, model.preview_tab),
            InvocationPolicy::interactive(),
        )?;
        apply_preview_values(model, values)?;
    } else {
        model.preview = None;
        model.preview_diagnostic = None;
    }
    model.change_token = app.change_token()?;
    Ok(())
}

fn mutation_selection_hint(request: &RuntimeRequest, selected: Option<&str>) -> Option<String> {
    match request {
        RuntimeRequest::EditFragment { draft, .. } => draft.symbol.clone(),
        RuntimeRequest::EditMetadata { symbol, .. } => Some(symbol.clone()),
        RuntimeRequest::Eval(source) => {
            let tokens = source
                .trim_end_matches(|character: char| character == ';' || character.is_whitespace())
                .split_whitespace()
                .collect::<Vec<_>>();
            if tokens.len() == 4
                && tokens[0].eq_ignore_ascii_case("RENAME")
                && tokens[2].eq_ignore_ascii_case("TO")
                && selected == Some(tokens[1])
            {
                Some(tokens[3].to_owned())
            } else {
                selected.map(str::to_owned)
            }
        }
        _ => selected.map(str::to_owned),
    }
}

/// 仅在变化令牌改变时刷新目录与当前预览。 / Refresh catalog and current preview only when the change token changes.
///
/// <!-- @brief 仅在变化令牌改变时刷新目录与当前预览。 / Refresh catalog and current preview only when the change token changes. -->
fn refresh_if_changed(app: &mut Promptr, model: &mut Model) -> Result<()> {
    let current = app.change_token()?;
    let changed =
        matches!((model.change_token, current), (Some(previous), Some(next)) if previous != next);
    model.change_token = current;
    if !changed {
        return Ok(());
    }
    refresh_catalog_and_preview(app, model, None)?;
    model.notice = Some("catalog refreshed after an external commit".into());
    Ok(())
}

/// 将规范 XML 原样写入剪贴板。 / Write canonical XML unchanged to the clipboard.
///
/// <!-- @brief 将规范 XML 原样写入剪贴板。 / Write canonical XML unchanged to the clipboard. -->
fn copy_xml(xml: &crate::application::CanonicalXml, clipboard: &mut dyn Clipboard) -> Result<()> {
    let text = xml.read_to_string().map_err(|error| {
        Diagnostic::error(
            "E_CLIPBOARD",
            DiagnosticCategory::External,
            "canonical XML spool could not be read",
        )
        .with_cause(error.to_string())
    })?;
    clipboard.set_text(text).map_err(|cause| {
        Diagnostic::error(
            "E_CLIPBOARD",
            DiagnosticCategory::External,
            "system clipboard write failed",
        )
        .with_cause(cause)
    })
}

fn load_fragment_draft(model: &mut Model, symbol: &str, values: &[Value]) -> Result<()> {
    let node = values.iter().find_map(|value| match value {
        Value::Node(node) if node.kind == crate::domain::NodeKind::Fragment => Some(node),
        _ => None,
    });
    let Some(node) = node else {
        model.mode = super::Mode::Browse;
        model.draft = None;
        return Err(Diagnostic::error(
            "E_KIND",
            DiagnosticCategory::Domain,
            format!("`{symbol}` is not a fragment"),
        ));
    };
    let xml = values
        .iter()
        .find_map(|value| match value {
            Value::Xml(xml) => Some(xml.read_to_string()),
            _ => None,
        })
        .ok_or_else(|| {
            Diagnostic::error(
                "E_TUI_VALUE",
                DiagnosticCategory::Internal,
                "fragment draft query returned no XML",
            )
        })?
        .map_err(|error| {
            Diagnostic::error(
                "E_TUI_VALUE",
                DiagnosticCategory::External,
                "fragment XML spool could not be read",
            )
            .with_cause(error.to_string())
        })?;
    let prefix = format!("<{symbol}>");
    let suffix = format!("</{symbol}>\n");
    let encoded = xml
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(&suffix))
        .ok_or_else(|| {
            Diagnostic::error(
                "E_TUI_VALUE",
                DiagnosticCategory::Internal,
                "fragment XML did not match its selected symbol",
            )
        })?;
    // 领域 renderer 只转义这三种 XML 文本字符；ampersand 必须最后还原。
    // The domain renderer escapes only these XML text characters; restore ampersands last.
    let text = encoded
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    model.set_draft(text.clone(), text);
    if let Some(draft) = &mut model.draft {
        draft.target = Some(EditTarget {
            node_id: Some(node.id),
            base_revision: Some(node.revision),
        });
        draft.symbol = Some(node.symbol.to_string());
    }
    Ok(())
}

fn apply_values(model: &mut Model, values: Vec<Value>) {
    apply_catalog_values(model, &values);
    if let Some(hits) = values.iter().rev().find_map(|value| match value {
        Value::SearchResults(hits) => Some(hits),
        _ => None,
    }) {
        model.catalog = hits.iter().map(|hit| hit.symbol.to_string()).collect();
        model.selected = (!model.catalog.is_empty()).then_some(0);
        model.scroll = 0;
    }
    if let Some(value) = values
        .iter()
        .rev()
        .find(|value| !matches!(value, Value::Unit | Value::Nodes(_)))
    {
        model.notice = Some(match value {
            Value::Text(text) => text.clone(),
            Value::Xml(xml) => xml_notice(xml),
            Value::Node(node) => node.symbol.to_string(),
            Value::SearchResults(hits) => format!("{} matches", hits.len()),
            Value::Metadata(_) => "metadata updated".into(),
            Value::Unit | Value::Nodes(_) => unreachable!(),
        });
    }
}

/// 以有界内存生成 XML 通知。 / Build an XML notice with bounded memory.
///
/// # Arguments / 参数
///
/// - `xml` — 规范 XML 值。 / Canonical XML value.
///
/// # Returns / 返回值
///
/// 包含长度和有界预览的通知。 / Notice containing length and a bounded preview.
///
/// <!-- @brief 以有界内存生成 XML 通知。 / Build an XML notice with bounded memory. -->
/// <!-- @param xml 规范 XML 值。 / Canonical XML value. -->
/// <!-- @return 包含长度和有界预览的通知。 / Notice containing length and a bounded preview. -->
fn xml_notice(xml: &crate::application::CanonicalXml) -> String {
    const PREVIEW_BYTES: usize = 4096;
    match xml.preview(PREVIEW_BYTES) {
        Ok(preview) if xml.len() > preview.len() => {
            format!("XML ({} bytes)\n{preview}\u{2026}", xml.len())
        }
        Ok(preview) => format!("XML ({} bytes)\n{preview}", xml.len()),
        Err(error) => format!("XML ({} bytes; preview unavailable: {error})", xml.len()),
    }
}

fn apply_catalog_values(model: &mut Model, values: &[Value]) {
    let selected = model.selected_symbol().map(str::to_owned);
    apply_catalog_values_preferred(model, values, selected.as_deref());
}

fn apply_catalog_values_preferred(model: &mut Model, values: &[Value], preferred: Option<&str>) {
    let Some(nodes) = values.iter().rev().find_map(|value| {
        if let Value::Nodes(nodes) = value {
            Some(nodes)
        } else {
            None
        }
    }) else {
        return;
    };
    model.nodes = nodes.clone();
    model.catalog = nodes.iter().map(|node| node.symbol.to_string()).collect();
    model.selected = preferred
        .and_then(|symbol| {
            model
                .catalog
                .iter()
                .position(|candidate| candidate == symbol)
        })
        .or((!model.catalog.is_empty()).then_some(0));
    model.scroll = model.scroll.min(model.catalog.len().saturating_sub(1));
}

fn terminal_error(error: io::Error) -> Diagnostic {
    Diagnostic::error(
        "E_TERMINAL",
        DiagnosticCategory::External,
        "terminal operation failed",
    )
    .with_cause(error.to_string())
}

fn editor_diagnostic(error: EditorError) -> Diagnostic {
    Diagnostic::error(
        "E_EDITOR",
        DiagnosticCategory::External,
        "external editor failed",
    )
    .with_cause(error.to_string())
}

fn restore_fragment_draft(model: &mut Model, text: String) {
    model.mode = super::Mode::FragmentEdit;
    let previous = model.draft.clone().unwrap_or_default();
    model.set_draft(previous.original, text);
    if let Some(draft) = &mut model.draft {
        draft.target = previous.target;
        draft.symbol = previous.symbol;
    }
}

/// 在保存失败后恢复元数据草稿。 / Restore a metadata draft after a failed save.
///
/// <!-- @brief 在保存失败后恢复元数据草稿。 / Restore a metadata draft after a failed save. -->
fn restore_metadata_draft(model: &mut Model, text: String) {
    model.mode = super::Mode::MetadataEdit;
    let previous = model.draft.clone().unwrap_or_default();
    model.set_draft(previous.original, text);
    if let Some(draft) = &mut model.draft {
        draft.target = previous.target;
        draft.symbol = previous.symbol;
    }
}

/// 为预览投影生成只读 DSL。 / Build read-only DSL for a preview projection.
///
/// <!-- @brief 为预览投影生成只读 DSL。 / Build read-only DSL for a preview projection. -->
fn preview_source(symbol: &str, tab: PreviewTab) -> String {
    match tab {
        PreviewTab::Xml | PreviewTab::Content => format!("PRINT {symbol}; OUTPUT {symbol};"),
        PreviewTab::Tree | PreviewTab::Metadata => format!("PRINT {symbol};"),
    }
}

/// 把类型化运行时值安装到当前预览。 / Install typed runtime values into the current preview.
///
/// <!-- @brief 把类型化运行时值安装到当前预览。 / Install typed runtime values into the current preview. -->
fn apply_preview_values(model: &mut Model, values: Vec<Value>) -> Result<()> {
    model.preview_diagnostic = None;
    let node = values.iter().find_map(|value| match value {
        Value::Node(node) => Some(node.clone()),
        _ => None,
    });
    model.preview = match model.preview_tab {
        PreviewTab::Tree => node
            .as_ref()
            .map(|node| PreviewPayload::Tree(tree_lines(node, &model.nodes, 4096))),
        PreviewTab::Xml => values
            .iter()
            .find_map(|value| match value {
                Value::Xml(xml) => {
                    Some(xml.preview(64 * 1024).map(|preview| PreviewPayload::Xml {
                        truncated: xml.len() > preview.len(),
                        value: xml.clone(),
                        preview,
                    }))
                }
                _ => None,
            })
            .transpose()
            .map_err(|error| {
                Diagnostic::error(
                    "E_TUI_VALUE",
                    DiagnosticCategory::External,
                    "XML preview could not be read",
                )
                .with_cause(error.to_string())
            })?,
        PreviewTab::Content => match (
            node.as_ref(),
            values.iter().find_map(|value| match value {
                Value::Xml(xml) => Some(xml),
                _ => None,
            }),
        ) {
            (Some(node), Some(xml)) if node.kind == crate::domain::NodeKind::Fragment => {
                let (text, truncated) = fragment_preview(node.symbol.as_str(), xml, 64 * 1024)?;
                Some(PreviewPayload::Content { text, truncated })
            }
            (Some(_), _) => Some(PreviewPayload::Content {
                text: "Prompt content is represented by its ordered children.".into(),
                truncated: false,
            }),
            _ => None,
        },
        PreviewTab::Metadata => node.map(PreviewPayload::Metadata),
    };
    Ok(())
}

/// 从单个 Fragment 规范 XML 恢复精确内容。 / Recover exact content from one Fragment canonical XML.
///
/// <!-- @brief 从单个 Fragment 规范 XML 恢复精确内容。 / Recover exact content from one Fragment canonical XML. -->
fn fragment_preview(
    symbol: &str,
    xml: &crate::application::CanonicalXml,
    budget: usize,
) -> Result<(String, bool)> {
    let prefix = format!("<{symbol}>");
    let xml_prefix = xml
        .preview(prefix.len().saturating_add(budget))
        .map_err(|error| {
            Diagnostic::error(
                "E_TUI_VALUE",
                DiagnosticCategory::External,
                "fragment XML preview could not be read",
            )
            .with_cause(error.to_string())
        })?;
    let encoded = xml_prefix.strip_prefix(&prefix).ok_or_else(|| {
        Diagnostic::error(
            "E_TUI_VALUE",
            DiagnosticCategory::Internal,
            "fragment XML did not match its selected symbol",
        )
    })?;
    let suffix = format!("</{symbol}>\n");
    let (encoded, truncated) = if let Some(body) = encoded.strip_suffix(&suffix) {
        (body, false)
    } else {
        (encoded, xml.len() > xml_prefix.len())
    };
    Ok((
        encoded
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
        truncated,
    ))
}

/// 在视觉预算内展开命名 DAG 的出现树。 / Expand a named DAG as an occurrence tree within a visual budget.
///
/// <!-- @brief 在视觉预算内展开命名 DAG 的出现树。 / Expand a named DAG as an occurrence tree within a visual budget. -->
fn tree_lines(
    root: &crate::application::NodeView,
    nodes: &[crate::application::NodeView],
    budget: usize,
) -> Vec<String> {
    use std::collections::BTreeMap;
    let by_symbol: BTreeMap<_, _> = nodes
        .iter()
        .map(|node| (node.symbol.as_str(), node))
        .collect();
    let mut lines = vec![root.symbol.to_string()];
    let mut pending: Vec<_> = root
        .children
        .iter()
        .rev()
        .map(|child| (child.as_str(), 1usize))
        .collect();
    while let Some((symbol, depth)) = pending.pop() {
        if lines.len() >= budget {
            lines.push(format!("… more occurrences (visual budget {budget})"));
            break;
        }
        lines.push(format!("{}{}", "  ".repeat(depth), symbol));
        if let Some(node) = by_symbol.get(symbol) {
            pending.extend(
                node.children
                    .iter()
                    .rev()
                    .map(|child| (child.as_str(), depth + 1)),
            );
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct TerminalCounts {
        enters: usize,
        restores: usize,
    }

    struct FakeOps(Arc<Mutex<TerminalCounts>>);

    impl TerminalOps for FakeOps {
        fn enter(&mut self, _: bool) -> io::Result<()> {
            self.0.lock().unwrap().enters += 1;
            Ok(())
        }

        fn restore(&mut self, _: bool) -> io::Result<()> {
            self.0.lock().unwrap().restores += 1;
            Ok(())
        }
    }

    struct RecordingProvider(Arc<Mutex<usize>>);

    impl TextProvider for RecordingProvider {
        fn edit(
            &mut self,
            request: EditRequest,
        ) -> std::result::Result<EditorOutcome, EditorError> {
            *self.0.lock().unwrap() += 1;
            Ok(EditorOutcome::Save(PreparedEdit::from_request(
                request,
                "edited".into(),
            )))
        }
    }

    struct CancelProvider;

    #[derive(Default)]
    struct RecordingClipboard(Option<String>);

    impl Clipboard for RecordingClipboard {
        fn set_text(&mut self, text: String) -> std::result::Result<(), String> {
            self.0 = Some(text);
            Ok(())
        }
    }

    impl TextProvider for CancelProvider {
        fn edit(&mut self, _: EditRequest) -> std::result::Result<EditorOutcome, EditorError> {
            Ok(EditorOutcome::Cancel)
        }
    }

    fn test_app(path: &std::path::Path) -> Promptr {
        let root = path.parent().unwrap();
        let config = crate::infrastructure::config::Config::defaults(
            &crate::infrastructure::config::ConfigPaths {
                user_config: root.join("config.toml"),
                database: path.to_owned(),
                state_dir: root.join("state"),
                cache_dir: root.join("cache"),
                backup_dir: root.join("backup"),
            },
        );
        Promptr::open(crate::application::PromptrOptions {
            database_path: Some(path.to_owned()),
            config,
        })
        .unwrap()
    }

    fn save_fragment(app: &mut Promptr, symbol: &str, text: &str) {
        struct Provider(String);
        impl TextProvider for Provider {
            fn edit(
                &mut self,
                request: EditRequest,
            ) -> std::result::Result<EditorOutcome, EditorError> {
                Ok(EditorOutcome::Save(PreparedEdit::from_request(
                    request,
                    self.0.clone(),
                )))
            }
        }
        let mut provider = Provider(text.to_owned());
        app.eval_with_provider(
            &format!("FRAGMENT {symbol};"),
            InvocationPolicy::interactive(),
            &mut provider,
        )
        .unwrap();
    }

    fn test_host() -> (
        TerminalSession<FakeOps>,
        RecordingClipboard,
        Arc<Mutex<TerminalCounts>>,
    ) {
        let counts = Arc::new(Mutex::new(TerminalCounts::default()));
        (
            TerminalSession::start(FakeOps(Arc::clone(&counts)), false).unwrap(),
            RecordingClipboard::default(),
            counts,
        )
    }

    #[test]
    fn effects_map_to_the_shared_dsl_path() {
        assert_eq!(
            runtime_request(Effect::Search("a\"b".into()), None),
            Some(RuntimeRequest::Eval("SEARCH \"a\\\"b\";".into()))
        );
        assert_eq!(
            runtime_request(Effect::Delete("Leaf".into()), None),
            Some(RuntimeRequest::Eval("DELETE Leaf;".into()))
        );
        let draft = super::super::model::Draft {
            original: "old".into(),
            text: "body".into(),
            target: Some(EditTarget {
                node_id: Some(crate::domain::NodeId::new(1).unwrap()),
                base_revision: Some(crate::domain::Revision::new(2).unwrap()),
            }),
            symbol: Some("Leaf".into()),
        };
        assert_eq!(
            runtime_request(Effect::SaveFragment(draft.clone()), Some("ignored")),
            Some(RuntimeRequest::EditFragment {
                source: "FRAGMENT Leaf;".into(),
                draft
            })
        );
    }

    #[test]
    fn configured_route_and_suspended_provider_are_observable() {
        assert_eq!(provider_route(EditorMode::Builtin), ProviderRoute::Builtin);
        assert_eq!(
            provider_route(EditorMode::External),
            ProviderRoute::External
        );

        let terminal = Arc::new(Mutex::new(TerminalCounts::default()));
        let provider_calls = Arc::new(Mutex::new(0));
        let mut session = TerminalSession::start(FakeOps(Arc::clone(&terminal)), false).unwrap();
        {
            let mut provider = SuspendedProvider {
                session: &mut session,
                provider: RecordingProvider(Arc::clone(&provider_calls)),
            };
            let request = EditRequest {
                target: crate::infrastructure::editor::EditTarget {
                    node_id: None,
                    base_revision: None,
                },
                original_text: "original".into(),
            };
            assert!(matches!(provider.edit(request), Ok(EditorOutcome::Save(_))));
        }
        {
            let mut canceling = SuspendedProvider {
                session: &mut session,
                provider: CancelProvider,
            };
            let request = EditRequest {
                target: crate::infrastructure::editor::EditTarget {
                    node_id: None,
                    base_revision: None,
                },
                original_text: "draft".into(),
            };
            assert_eq!(canceling.edit(request).unwrap(), EditorOutcome::Cancel);
        }
        drop(session);

        assert_eq!(*provider_calls.lock().unwrap(), 1);
        let terminal = terminal.lock().unwrap();
        assert_eq!((terminal.enters, terminal.restores), (3, 3));
    }

    #[test]
    fn clipboard_receives_exact_canonical_output_bytes() {
        let xml: crate::application::CanonicalXml = "<Root>\n&amp;</Root>\n".into();
        let mut clipboard = RecordingClipboard::default();
        copy_xml(&xml, &mut clipboard).unwrap();
        assert_eq!(clipboard.0.as_deref(), Some("<Root>\n&amp;</Root>\n"));
    }

    #[test]
    fn stale_fragment_and_metadata_drafts_conflict_across_connections() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.sqlite");
        let mut first = test_app(&path);
        let mut second = test_app(&path);
        save_fragment(&mut first, "Leaf", "original");
        let (mut session, mut clipboard, _) = test_host();

        let mut fragment = Model {
            catalog: vec!["Leaf".into()],
            selected: Some(0),
            ..Model::default()
        };
        execute_effect(
            &mut first,
            &mut fragment,
            Effect::LoadFragmentDraft("Leaf".into()),
            &mut session,
            &mut clipboard,
        )
        .unwrap();
        fragment.draft.as_mut().unwrap().text = "mine".into();
        save_fragment(&mut second, "Leaf", "theirs");
        let saved = fragment.draft.clone().unwrap();
        let error = execute_effect(
            &mut first,
            &mut fragment,
            Effect::SaveFragment(saved.clone()),
            &mut session,
            &mut clipboard,
        )
        .unwrap_err();
        assert_eq!(error.code, "E_CONFLICT");
        restore_fragment_draft(&mut fragment, saved.text);
        assert!(fragment.draft.as_ref().unwrap().is_dirty());
        assert_eq!(fragment.draft.as_ref().unwrap().target, saved.target);

        let mut metadata = Model {
            catalog: vec!["Leaf".into()],
            selected: Some(0),
            ..Model::default()
        };
        execute_effect(
            &mut first,
            &mut metadata,
            Effect::LoadMetadataDraft("Leaf".into()),
            &mut session,
            &mut clipboard,
        )
        .unwrap();
        metadata.draft.as_mut().unwrap().text = "mine description".into();
        second
            .eval(
                "METADATA Leaf DESCRIPTION \"theirs\";",
                InvocationPolicy::interactive(),
            )
            .unwrap();
        let saved = metadata.draft.clone().unwrap();
        let error = execute_effect(
            &mut first,
            &mut metadata,
            Effect::SaveMetadata(saved.clone()),
            &mut session,
            &mut clipboard,
        )
        .unwrap_err();
        assert_eq!(error.code, "E_CONFLICT");
        restore_metadata_draft(&mut metadata, saved.text);
        assert!(metadata.draft.as_ref().unwrap().is_dirty());
        assert_eq!(metadata.draft.as_ref().unwrap().target, saved.target);
    }

    #[test]
    fn successful_local_mutation_refreshes_content_preview_immediately() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.sqlite");
        let mut app = test_app(&path);
        save_fragment(&mut app, "Leaf", "old");
        let (mut session, mut clipboard, _) = test_host();
        let mut model = Model {
            preview_tab: PreviewTab::Content,
            ..Model::default()
        };
        apply_catalog_values(
            &mut model,
            &app.eval("LIST;", InvocationPolicy::interactive()).unwrap(),
        );
        execute_effect(
            &mut app,
            &mut model,
            Effect::LoadFragmentDraft("Leaf".into()),
            &mut session,
            &mut clipboard,
        )
        .unwrap();
        let mut draft = model.draft.clone().unwrap();
        draft.text = "new & visible".into();
        execute_effect(
            &mut app,
            &mut model,
            Effect::SaveFragment(draft),
            &mut session,
            &mut clipboard,
        )
        .unwrap();
        assert!(matches!(
            model.preview,
            Some(PreviewPayload::Content { ref text, truncated: false }) if text == "new & visible"
        ));
    }

    #[test]
    fn rename_selects_new_symbol_and_refreshes_preview() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.sqlite");
        let mut app = test_app(&path);
        save_fragment(&mut app, "Leaf", "body");
        let (mut session, mut clipboard, _) = test_host();
        let mut model = Model::default();
        apply_catalog_values(
            &mut model,
            &app.eval("LIST;", InvocationPolicy::interactive()).unwrap(),
        );
        execute_effect(
            &mut app,
            &mut model,
            Effect::Execute("RENAME Leaf TO Renamed;".into()),
            &mut session,
            &mut clipboard,
        )
        .unwrap();
        assert_eq!(model.selected_symbol(), Some("Renamed"));
        assert!(matches!(
            model.preview,
            Some(PreviewPayload::Tree(ref lines)) if lines.first().is_some_and(|line| line == "Renamed")
        ));
    }

    #[test]
    fn large_content_preview_reads_only_a_bounded_prefix() {
        let mut writer = crate::application::SpillWriter::with_threshold(1);
        writer.write_all(b"<Leaf>").unwrap();
        for _ in 0..(17 * 1024) {
            writer.write_all(&[b'x'; 1024]).unwrap();
        }
        writer.write_all(b"</Leaf>\n").unwrap();
        let xml = writer.finish().unwrap();
        assert!(xml.len() > 16 * 1024 * 1024);
        let (text, truncated) = fragment_preview("Leaf", &xml, 64 * 1024).unwrap();
        assert!(truncated);
        assert!(text.len() <= 64 * 1024);
        assert!(text.bytes().all(|byte| byte == b'x'));
    }

    #[test]
    fn occurrence_tree_is_bounded_and_preserves_duplicates() {
        use crate::{
            application::NodeView,
            domain::{Metadata, NodeId, NodeKind, Revision, Symbol},
        };
        let leaf = NodeView {
            id: NodeId::new(1).unwrap(),
            symbol: Symbol::new("Leaf").unwrap(),
            kind: NodeKind::Fragment,
            revision: Revision::new(1).unwrap(),
            children: vec![],
            byte_size: Some(1),
            metadata: Metadata::default(),
        };
        let root = NodeView {
            id: NodeId::new(2).unwrap(),
            symbol: Symbol::new("Root").unwrap(),
            kind: NodeKind::Prompt,
            revision: Revision::new(1).unwrap(),
            children: vec![leaf.symbol.clone(), leaf.symbol.clone()],
            byte_size: None,
            metadata: Metadata::default(),
        };
        let lines = tree_lines(&root, &[root.clone(), leaf], 2);
        assert_eq!(lines[0], "Root");
        assert_eq!(lines[1].trim(), "Leaf");
        assert!(lines[2].contains("visual budget"));
    }

    #[test]
    fn configured_non_default_preview_is_honored() {
        assert_eq!(
            configured_preview(crate::infrastructure::config::PreviewMode::Metadata),
            PreviewTab::Metadata
        );
        assert_eq!(parse_color("#010203"), Color::Rgb(1, 2, 3));
    }

    #[test]
    fn failed_saves_restore_original_and_dirty_drafts() {
        let mut fragment = Model {
            mode: super::super::Mode::Browse,
            ..Model::default()
        };
        fragment.draft = Some(super::super::model::Draft {
            original: "old".into(),
            text: "new".into(),
            ..Default::default()
        });
        restore_fragment_draft(&mut fragment, "new".into());
        assert_eq!(fragment.mode, super::super::Mode::FragmentEdit);
        assert_eq!(fragment.draft.as_ref().unwrap().original, "old");
        assert!(fragment.draft.as_ref().unwrap().is_dirty());

        let mut metadata = Model {
            mode: super::super::Mode::Browse,
            ..Model::default()
        };
        metadata.draft = Some(super::super::model::Draft {
            original: "before".into(),
            text: "after".into(),
            ..Default::default()
        });
        restore_metadata_draft(&mut metadata, "after".into());
        assert_eq!(metadata.mode, super::super::Mode::MetadataEdit);
        assert_eq!(metadata.draft.as_ref().unwrap().original, "before");
        assert!(metadata.draft.as_ref().unwrap().is_dirty());
    }

    #[test]
    fn explicit_color_overrides_no_color_but_dumb_terminal_always_degrades() {
        use crate::infrastructure::config::{
            ColorMode, Config, ConfigPaths, GlyphMode as ConfigGlyph,
        };
        let root = tempfile::tempdir().unwrap();
        let mut config = Config::defaults(&ConfigPaths {
            user_config: root.path().join("config.toml"),
            database: root.path().join("db.sqlite"),
            state_dir: root.path().join("state"),
            cache_dir: root.path().join("cache"),
            backup_dir: root.path().join("backup"),
        });
        assert_eq!(
            terminal_capabilities_from(&config, true, false).0,
            Theme::Monochrome
        );

        config.ui.color = ColorMode::Truecolor;
        assert_eq!(
            terminal_capabilities_from(&config, true, false).0,
            Theme::Dark
        );
        config.ui.color = ColorMode::Ansi256;
        assert_eq!(
            terminal_capabilities_from(&config, true, false).0,
            Theme::Dark
        );

        config.ui.color = ColorMode::None;
        assert_eq!(
            terminal_capabilities_from(&config, false, false).0,
            Theme::Monochrome
        );
        config.ui.color = ColorMode::Truecolor;
        config.ui.glyphs = ConfigGlyph::Unicode;
        assert_eq!(
            terminal_capabilities_from(&config, false, true),
            (Theme::Monochrome, GlyphMode::Ascii)
        );
    }
}
