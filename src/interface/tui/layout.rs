//! 响应式终端几何计算。 / Responsive terminal geometry.

use ratatui::layout::Rect;

/// 响应式布局等级。 / Responsive layout class.
///
/// <!-- @brief 响应式布局等级。 / Responsive layout class. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutClass {
    /// 三栏完整布局。 / Full three-pane layout.
    ThreePane,
    /// 双栏布局。 / Two-pane layout.
    TwoPane,
    /// 单栏布局。 / Single-pane layout.
    SinglePane,
    /// 极小终端的聚焦布局。 / Focused layout for tiny terminals.
    Focused,
}

impl LayoutClass {
    /// 根据终端尺寸选择布局。 / Select a layout from terminal dimensions.
    ///
    /// # Arguments / 参数
    ///
    /// - `width` — 终端列数。 / Terminal columns.
    /// - `height` — 终端行数。 / Terminal rows.
    ///
    /// # Returns / 返回值
    ///
    /// 确定且无重叠的布局等级。 / Deterministic non-overlapping layout class.
    ///
    /// <!-- @brief 根据终端尺寸选择布局。 / Select a layout from terminal dimensions. -->
    /// <!-- @param width 终端列数。 / Terminal columns. -->
    /// <!-- @param height 终端行数。 / Terminal rows. -->
    /// <!-- @return 确定且无重叠的布局等级。 / Deterministic non-overlapping layout class. -->
    pub const fn for_size(width: u16, height: u16) -> Self {
        if width < 60 || height < 16 {
            Self::Focused
        } else if width >= 140 && height >= 32 {
            Self::ThreePane
        } else if width >= 90 && height >= 24 {
            Self::TwoPane
        } else {
            Self::SinglePane
        }
    }
}

/// 一次 view 计算所共享的窗格矩形。 / Pane rectangles shared by one view pass.
///
/// <!-- @brief 一次 view 计算所共享的窗格矩形。 / Pane rectangles shared by one view pass. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// 布局等级。 / Layout class.
    pub class: LayoutClass,
    /// 可用内容区。 / Available content area.
    pub content: Rect,
    /// 目录窗格（若可见）。 / Catalog pane when visible.
    pub catalog: Option<Rect>,
    /// 预览窗格（若可见）。 / Preview pane when visible.
    pub preview: Option<Rect>,
    /// 元数据窗格（若可见）。 / Metadata pane when visible.
    pub metadata: Option<Rect>,
    /// 底部状态区。 / Bottom status area.
    pub status: Rect,
}

impl Layout {
    /// 计算经过饱和减法保护的布局。 / Compute layout with saturating geometry.
    ///
    /// # Arguments / 参数
    ///
    /// - `area` — 完整终端矩形。 / Full terminal rectangle.
    ///
    /// # Returns / 返回值
    ///
    /// 供渲染和命中测试共同使用的矩形。 / Rectangles shared by rendering and hit testing.
    ///
    /// <!-- @brief 计算经过饱和减法保护的布局。 / Compute layout with saturating geometry. -->
    /// <!-- @param area 完整终端矩形。 / Full terminal rectangle. -->
    /// <!-- @return 供渲染和命中测试共同使用的矩形。 / Rectangles shared by rendering and hit testing. -->
    pub fn compute(area: Rect) -> Self {
        let class = LayoutClass::for_size(area.width, area.height);
        let status_height = if area.height == 0 { 0 } else { 1 };
        let content = Rect::new(
            area.x,
            area.y,
            area.width,
            area.height.saturating_sub(status_height),
        );
        let status = Rect::new(
            area.x,
            area.y.saturating_add(content.height),
            area.width,
            status_height,
        );
        let mut layout = Self {
            class,
            content,
            catalog: None,
            preview: None,
            metadata: None,
            status,
        };
        match class {
            LayoutClass::ThreePane => {
                let catalog = content.width * 34 / 100;
                let metadata = content.width * 22 / 100;
                let preview = content
                    .width
                    .saturating_sub(catalog)
                    .saturating_sub(metadata);
                layout.catalog = Some(Rect::new(content.x, content.y, catalog, content.height));
                layout.preview = Some(Rect::new(
                    content.x + catalog,
                    content.y,
                    preview,
                    content.height,
                ));
                layout.metadata = Some(Rect::new(
                    content.x + catalog + preview,
                    content.y,
                    metadata,
                    content.height,
                ));
            }
            LayoutClass::TwoPane => {
                let catalog = content.width * 38 / 100;
                layout.catalog = Some(Rect::new(content.x, content.y, catalog, content.height));
                layout.preview = Some(Rect::new(
                    content.x + catalog,
                    content.y,
                    content.width - catalog,
                    content.height,
                ));
            }
            LayoutClass::SinglePane | LayoutClass::Focused => layout.catalog = Some(content),
        }
        layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints_are_exhaustive_and_exact() {
        assert_eq!(LayoutClass::for_size(140, 32), LayoutClass::ThreePane);
        assert_eq!(LayoutClass::for_size(139, 32), LayoutClass::TwoPane);
        assert_eq!(LayoutClass::for_size(90, 24), LayoutClass::TwoPane);
        assert_eq!(LayoutClass::for_size(89, 24), LayoutClass::SinglePane);
        assert_eq!(LayoutClass::for_size(200, 23), LayoutClass::SinglePane);
        assert_eq!(LayoutClass::for_size(59, 100), LayoutClass::Focused);
        assert_eq!(LayoutClass::for_size(200, 15), LayoutClass::Focused);
    }
}
