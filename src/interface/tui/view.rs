//! 基于 Ratatui Buffer 的无副作用视图投影。 / Side-effect-free view projection onto a Ratatui Buffer.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use super::{
    GlyphMode, Glyphs, HitRegion, HitTarget, StyleToken, Theme,
    layout::{Layout, LayoutClass},
    model::{Mode, Model, Pane, PreviewPayload, PreviewTab},
};

/// @brief 将模型渲染到给定 Buffer 并返回同源命中区域。 / Render a model into a Buffer and return co-generated hit regions.
/// @param model 只读 UI 模型。 / Read-only UI model.
/// @param buffer Ratatui 目标缓冲区。 / Ratatui destination buffer.
/// @param theme 完整语义主题。 / Complete semantic theme.
/// @param glyph_mode Unicode 或 ASCII 能力。 / Unicode or ASCII capability.
/// @return 与此次渲染几何严格一致的命中区域。 / Hit regions exactly matching this render geometry.
pub fn render(
    model: &Model,
    buffer: &mut Buffer,
    theme: Theme,
    glyph_mode: GlyphMode,
) -> Vec<HitRegion> {
    let area = intersect(model.terminal, buffer.area);
    buffer.set_style(area, theme.style(StyleToken::Surface));
    let layout = Layout::compute(area);
    let glyphs = Glyphs::for_mode(glyph_mode);
    let mut hits = Vec::new();
    if matches!(
        model.mode,
        Mode::FragmentEdit | Mode::FragmentFind | Mode::MetadataEdit
    ) {
        render_editor(model, buffer, layout.content, theme);
        render_status(model, buffer, layout.status, layout.class, theme, glyphs);
        return hits;
    }
    if let Some(rect) = layout.catalog {
        render_catalog(model, buffer, rect, theme, glyphs, &mut hits);
    }
    if let Some(rect) = layout.preview {
        render_preview(model, buffer, rect, theme, &mut hits);
    }
    if let Some(rect) = layout.metadata {
        render_metadata(model, buffer, rect, theme, &mut hits);
    }
    render_status(model, buffer, layout.status, layout.class, theme, glyphs);
    hits
}

fn render_editor(model: &Model, buffer: &mut Buffer, rect: Rect, theme: Theme) {
    if rect.is_empty() {
        return;
    }
    let title = if model.mode == Mode::FragmentEdit {
        " Fragment editor - Ctrl-S save / Esc cancel "
    } else {
        " Metadata editor - Ctrl-S save / Esc cancel "
    };
    if let Some(editor) = &model.editor {
        let mut area = editor.textarea();
        area.set_block(pane_block(title, true, theme));
        area.set_style(theme.style(StyleToken::Text));
        area.set_cursor_style(theme.style(StyleToken::Selection));
        Widget::render(&area, rect, buffer);
    } else {
        let block = pane_block(title, true, theme);
        let inner = block.inner(rect);
        block.render(rect, buffer);
        if !inner.is_empty() {
            Paragraph::new(
                model
                    .draft
                    .as_ref()
                    .map(|d| d.text.as_str())
                    .unwrap_or_default(),
            )
            .style(theme.style(StyleToken::Text))
            .render(inner, buffer);
        }
    }
}

fn intersect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.x.saturating_add(a.width).min(b.x.saturating_add(b.width));
    let bottom =
        a.y.saturating_add(a.height)
            .min(b.y.saturating_add(b.height));
    Rect::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
}

fn pane_block<'a>(title: &'a str, focused: bool, theme: Theme) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(theme.style(if focused {
            StyleToken::BorderFocused
        } else {
            StyleToken::Border
        }))
}

fn render_catalog(
    model: &Model,
    buffer: &mut Buffer,
    rect: Rect,
    theme: Theme,
    glyphs: Glyphs,
    hits: &mut Vec<HitRegion>,
) {
    if rect.is_empty() {
        return;
    }
    let block = pane_block(" Catalog ", model.focus == Pane::Catalog, theme);
    let inner = block.inner(rect);
    block.render(rect, buffer);
    if inner.is_empty() {
        return;
    }
    let rows = inner.height as usize;
    for (visible, (index, symbol)) in model
        .catalog
        .iter()
        .enumerate()
        .skip(model.scroll)
        .take(rows)
        .enumerate()
    {
        let selected = model.selected == Some(index);
        let marker = if selected { glyphs.collapsed } else { " " };
        let style = theme.style(if selected {
            StyleToken::Selection
        } else {
            StyleToken::Text
        });
        let row = Rect::new(inner.x, inner.y + visible as u16, inner.width, 1);
        Paragraph::new(Line::from(vec![
            Span::raw(marker),
            Span::raw(" "),
            Span::raw(symbol),
        ]))
        .style(style)
        .render(row, buffer);
        hits.push(HitRegion {
            rect: row,
            target: HitTarget::Node(index),
        });
    }
    hits.push(HitRegion {
        rect,
        target: HitTarget::Pane(Pane::Catalog),
    });
}

fn render_preview(
    model: &Model,
    buffer: &mut Buffer,
    rect: Rect,
    theme: Theme,
    hits: &mut Vec<HitRegion>,
) {
    if rect.is_empty() {
        return;
    }
    let block = pane_block(" Preview ", model.focus == Pane::Preview, theme);
    let inner = block.inner(rect);
    block.render(rect, buffer);
    if !inner.is_empty() {
        let tabs = [
            (PreviewTab::Tree, "TREE"),
            (PreviewTab::Xml, "XML"),
            (PreviewTab::Content, "CONTENT"),
            (PreviewTab::Metadata, "META"),
        ];
        let mut x = inner.x;
        for (tab, label) in tabs {
            let width = (label.len() as u16)
                .saturating_add(1)
                .min(inner.right().saturating_sub(x));
            if width == 0 {
                break;
            }
            let target = Rect::new(x, inner.y, width, 1);
            Paragraph::new(label)
                .style(theme.style(if model.preview_tab == tab {
                    StyleToken::Selection
                } else {
                    StyleToken::TextMuted
                }))
                .render(target, buffer);
            hits.push(HitRegion {
                rect: target,
                target: HitTarget::Preview(tab),
            });
            x = x.saturating_add(width);
        }
        if inner.height > 1 {
            let content = match &model.preview {
                Some(PreviewPayload::Tree(lines)) => lines.join("\n"),
                Some(PreviewPayload::Xml {
                    preview, truncated, ..
                }) => {
                    if *truncated {
                        format!("{preview}\n… preview truncated")
                    } else {
                        preview.clone()
                    }
                }
                Some(PreviewPayload::Content { text, truncated }) => {
                    if *truncated {
                        format!("{text}\n… preview truncated")
                    } else {
                        text.clone()
                    }
                }
                Some(PreviewPayload::Metadata(node)) => metadata_text(node),
                None => model.selected_symbol().unwrap_or("No selection").to_owned(),
            };
            let offset = model.preview_scroll[preview_index(model.preview_tab)]
                .min(u16::MAX as usize) as u16;
            Paragraph::new(content)
                .style(theme.style(StyleToken::Text))
                .scroll((offset, 0))
                .wrap(ratatui::widgets::Wrap { trim: false })
                .render(
                    Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1),
                    buffer,
                );
        }
    }
    hits.push(HitRegion {
        rect,
        target: HitTarget::Pane(Pane::Preview),
    });
}

fn render_metadata(
    model: &Model,
    buffer: &mut Buffer,
    rect: Rect,
    theme: Theme,
    hits: &mut Vec<HitRegion>,
) {
    if rect.is_empty() {
        return;
    }
    let block = pane_block(" Metadata ", model.focus == Pane::Metadata, theme);
    let inner = block.inner(rect);
    block.render(rect, buffer);
    if !inner.is_empty() {
        let text = model
            .nodes
            .iter()
            .find(|node| Some(node.symbol.as_str()) == model.selected_symbol())
            .map(metadata_text)
            .unwrap_or_else(|| "No selection".into());
        Paragraph::new(text)
            .style(theme.style(StyleToken::TextMuted))
            .render(inner, buffer);
    }
    hits.push(HitRegion {
        rect,
        target: HitTarget::Pane(Pane::Metadata),
    });
}

fn preview_index(tab: PreviewTab) -> usize {
    match tab {
        PreviewTab::Tree => 0,
        PreviewTab::Xml => 1,
        PreviewTab::Content => 2,
        PreviewTab::Metadata => 3,
    }
}

fn metadata_text(node: &crate::application::NodeView) -> String {
    let description = node.metadata.description().unwrap_or_default();
    let tags = node
        .metadata
        .tags()
        .map(|tag| tag.as_str().to_owned())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{}\nkind: {:?}\nrevision: {}\nchildren: {}\ntags: {}\ndescription: {}",
        node.symbol,
        node.kind,
        node.revision.get(),
        node.children.len(),
        tags,
        description
    )
}

fn render_status(
    model: &Model,
    buffer: &mut Buffer,
    rect: Rect,
    class: LayoutClass,
    theme: Theme,
    glyphs: Glyphs,
) {
    if rect.is_empty() {
        return;
    }
    let mode = match model.mode {
        Mode::Browse => "BROWSE",
        Mode::Command => "COMMAND",
        Mode::Search => "SEARCH",
        Mode::FragmentEdit => "EDIT",
        Mode::FragmentFind => "FIND",
        Mode::MetadataEdit => "METADATA",
        Mode::Confirm(_) => "CONFIRM",
        Mode::Help => "HELP",
    };
    let mut status = if model.draft.as_ref().is_some_and(|draft| draft.is_dirty()) {
        format!("{} DRAFT  {mode}", glyphs.dirty)
    } else {
        mode.to_owned()
    };
    if model.mode == Mode::Command {
        status.push_str(&format!("  :{}", model.input));
    }
    if model.mode == Mode::Search {
        status.push_str(&format!("  /{}", model.input));
    }
    if model.mode == Mode::FragmentFind {
        status.push_str(&format!("  find /{}", model.input));
    }
    if let Some(diagnostic) = &model.preview_diagnostic {
        status.push_str(&format!("  {}: {}", diagnostic.code, diagnostic.message));
    }
    if class == LayoutClass::Focused {
        status.push_str("  ! terminal too small");
    }
    if let Some(notice) = &model.notice {
        status.push_str("  ");
        status.push_str(notice);
    }
    Paragraph::new(status)
        .style(theme.style(if class == LayoutClass::Focused {
            StyleToken::Warning
        } else {
            StyleToken::Text
        }))
        .render(rect, buffer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_small_layout_renders_without_panicking() {
        for width in 0..60 {
            for height in 0..16 {
                let area = Rect::new(0, 0, width, height);
                let mut buffer = Buffer::empty(area);
                render(
                    &Model::new(width, height),
                    &mut buffer,
                    Theme::Dark,
                    GlyphMode::Ascii,
                );
            }
        }
    }

    #[test]
    fn monochrome_selection_and_focus_have_non_color_semantics() {
        let selection = Theme::Monochrome.style(StyleToken::Selection);
        let focus = Theme::Monochrome.style(StyleToken::BorderFocused);
        assert!(
            selection
                .add_modifier
                .contains(ratatui::style::Modifier::REVERSED)
        );
        assert!(focus.add_modifier.contains(ratatui::style::Modifier::BOLD));
        assert_eq!(Glyphs::for_mode(GlyphMode::Ascii).error, "X");
    }

    #[test]
    fn preview_buffer_contains_typed_payload_not_only_symbol() {
        let area = Rect::new(0, 0, 120, 30);
        let mut buffer = Buffer::empty(area);
        let mut model = Model::new(120, 30);
        model.catalog = vec!["Root".into()];
        model.selected = Some(0);
        model.preview = Some(PreviewPayload::Tree(vec!["Root".into(), "  Leaf".into()]));
        render(&model, &mut buffer, Theme::Dark, GlyphMode::Ascii);
        let rendered = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Leaf"));
        assert!(rendered.contains("TREE"));
    }

    #[test]
    fn custom_palette_controls_semantic_tokens() {
        let palette = super::super::CustomPalette {
            surface: ratatui::style::Color::Rgb(1, 2, 3),
            text: ratatui::style::Color::White,
            muted: ratatui::style::Color::Gray,
            selection: ratatui::style::Color::Blue,
            fragment: ratatui::style::Color::Cyan,
            prompt: ratatui::style::Color::Magenta,
            tag: ratatui::style::Color::Yellow,
            success: ratatui::style::Color::Green,
            warning: ratatui::style::Color::Yellow,
            error: ratatui::style::Color::Red,
        };
        assert_eq!(
            Theme::Custom(palette).style(StyleToken::Surface).fg,
            Some(ratatui::style::Color::Rgb(1, 2, 3))
        );
    }
}
