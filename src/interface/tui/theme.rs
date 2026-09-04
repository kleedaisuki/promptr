//! 语义样式和字形能力适配。 / Semantic styles and glyph capability adaptation.

use ratatui::style::{Color, Modifier, Style};

/// 控件请求的语义样式，而非具体颜色。 / Semantic style requested by widgets instead of a raw color.
///
/// <!-- @brief 控件请求的语义样式，而非具体颜色。 / Semantic style requested by widgets instead of a raw color. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleToken {
    /// 应用背景表面。 / Application background surface.
    Surface,
    /// 未聚焦的窗格边框。 / Unfocused pane border.
    Border,
    /// 已聚焦的窗格边框。 / Focused pane border.
    BorderFocused,
    /// 主要正文。 / Primary text.
    Text,
    /// 次要弱化正文。 / Secondary muted text.
    TextMuted,
    /// 当前选择。 / Current selection.
    Selection,
    /// 搜索匹配。 / Search match.
    Match,
    /// Fragment 节点。 / Fragment node.
    Fragment,
    /// Prompt 节点。 / Prompt node.
    Prompt,
    /// 元数据标签。 / Metadata tag.
    Tag,
    /// 成功状态。 / Success state.
    Success,
    /// 警告状态。 / Warning state.
    Warning,
    /// 错误状态。 / Error state.
    Error,
    /// 已修改草稿。 / Modified draft.
    Draft,
    /// XML 标签。 / XML tag.
    XmlTag,
    /// XML 文本。 / XML text.
    XmlText,
}

/// 完整主题能力。 / Complete theme capability.
///
/// <!-- @brief 完整主题能力。 / Complete theme capability. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// 深色终端主题。 / Dark terminal theme.
    Dark,
    /// 浅色终端主题。 / Light terminal theme.
    Light,
    /// 不依赖颜色的单色主题。 / Color-independent monochrome theme.
    Monochrome,
    /// 完整自定义语义调色板。 / Complete custom semantic palette.
    Custom(CustomPalette),
}

/// 已解析的自定义主题颜色。 / Parsed custom theme colors.
///
/// <!-- @brief 已解析的自定义主题颜色。 / Parsed custom theme colors. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomPalette {
    /// 表面色。 / Surface color.
    ///
    /// <!-- @brief 表面色。 / Surface color. -->
    pub surface: Color,
    /// 正文色。 / Text color.
    ///
    /// <!-- @brief 正文色。 / Text color. -->
    pub text: Color,
    /// 弱化色。 / Muted color.
    ///
    /// <!-- @brief 弱化色。 / Muted color. -->
    pub muted: Color,
    /// 选中色。 / Selection color.
    ///
    /// <!-- @brief 选中色。 / Selection color. -->
    pub selection: Color,
    /// Fragment 色。 / Fragment color.
    ///
    /// <!-- @brief Fragment 色。 / Fragment color. -->
    pub fragment: Color,
    /// Prompt 色。 / Prompt color.
    ///
    /// <!-- @brief Prompt 色。 / Prompt color. -->
    pub prompt: Color,
    /// 标签色。 / Tag color.
    ///
    /// <!-- @brief 标签色。 / Tag color. -->
    pub tag: Color,
    /// 成功色。 / Success color.
    ///
    /// <!-- @brief 成功色。 / Success color. -->
    pub success: Color,
    /// 警告色。 / Warning color.
    ///
    /// <!-- @brief 警告色。 / Warning color. -->
    pub warning: Color,
    /// 错误色。 / Error color.
    ///
    /// <!-- @brief 错误色。 / Error color. -->
    pub error: Color,
}

impl Theme {
    /// 将语义标记降低为终端样式。 / Lower a semantic token to terminal style.
    ///
    /// # Arguments / 参数
    ///
    /// - `token` — 语义样式标记。 / Semantic style token.
    ///
    /// # Returns / 返回值
    ///
    /// 不依赖业务状态的终端样式。 / Terminal style independent of business state.
    ///
    /// <!-- @brief 将语义标记降低为终端样式。 / Lower a semantic token to terminal style. -->
    /// <!-- @param token 语义样式标记。 / Semantic style token. -->
    /// <!-- @return 不依赖业务状态的终端样式。 / Terminal style independent of business state. -->
    pub fn style(self, token: StyleToken) -> Style {
        if self == Self::Monochrome {
            return match token {
                StyleToken::Selection => Style::default().add_modifier(Modifier::REVERSED),
                StyleToken::BorderFocused => Style::default().add_modifier(Modifier::BOLD),
                StyleToken::Error
                | StyleToken::Warning
                | StyleToken::Success
                | StyleToken::Draft => Style::default().add_modifier(Modifier::BOLD),
                StyleToken::TextMuted => Style::default().add_modifier(Modifier::DIM),
                _ => Style::default(),
            };
        }
        if let Self::Custom(palette) = self {
            let foreground = match token {
                StyleToken::Surface => palette.surface,
                StyleToken::Border | StyleToken::TextMuted => palette.muted,
                StyleToken::BorderFocused | StyleToken::Selection => palette.selection,
                StyleToken::Match | StyleToken::Warning => palette.warning,
                StyleToken::Fragment | StyleToken::XmlText => palette.fragment,
                StyleToken::Prompt | StyleToken::XmlTag => palette.prompt,
                StyleToken::Tag | StyleToken::Draft => palette.tag,
                StyleToken::Success => palette.success,
                StyleToken::Error => palette.error,
                StyleToken::Text => palette.text,
            };
            let style = Style::default().fg(foreground);
            return if token == StyleToken::Selection {
                style.add_modifier(Modifier::REVERSED)
            } else {
                style
            };
        }
        let light = self == Self::Light;
        let foreground = match token {
            StyleToken::Surface => {
                if light {
                    Color::White
                } else {
                    Color::Black
                }
            }
            StyleToken::Border | StyleToken::TextMuted => Color::Gray,
            StyleToken::BorderFocused | StyleToken::Selection => Color::Blue,
            StyleToken::Match | StyleToken::Warning => Color::Yellow,
            StyleToken::Fragment | StyleToken::XmlText => Color::Cyan,
            StyleToken::Prompt | StyleToken::XmlTag => Color::Magenta,
            StyleToken::Tag | StyleToken::Draft => Color::LightMagenta,
            StyleToken::Success => Color::Green,
            StyleToken::Error => Color::Red,
            StyleToken::Text => {
                if light {
                    Color::Black
                } else {
                    Color::White
                }
            }
        };
        let style = Style::default().fg(foreground);
        if matches!(token, StyleToken::Selection) {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        }
    }
}

/// 字形能力模式。 / Glyph capability mode.
///
/// <!-- @brief 字形能力模式。 / Glyph capability mode. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphMode {
    /// 使用固定宽度 Unicode 符号。 / Use fixed-width Unicode symbols.
    Unicode,
    /// 仅使用 ASCII 符号。 / Use ASCII-only symbols.
    Ascii,
}

/// 一组固定宽度、非 emoji 的语义字形。 / A semantic, non-emoji glyph set.
///
/// <!-- @brief 一组固定宽度、非 emoji 的语义字形。 / A semantic, non-emoji glyph set. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyphs {
    /// 展开状态字形。 / Expanded-state glyph.
    ///
    /// <!-- @brief 展开状态字形。 / Expanded-state glyph. -->
    pub expanded: &'static str,
    /// 折叠状态字形。 / Collapsed-state glyph.
    ///
    /// <!-- @brief 折叠状态字形。 / Collapsed-state glyph. -->
    pub collapsed: &'static str,
    /// 中间树分支字形。 / Intermediate tree-branch glyph.
    ///
    /// <!-- @brief 中间树分支字形。 / Intermediate tree-branch glyph. -->
    pub branch: &'static str,
    /// 最后树分支字形。 / Last tree-branch glyph.
    ///
    /// <!-- @brief 最后树分支字形。 / Last tree-branch glyph. -->
    pub last_branch: &'static str,
    /// 成功状态字形。 / Success-state glyph.
    ///
    /// <!-- @brief 成功状态字形。 / Success-state glyph. -->
    pub success: &'static str,
    /// 警告状态字形。 / Warning-state glyph.
    ///
    /// <!-- @brief 警告状态字形。 / Warning-state glyph. -->
    pub warning: &'static str,
    /// 错误状态字形。 / Error-state glyph.
    ///
    /// <!-- @brief 错误状态字形。 / Error-state glyph. -->
    pub error: &'static str,
    /// 脏草稿状态字形。 / Dirty-draft glyph.
    ///
    /// <!-- @brief 脏草稿状态字形。 / Dirty-draft glyph. -->
    pub dirty: &'static str,
}

impl Glyphs {
    /// 按能力返回 Unicode 或 ASCII 字形。 / Return Unicode or ASCII glyphs by capability.
    ///
    /// <!-- @brief 按能力返回 Unicode 或 ASCII 字形。 / Return Unicode or ASCII glyphs by capability. -->
    pub const fn for_mode(mode: GlyphMode) -> Self {
        match mode {
            GlyphMode::Unicode => Self {
                expanded: "▾",
                collapsed: "▸",
                branch: "├─",
                last_branch: "└─",
                success: "✓",
                warning: "!",
                error: "×",
                dirty: "●",
            },
            GlyphMode::Ascii => Self {
                expanded: "v",
                collapsed: ">",
                branch: "+-",
                last_branch: "`-",
                success: "OK",
                warning: "!",
                error: "X",
                dirty: "*",
            },
        }
    }
}
