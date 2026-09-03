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
    application::{InvocationPolicy, Promptr, Value},
    diagnostic::{Diagnostic, DiagnosticCategory, Result},
    infrastructure::config::{
        ColorMode as ConfigColorMode, EditorMode, GlyphMode as ConfigGlyphMode,
    },
    infrastructure::editor::{
        EditRequest, EditorError, EditorOutcome, ExternalTextProvider, PreparedEdit, TextProvider,
    },
};

use super::{
    CustomPalette, Effect, GlyphMode, InputMapper, Model, PreviewTab, Theme,
    model::PreviewPayload,
    terminal::{CrosstermOps, TerminalOps, TerminalSession},
    update, view,
};

/// @brief 可单元测试的运行时请求计划。 / Unit-testable runtime request plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeRequest {
    Eval(String),
    LoadFragmentDraft { source: String, symbol: String },
    LoadMetadataDraft(String),
    EditFragment { source: String, draft: String },
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

/// @brief 可替换的系统剪贴板边界。 / Replaceable system clipboard boundary.
pub trait Clipboard {
    /// @brief 写入完整 UTF-8 文本。 / Write complete UTF-8 text.
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

/// @brief 将 reducer 效果转换为唯一应用运行时请求。 / Convert a reducer effect into one application-runtime request.
/// @param effect reducer 产生的效果。 / Effect produced by the reducer.
/// @param selected 当前选中符号。 / Currently selected symbol.
/// @return 请求；纯 UI 预览也统一落到 DSL eval。 / Request; UI projections also converge on DSL eval.
pub fn runtime_request(effect: Effect, selected: Option<&str>) -> Option<RuntimeRequest> {
    match effect {
        Effect::Execute(source) => Some(RuntimeRequest::Eval(source)),
        Effect::Search(query) => Some(RuntimeRequest::Eval(format!("SEARCH {};", quote(&query)))),
        Effect::SaveFragment(draft) => selected.map(|symbol| RuntimeRequest::EditFragment {
            source: format!("FRAGMENT {symbol};"),
            draft,
        }),
        Effect::LoadFragmentDraft(symbol) => Some(RuntimeRequest::LoadFragmentDraft {
            source: format!("PRINT {symbol}; OUTPUT {symbol};"),
            symbol,
        }),
        Effect::LoadMetadataDraft(symbol) => Some(RuntimeRequest::LoadMetadataDraft(format!(
            "PRINT {symbol};"
        ))),
        Effect::SaveMetadata(draft) => selected.map(|symbol| {
            RuntimeRequest::Eval(format!("METADATA {symbol} DESCRIPTION {};", quote(&draft)))
        }),
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

struct OneShotProvider(Option<String>);

impl TextProvider for OneShotProvider {
    fn edit(&mut self, request: EditRequest) -> std::result::Result<EditorOutcome, EditorError> {
        Ok(match self.0.take() {
            Some(text) => EditorOutcome::Save(PreparedEdit::from_request(request, text)),
            None => EditorOutcome::Cancel,
        })
    }
}

/// @brief 配置选择的 Fragment 编辑提供者类别。 / Fragment editor provider kind selected by configuration.
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
    draft: Option<String>,
}

impl<P: TextProvider> TextProvider for DraftSeedProvider<P> {
    fn edit(&mut self, request: EditRequest) -> std::result::Result<EditorOutcome, EditorError> {
        let original_request = request.clone();
        let seeded = EditRequest {
            target: request.target,
            original_text: self.draft.take().unwrap_or(request.original_text),
        };
        Ok(match self.provider.edit(seeded)? {
            EditorOutcome::Save(edit) => EditorOutcome::Save(PreparedEdit::from_request(
                original_request,
                edit.edited_text,
            )),
            EditorOutcome::Cancel => EditorOutcome::Cancel,
        })
    }
}

/// @brief 在当前终端运行同步 TUI。 / Run the synchronous TUI in the current terminal.
/// @param app 已打开的共享应用门面。 / Open shared application facade.
/// @return 正常退出或结构化终端诊断。 / Normal exit or structured terminal diagnostic.
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
                Effect::SaveFragment(draft) => Some(draft.clone()),
                _ => None,
            };
            let failed_metadata = match &effect {
                Effect::SaveMetadata(draft) => Some(draft.clone()),
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

/// @brief 从配置与已探测环境计算终端能力。 / Compute terminal capabilities from config and already-probed environment.
/// @param config 已验证配置。 / Validated configuration.
/// @param no_color 是否存在 NO_COLOR。 / Whether NO_COLOR is present.
/// @param dumb TERM 是否为 dumb。 / Whether TERM is dumb.
/// @return 降级后的主题与字形。 / Degraded theme and glyphs.
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

/// @brief 将配置预览模式转换为 TUI 标签。 / Convert configured preview mode to a TUI tab.
fn configured_preview(preview: crate::infrastructure::config::PreviewMode) -> PreviewTab {
    match preview {
        crate::infrastructure::config::PreviewMode::Tree => PreviewTab::Tree,
        crate::infrastructure::config::PreviewMode::Xml => PreviewTab::Xml,
        crate::infrastructure::config::PreviewMode::Content => PreviewTab::Content,
        crate::infrastructure::config::PreviewMode::Metadata => PreviewTab::Metadata,
    }
}

/// @brief 将已验证配置颜色降低为 Ratatui 颜色。 / Lower a validated config color to a Ratatui color.
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
    let saves_metadata = matches!(&request, RuntimeRequest::Eval(source) if source.trim_start().starts_with("METADATA "));
    let refresh_catalog = match &request {
        RuntimeRequest::EditFragment { .. } => true,
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
    let values = match request {
        RuntimeRequest::Eval(source) => app.eval(&source, InvocationPolicy::interactive())?,
        RuntimeRequest::EditFragment { source, draft } => {
            let values = match provider_route(app.config().editor.mode) {
                ProviderRoute::Builtin => {
                    let mut provider = OneShotProvider(Some(draft.clone()));
                    app.eval_with_provider(&source, InvocationPolicy::interactive(), &mut provider)?
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
                        draft: Some(draft.clone()),
                    };
                    let mut provider = SuspendedProvider {
                        session,
                        provider: seeded,
                    };
                    app.eval_with_provider(&source, InvocationPolicy::interactive(), &mut provider)?
                }
            };
            if values.is_empty() {
                restore_fragment_draft(model, draft);
                model.notice = Some("editor canceled; draft preserved".into());
                return Ok(());
            }
            values
        }
        RuntimeRequest::LoadFragmentDraft { source, symbol } => {
            let values = app.eval(&source, InvocationPolicy::interactive())?;
            load_fragment_draft(model, &symbol, &values)?;
            return Ok(());
        }
        RuntimeRequest::LoadMetadataDraft(source) => {
            let values = app.eval(&source, InvocationPolicy::interactive())?;
            let description = values
                .iter()
                .find_map(|value| match value {
                    Value::Node(node) => Some(node.metadata.description().unwrap_or_default()),
                    _ => None,
                })
                .ok_or_else(|| {
                    Diagnostic::error(
                        "E_TUI_VALUE",
                        DiagnosticCategory::Internal,
                        "metadata draft query returned no node",
                    )
                })?;
            model.set_draft(description.to_owned(), description.to_owned());
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
    if refresh_catalog && !model.should_quit {
        let values = app.eval("LIST;", InvocationPolicy::interactive())?;
        apply_catalog_values(model, &values);
    }
    Ok(())
}

/// @brief 仅在变化令牌改变时刷新目录与当前预览。 / Refresh catalog and current preview only when the change token changes.
fn refresh_if_changed(app: &mut Promptr, model: &mut Model) -> Result<()> {
    let current = app.change_token()?;
    let changed =
        matches!((model.change_token, current), (Some(previous), Some(next)) if previous != next);
    model.change_token = current;
    if !changed {
        return Ok(());
    }
    apply_catalog_values(model, &app.eval("LIST;", InvocationPolicy::interactive())?);
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
    model.notice = Some("catalog refreshed after an external commit".into());
    Ok(())
}

/// @brief 将规范 XML 原样写入剪贴板。 / Write canonical XML unchanged to the clipboard.
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
    let is_fragment = values.iter().any(
        |value| matches!(value, Value::Node(node) if node.kind == crate::domain::NodeKind::Fragment),
    );
    if !is_fragment {
        model.mode = super::Mode::Browse;
        model.draft = None;
        return Err(Diagnostic::error(
            "E_KIND",
            DiagnosticCategory::Domain,
            format!("`{symbol}` is not a fragment"),
        ));
    }
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
    let text = encoded
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    model.set_draft(text.clone(), text);
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

/// @brief 以有界内存生成 XML 通知。 / Build an XML notice with bounded memory.
/// @param xml 规范 XML 值。 / Canonical XML value.
/// @return 包含长度和有界预览的通知。 / Notice containing length and a bounded preview.
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
    let Some(nodes) = values.iter().rev().find_map(|value| {
        if let Value::Nodes(nodes) = value {
            Some(nodes)
        } else {
            None
        }
    }) else {
        return;
    };
    let selected = model.selected_symbol().map(str::to_owned);
    model.nodes = nodes.clone();
    model.catalog = nodes.iter().map(|node| node.symbol.to_string()).collect();
    model.selected = selected
        .and_then(|symbol| {
            model
                .catalog
                .iter()
                .position(|candidate| candidate == &symbol)
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
    let original = model
        .draft
        .as_ref()
        .map(|draft| draft.original.clone())
        .unwrap_or_default();
    model.set_draft(original, text);
}

/// @brief 在保存失败后恢复元数据草稿。 / Restore a metadata draft after a failed save.
fn restore_metadata_draft(model: &mut Model, text: String) {
    model.mode = super::Mode::MetadataEdit;
    let original = model
        .draft
        .as_ref()
        .map(|draft| draft.original.clone())
        .unwrap_or_default();
    model.set_draft(original, text);
}

/// @brief 为预览投影生成只读 DSL。 / Build read-only DSL for a preview projection.
fn preview_source(symbol: &str, tab: PreviewTab) -> String {
    match tab {
        PreviewTab::Xml | PreviewTab::Content => format!("PRINT {symbol}; OUTPUT {symbol};"),
        PreviewTab::Tree | PreviewTab::Metadata => format!("PRINT {symbol};"),
    }
}

/// @brief 把类型化运行时值安装到当前预览。 / Install typed runtime values into the current preview.
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
            (Some(node), Some(xml)) if node.kind == crate::domain::NodeKind::Fragment => Some(
                PreviewPayload::Content(fragment_text(node.symbol.as_str(), xml)?),
            ),
            (Some(_), _) => Some(PreviewPayload::Content(
                "Prompt content is represented by its ordered children.".into(),
            )),
            _ => None,
        },
        PreviewTab::Metadata => node.map(PreviewPayload::Metadata),
    };
    Ok(())
}

/// @brief 从单个 Fragment 规范 XML 恢复精确内容。 / Recover exact content from one Fragment canonical XML.
fn fragment_text(symbol: &str, xml: &crate::application::CanonicalXml) -> Result<String> {
    let xml = xml.read_to_string().map_err(|error| {
        Diagnostic::error(
            "E_TUI_VALUE",
            DiagnosticCategory::External,
            "fragment XML could not be read",
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
    Ok(encoded
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&"))
}

/// @brief 在视觉预算内展开命名 DAG 的出现树。 / Expand a named DAG as an occurrence tree within a visual budget.
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
        assert_eq!(
            runtime_request(Effect::SaveFragment("body".into()), Some("Leaf")),
            Some(RuntimeRequest::EditFragment {
                source: "FRAGMENT Leaf;".into(),
                draft: "body".into()
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
