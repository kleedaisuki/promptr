//! 语义样式和字形能力适配。 / Semantic styles and glyph capability adaptation.

use ratatui::style::{Color, Modifier, Style};

/// @brief 控件请求的语义样式，而非具体颜色。 / Semantic style requested by widgets instead of a raw color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleToken {
    Surface,
    Border,
    BorderFocused,
    Text,
    TextMuted,
    Selection,
    Match,
    Fragment,
    Prompt,
    Tag,
    Success,
    Warning,
    Error,
    Draft,
    XmlTag,
    XmlText,
}

/// @brief 完整主题能力。 / Complete theme capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    Monochrome,
    /// 完整自定义语义调色板。 / Complete custom semantic palette.
    Custom(CustomPalette),
}

/// @brief 已解析的自定义主题颜色。 / Parsed custom theme colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomPalette {
    /// @brief 表面色。 / Surface color.
    pub surface: Color,
    /// @brief 正文色。 / Text color.
    pub text: Color,
    /// @brief 弱化色。 / Muted color.
    pub muted: Color,
    /// @brief 选中色。 / Selection color.
    pub selection: Color,
    /// @brief Fragment 色。 / Fragment color.
    pub fragment: Color,
    /// @brief Prompt 色。 / Prompt color.
    pub prompt: Color,
    /// @brief 标签色。 / Tag color.
    pub tag: Color,
    /// @brief 成功色。 / Success color.
    pub success: Color,
    /// @brief 警告色。 / Warning color.
    pub warning: Color,
    /// @brief 错误色。 / Error color.
    pub error: Color,
}

impl Theme {
    /// @brief 将语义标记降低为终端样式。 / Lower a semantic token to terminal style.
    /// @param token 语义样式标记。 / Semantic style token.
    /// @return 不依赖业务状态的终端样式。 / Terminal style independent of business state.
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

/// @brief 字形能力模式。 / Glyph capability mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphMode {
    Unicode,
    Ascii,
}

/// @brief 一组固定宽度、非 emoji 的语义字形。 / A semantic, non-emoji glyph set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyphs {
    /// @brief 展开状态字形。 / Expanded-state glyph.
    pub expanded: &'static str,
    /// @brief 折叠状态字形。 / Collapsed-state glyph.
    pub collapsed: &'static str,
    /// @brief 中间树分支字形。 / Intermediate tree-branch glyph.
    pub branch: &'static str,
    /// @brief 最后树分支字形。 / Last tree-branch glyph.
    pub last_branch: &'static str,
    /// @brief 成功状态字形。 / Success-state glyph.
    pub success: &'static str,
    /// @brief 警告状态字形。 / Warning-state glyph.
    pub warning: &'static str,
    /// @brief 错误状态字形。 / Error-state glyph.
    pub error: &'static str,
    /// @brief 脏草稿状态字形。 / Dirty-draft glyph.
    pub dirty: &'static str,
}

impl Glyphs {
    /// @brief 按能力返回 Unicode 或 ASCII 字形。 / Return Unicode or ASCII glyphs by capability.
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
