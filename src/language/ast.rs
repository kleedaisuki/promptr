//! 语言前端产生的语法树与诊断。

/// @brief UTF-8 源码中的半开字节区间 / Half-open byte range in UTF-8 source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct Span {
    /// @brief 起始字节偏移 / Inclusive start byte offset.
    pub start: usize,
    /// @brief 结束字节偏移 / Exclusive end byte offset.
    pub end: usize,
}

impl Span {
    /// @brief 创建源码区间 / Creates a source span.
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// @brief 合并两个源码区间 / Covers two source spans.
    pub const fn cover(self, other: Self) -> Self {
        Self::new(self.start, other.end)
    }
}

/// @brief 带源码位置的值 / A value annotated with its source span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spanned<T> {
    /// @brief 解析后的值 / Parsed value.
    pub value: T,
    /// @brief 原始源码位置 / Original source location.
    pub span: Span,
}

impl<T> Spanned<T> {
    /// @brief 创建带位置的值 / Creates a spanned value.
    pub const fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }
}

/// @brief 完整 DSL 程序 / A complete DSL program.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Program {
    /// @brief 按源码顺序排列的语句 / Statements in source order.
    pub statements: Vec<Spanned<Statement>>,
    /// @brief 整个输入的字节区间 / Byte span of the entire input.
    pub span: Span,
}

/// @brief 搜索字段选择 / Search field selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchField {
    /// @brief 仅标题 / Title only.
    Title,
    /// @brief 仅内容 / Content only.
    Content,
    /// @brief 标题与内容 / Title and content.
    #[default]
    Mixed,
}

/// @brief 列表节点类型过滤器 / Node-kind filter for listing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ListFilter {
    /// @brief 所有节点 / All nodes.
    #[default]
    All,
    /// @brief 仅片段 / Fragments only.
    Fragments,
    /// @brief 仅提示词 / Prompts only.
    Prompts,
}

/// @brief 元数据替换值 / Metadata replacement value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataValue {
    /// @brief 描述文本，空串表示清除 / Description text; empty clears it.
    Description(Spanned<String>),
    /// @brief 完整标签集合，空数组表示清除 / Complete tag set; an empty array clears it.
    Tags(Vec<Spanned<String>>),
}

/// @brief DSL 语句 / A DSL statement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Statement {
    /// @brief 创建或编辑片段 / Creates or edits a fragment.
    Fragment { symbol: Spanned<String> },
    /// @brief 创建或替换提示词组合 / Creates or replaces a prompt composition.
    Prompt {
        symbol: Spanned<String>,
        children: Vec<Spanned<String>>,
    },
    /// @brief 重命名节点 / Renames a node.
    Rename {
        old: Spanned<String>,
        new: Spanned<String>,
    },
    /// @brief 删除节点 / Deletes a node.
    Delete { symbol: Spanned<String> },
    /// @brief 列出节点 / Lists nodes.
    List { filter: ListFilter },
    /// @brief 输出人类可读检查视图 / Prints a human-oriented inspection view.
    Print { symbol: Spanned<String> },
    /// @brief 输出规范 XML / Outputs canonical XML.
    Output { symbol: Spanned<String> },
    /// @brief 全局搜索片段 / Searches the global fragment catalog.
    Search {
        query: Spanned<String>,
        field: SearchField,
    },
    /// @brief 在提示词可达范围内搜索 / Searches within a prompt's reachable fragments.
    Find {
        query: Spanned<String>,
        prompt: Spanned<String>,
        field: SearchField,
    },
    /// @brief 替换用户元数据 / Replaces user metadata.
    Metadata {
        symbol: Spanned<String>,
        value: MetadataValue,
    },
}

/// @brief 稳定的语法诊断代码 / Stable syntax diagnostic code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticCode {
    /// @brief 无效词法字符 / Invalid lexical character.
    InvalidCharacter,
    /// @brief 无效符号 / Invalid symbol.
    InvalidSymbol,
    /// @brief 无效字符串转义 / Invalid string escape.
    InvalidEscape,
    /// @brief 非法语法 / Invalid syntax.
    UnexpectedToken,
    /// @brief 空提示词 / Empty prompt.
    EmptyPrompt,
    /// @brief Shell 转义仅属于 REPL / Shell escapes are REPL-only.
    ReplOnlyShell,
}

impl DiagnosticCode {
    /// @brief 返回稳定机器代码 / Returns the stable machine code.
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

/// @brief 单个解析诊断 / A single parse diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseDiagnostic {
    /// @brief 稳定诊断代码 / Stable diagnostic code.
    pub code: DiagnosticCode,
    /// @brief 面向用户的说明 / User-facing explanation.
    pub message: String,
    /// @brief 首要源码位置 / Primary source span.
    pub span: Span,
}

/// @brief 解析状态，区分可继续输入与确定错误 / Parse status distinguishing continuation from definite error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseOutcome {
    /// @brief 完整且合法的程序 / Complete and valid program.
    Complete(Program),
    /// @brief 输入前缀合法但仍需更多文本 / Valid prefix requiring more input.
    Incomplete(ParseDiagnostic),
    /// @brief 已确定无法由追加输入修复 / Definitely invalid input.
    Invalid(ParseDiagnostic),
}
