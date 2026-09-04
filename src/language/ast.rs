//! 语言前端产生的语法树与诊断。 / Syntax trees and diagnostics produced by the language frontend.

/// UTF-8 源码中的半开字节区间 / Half-open byte range in UTF-8 source.
///
/// <!-- @brief UTF-8 源码中的半开字节区间 / Half-open byte range in UTF-8 source. -->
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct Span {
    /// 起始字节偏移 / Inclusive start byte offset.
    ///
    /// <!-- @brief 起始字节偏移 / Inclusive start byte offset. -->
    pub start: usize,
    /// 结束字节偏移 / Exclusive end byte offset.
    ///
    /// <!-- @brief 结束字节偏移 / Exclusive end byte offset. -->
    pub end: usize,
}

impl Span {
    /// 创建源码区间 / Creates a source span.
    ///
    /// <!-- @brief 创建源码区间 / Creates a source span. -->
    ///
    /// # Arguments
    ///
    /// - `start`：起始字节偏移。 / Inclusive start byte offset.
    /// - `end`：结束字节偏移。 / Exclusive end byte offset.
    ///
    /// <!-- @param start 起始字节偏移。 / Inclusive start byte offset. -->
    /// <!-- @param end 结束字节偏移。 / Exclusive end byte offset. -->
    ///
    /// # Returns
    ///
    /// 由两个偏移构成的区间。 / A span formed from the two offsets.
    ///
    /// <!-- @return 由两个偏移构成的区间。 / A span formed from the two offsets. -->
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// 合并两个源码区间 / Covers two source spans.
    ///
    /// <!-- @brief 合并两个源码区间 / Covers two source spans. -->
    ///
    /// # Arguments
    ///
    /// - `other`：提供结束偏移的区间。 / Span providing the end offset.
    ///
    /// <!-- @param other 提供结束偏移的区间。 / Span providing the end offset. -->
    ///
    /// # Returns
    ///
    /// 从 `self.start` 到 `other.end` 的区间。 / The span from `self.start` to `other.end`.
    ///
    /// <!-- @return 从 self.start 到 other.end 的区间。 / The span from self.start to other.end. -->
    pub const fn cover(self, other: Self) -> Self {
        Self::new(self.start, other.end)
    }
}

/// 带源码位置的值 / A value annotated with its source span.
///
/// <!-- @brief 带源码位置的值 / A value annotated with its source span. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spanned<T> {
    /// 解析后的值 / Parsed value.
    ///
    /// <!-- @brief 解析后的值 / Parsed value. -->
    pub value: T,
    /// 原始源码位置 / Original source location.
    ///
    /// <!-- @brief 原始源码位置 / Original source location. -->
    pub span: Span,
}

impl<T> Spanned<T> {
    /// 创建带位置的值 / Creates a spanned value.
    ///
    /// <!-- @brief 创建带位置的值 / Creates a spanned value. -->
    ///
    /// # Arguments
    ///
    /// - `value`：要标注的值。 / Value to annotate.
    /// - `span`：值在源码中的区间。 / Value's source span.
    ///
    /// <!-- @param value 要标注的值。 / Value to annotate. -->
    /// <!-- @param span 值在源码中的区间。 / Value's source span. -->
    ///
    /// # Returns
    ///
    /// 带源码位置的值。 / The source-annotated value.
    ///
    /// <!-- @return 带源码位置的值。 / The source-annotated value. -->
    pub const fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }
}

/// 完整 DSL 程序 / A complete DSL program.
///
/// <!-- @brief 完整 DSL 程序 / A complete DSL program. -->
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Program {
    /// 按源码顺序排列的语句 / Statements in source order.
    ///
    /// <!-- @brief 按源码顺序排列的语句 / Statements in source order. -->
    pub statements: Vec<Spanned<Statement>>,
    /// 整个输入的字节区间 / Byte span of the entire input.
    ///
    /// <!-- @brief 整个输入的字节区间 / Byte span of the entire input. -->
    pub span: Span,
}

/// 搜索字段选择 / Search field selection.
///
/// <!-- @brief 搜索字段选择 / Search field selection. -->
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchField {
    /// 仅标题 / Title only.
    ///
    /// <!-- @brief 仅标题 / Title only. -->
    Title,
    /// 仅内容 / Content only.
    ///
    /// <!-- @brief 仅内容 / Content only. -->
    Content,
    /// 标题与内容 / Title and content.
    ///
    /// <!-- @brief 标题与内容 / Title and content. -->
    #[default]
    Mixed,
}

/// 列表节点类型过滤器 / Node-kind filter for listing.
///
/// <!-- @brief 列表节点类型过滤器 / Node-kind filter for listing. -->
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ListFilter {
    /// 所有节点 / All nodes.
    ///
    /// <!-- @brief 所有节点 / All nodes. -->
    #[default]
    All,
    /// 仅片段 / Fragments only.
    ///
    /// <!-- @brief 仅片段 / Fragments only. -->
    Fragments,
    /// 仅提示词 / Prompts only.
    ///
    /// <!-- @brief 仅提示词 / Prompts only. -->
    Prompts,
}

/// 元数据替换值 / Metadata replacement value.
///
/// <!-- @brief 元数据替换值 / Metadata replacement value. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataValue {
    /// 描述文本，空串表示清除 / Description text; empty clears it.
    ///
    /// <!-- @brief 描述文本，空串表示清除 / Description text; empty clears it. -->
    Description(Spanned<String>),
    /// 完整标签集合，空数组表示清除 / Complete tag set; an empty array clears it.
    ///
    /// <!-- @brief 完整标签集合，空数组表示清除 / Complete tag set; an empty array clears it. -->
    Tags(Vec<Spanned<String>>),
}

/// DSL 语句 / A DSL statement.
///
/// <!-- @brief DSL 语句 / A DSL statement. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Statement {
    /// 创建或编辑片段 / Creates or edits a fragment.
    ///
    /// <!-- @brief 创建或编辑片段 / Creates or edits a fragment. -->
    Fragment {
        /// 片段符号。 / Fragment symbol.
        ///
        /// <!-- @brief 片段符号。 / Fragment symbol. -->
        symbol: Spanned<String>,
    },
    /// 创建或替换提示词组合 / Creates or replaces a prompt composition.
    ///
    /// <!-- @brief 创建或替换提示词组合 / Creates or replaces a prompt composition. -->
    Prompt {
        /// 提示词符号。 / Prompt symbol.
        ///
        /// <!-- @brief 提示词符号。 / Prompt symbol. -->
        symbol: Spanned<String>,
        /// 按展开顺序排列的子符号。 / Child symbols in expansion order.
        ///
        /// <!-- @brief 按展开顺序排列的子符号。 / Child symbols in expansion order. -->
        children: Vec<Spanned<String>>,
    },
    /// 重命名节点 / Renames a node.
    ///
    /// <!-- @brief 重命名节点 / Renames a node. -->
    Rename {
        /// 当前符号。 / Current symbol.
        ///
        /// <!-- @brief 当前符号。 / Current symbol. -->
        old: Spanned<String>,
        /// 目标符号。 / Destination symbol.
        ///
        /// <!-- @brief 目标符号。 / Destination symbol. -->
        new: Spanned<String>,
    },
    /// 删除节点 / Deletes a node.
    ///
    /// <!-- @brief 删除节点 / Deletes a node. -->
    Delete {
        /// 要删除的符号。 / Symbol to delete.
        ///
        /// <!-- @brief 要删除的符号。 / Symbol to delete. -->
        symbol: Spanned<String>,
    },
    /// 列出节点 / Lists nodes.
    ///
    /// <!-- @brief 列出节点 / Lists nodes. -->
    List {
        /// 节点类型过滤器。 / Node-kind filter.
        ///
        /// <!-- @brief 节点类型过滤器。 / Node-kind filter. -->
        filter: ListFilter,
    },
    /// 输出人类可读检查视图 / Prints a human-oriented inspection view.
    ///
    /// <!-- @brief 输出人类可读检查视图 / Prints a human-oriented inspection view. -->
    Print {
        /// 要检查的符号。 / Symbol to inspect.
        ///
        /// <!-- @brief 要检查的符号。 / Symbol to inspect. -->
        symbol: Spanned<String>,
    },
    /// 输出规范 XML / Outputs canonical XML.
    ///
    /// <!-- @brief 输出规范 XML / Outputs canonical XML. -->
    Output {
        /// 要展开的符号。 / Symbol to expand.
        ///
        /// <!-- @brief 要展开的符号。 / Symbol to expand. -->
        symbol: Spanned<String>,
    },
    /// 全局搜索片段 / Searches the global fragment catalog.
    ///
    /// <!-- @brief 全局搜索片段 / Searches the global fragment catalog. -->
    Search {
        /// 模糊查询文本。 / Fuzzy query text.
        ///
        /// <!-- @brief 模糊查询文本。 / Fuzzy query text. -->
        query: Spanned<String>,
        /// 显式字段；None 表示延迟到运行时配置。 / Explicit field; None defers to runtime configuration.
        ///
        /// <!-- @brief 显式字段；None 表示延迟到运行时配置。 / Explicit field; None defers to runtime configuration. -->
        field: Option<SearchField>,
    },
    /// 在提示词可达范围内搜索 / Searches within a prompt's reachable fragments.
    ///
    /// <!-- @brief 在提示词可达范围内搜索 / Searches within a prompt's reachable fragments. -->
    Find {
        /// 模糊查询文本。 / Fuzzy query text.
        ///
        /// <!-- @brief 模糊查询文本。 / Fuzzy query text. -->
        query: Spanned<String>,
        /// 限定可达范围的提示词。 / Prompt delimiting the reachable scope.
        ///
        /// <!-- @brief 限定可达范围的提示词。 / Prompt delimiting the reachable scope. -->
        prompt: Spanned<String>,
        /// 显式字段；None 表示延迟到运行时配置。 / Explicit field; None defers to runtime configuration.
        ///
        /// <!-- @brief 显式字段；None 表示延迟到运行时配置。 / Explicit field; None defers to runtime configuration. -->
        field: Option<SearchField>,
    },
    /// 替换用户元数据 / Replaces user metadata.
    ///
    /// <!-- @brief 替换用户元数据 / Replaces user metadata. -->
    Metadata {
        /// 要更新的节点符号。 / Symbol of the node to update.
        ///
        /// <!-- @brief 要更新的节点符号。 / Symbol of the node to update. -->
        symbol: Spanned<String>,
        /// 要完整替换的元数据值。 / Metadata value to replace in full.
        ///
        /// <!-- @brief 要完整替换的元数据值。 / Metadata value to replace in full. -->
        value: MetadataValue,
    },
}

/// 稳定的语法诊断代码 / Stable syntax diagnostic code.
///
/// <!-- @brief 稳定的语法诊断代码 / Stable syntax diagnostic code. -->
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticCode {
    /// 无效词法字符 / Invalid lexical character.
    ///
    /// <!-- @brief 无效词法字符 / Invalid lexical character. -->
    InvalidCharacter,
    /// 无效符号 / Invalid symbol.
    ///
    /// <!-- @brief 无效符号 / Invalid symbol. -->
    InvalidSymbol,
    /// 无效字符串转义 / Invalid string escape.
    ///
    /// <!-- @brief 无效字符串转义 / Invalid string escape. -->
    InvalidEscape,
    /// 非法语法 / Invalid syntax.
    ///
    /// <!-- @brief 非法语法 / Invalid syntax. -->
    UnexpectedToken,
    /// 空提示词 / Empty prompt.
    ///
    /// <!-- @brief 空提示词 / Empty prompt. -->
    EmptyPrompt,
    /// Shell 转义仅属于 REPL / Shell escapes are REPL-only.
    ///
    /// <!-- @brief Shell 转义仅属于 REPL / Shell escapes are REPL-only. -->
    ReplOnlyShell,
}

impl DiagnosticCode {
    /// 返回稳定机器代码 / Returns the stable machine code.
    ///
    /// <!-- @brief 返回稳定机器代码 / Returns the stable machine code. -->
    ///
    /// # Returns
    ///
    /// 与诊断变体对应的稳定错误码。 / Stable error code corresponding to the diagnostic variant.
    ///
    /// <!-- @return 与诊断变体对应的稳定错误码。 / Stable error code corresponding to the diagnostic variant. -->
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidCharacter => "E0001",
            Self::InvalidSymbol => "E0002",
            Self::InvalidEscape => "E0003",
            Self::UnexpectedToken => "E0004",
            Self::EmptyPrompt => "E0005",
            Self::ReplOnlyShell => "E0006",
        }
    }
}

/// 单个解析诊断 / A single parse diagnostic.
///
/// <!-- @brief 单个解析诊断 / A single parse diagnostic. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseDiagnostic {
    /// 稳定诊断代码 / Stable diagnostic code.
    ///
    /// <!-- @brief 稳定诊断代码 / Stable diagnostic code. -->
    pub code: DiagnosticCode,
    /// 面向用户的说明 / User-facing explanation.
    ///
    /// <!-- @brief 面向用户的说明 / User-facing explanation. -->
    pub message: String,
    /// 首要源码位置 / Primary source span.
    ///
    /// <!-- @brief 首要源码位置 / Primary source span. -->
    pub span: Span,
}

/// 解析状态，区分可继续输入与确定错误 / Parse status distinguishing continuation from definite error.
///
/// <!-- @brief 解析状态，区分可继续输入与确定错误 / Parse status distinguishing continuation from definite error. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseOutcome {
    /// 完整且合法的程序 / Complete and valid program.
    ///
    /// <!-- @brief 完整且合法的程序 / Complete and valid program. -->
    Complete(Program),
    /// 输入前缀合法但仍需更多文本 / Valid prefix requiring more input.
    ///
    /// <!-- @brief 输入前缀合法但仍需更多文本 / Valid prefix requiring more input. -->
    Incomplete(ParseDiagnostic),
    /// 已确定无法由追加输入修复 / Definitely invalid input.
    ///
    /// <!-- @brief 已确定无法由追加输入修复 / Definitely invalid input. -->
    Invalid(ParseDiagnostic),
}
