//! 稳定、可序列化的诊断模型。 / Stable serializable diagnostic model.
//!
//! <!-- @brief 稳定、可序列化的诊断模型。 / Stable serializable diagnostic model. -->

use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

/// 源代码中的半开字节区间。 / Half-open byte range in source text.
///
/// <!-- @brief 源代码中的半开字节区间。 / Half-open byte range in source text. -->
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceSpan {
    /// 起始 UTF-8 字节偏移。 / Starting UTF-8 byte offset.
    ///
    /// <!-- @brief 起始 UTF-8 字节偏移。 / Starting UTF-8 byte offset. -->
    pub start: usize,
    /// 区间 UTF-8 字节长度。 / UTF-8 byte length of the range.
    ///
    /// <!-- @brief 区间 UTF-8 字节长度。 / UTF-8 byte length of the range. -->
    pub length: usize,
}

impl SourceSpan {
    /// 从起止偏移构造区间。 / Construct a span from start and end offsets.
    ///
    /// 当 `end` 小于 `start` 时，区间长度规范化为零。 /
    /// When `end` is smaller than `start`, the span length is normalized to zero.
    ///
    /// # Arguments
    ///
    /// - `start`: 起始字节偏移。 / Starting byte offset.
    /// - `end`: 结束字节偏移（不含）。 / Exclusive ending byte offset.
    ///
    /// # Returns
    ///
    /// 规范化后的源区间。 / The normalized source span.
    ///
    /// # Examples
    ///
    /// ```
    /// use promptr::SourceSpan;
    ///
    /// let span = SourceSpan::new(2, 7);
    /// assert_eq!(span.start, 2);
    /// assert_eq!(span.length, 5);
    /// ```
    ///
    /// <!-- @brief 从起止偏移构造区间。 / Construct a span from start and end offsets. -->
    /// <!-- @param start 起始字节偏移。 / Starting byte offset. -->
    /// <!-- @param end 结束字节偏移（不含）。 / Exclusive ending byte offset. -->
    /// <!-- @return 规范化区间。 / Normalized span. -->
    /// <!-- @note `end` 小于 `start` 时长度为零。 / The length is zero when `end` is smaller than `start`. -->
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            length: end.saturating_sub(start),
        }
    }
}

/// 诊断严重级别。 / Diagnostic severity.
///
/// <!-- @brief 诊断严重级别。 / Diagnostic severity. -->
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// 提示信息。 / Informational message.
    ///
    /// <!-- @brief 提示信息。 / Informational message. -->
    Info,
    /// 不阻止当前操作的警告。 / Non-blocking warning.
    ///
    /// <!-- @brief 不阻止当前操作的警告。 / Non-blocking warning. -->
    Warning,
    /// 阻止当前操作的错误。 / Operation-blocking error.
    ///
    /// <!-- @brief 阻止当前操作的错误。 / Operation-blocking error. -->
    Error,
}

/// 诊断分类。 / Diagnostic category.
///
/// <!-- @brief 诊断分类。 / Diagnostic category. -->
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    /// 语法问题。 / Syntax problem.
    ///
    /// <!-- @brief 语法问题。 / Syntax problem. -->
    Syntax,
    /// 领域约束问题。 / Domain invariant problem.
    ///
    /// <!-- @brief 领域约束问题。 / Domain invariant problem. -->
    Domain,
    /// 引用完整性问题。 / Referential-integrity problem.
    ///
    /// <!-- @brief 引用完整性问题。 / Referential-integrity problem. -->
    ReferentialIntegrity,
    /// 调用能力不匹配。 / Invocation-capability mismatch.
    ///
    /// <!-- @brief 调用能力不匹配。 / Invocation-capability mismatch. -->
    Capability,
    /// 并发修订冲突。 / Concurrent-revision conflict.
    ///
    /// <!-- @brief 并发修订冲突。 / Concurrent-revision conflict. -->
    Conflict,
    /// 持久化问题。 / Persistence problem.
    ///
    /// <!-- @brief 持久化问题。 / Persistence problem. -->
    Storage,
    /// 配置问题。 / Configuration problem.
    ///
    /// <!-- @brief 配置问题。 / Configuration problem. -->
    Configuration,
    /// 不支持的格式版本。 / Unsupported format version.
    ///
    /// <!-- @brief 不支持的格式版本。 / Unsupported format version. -->
    Compatibility,
    /// 外部编辑器或输入提供者问题。 / External editor or input-provider problem.
    ///
    /// <!-- @brief 外部编辑器或输入提供者问题。 / External editor or input-provider problem. -->
    External,
    /// 内部一致性问题。 / Internal consistency problem.
    ///
    /// <!-- @brief 内部一致性问题。 / Internal consistency problem. -->
    Internal,
}

/// 与主诊断相关的符号或路径。 / Symbol or path related to a diagnostic.
///
/// <!-- @brief 与主诊断相关的符号或路径。 / Symbol or path related to a diagnostic. -->
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RelatedDiagnostic {
    /// 相关符号（若有）。 / Related symbol, when available.
    ///
    /// <!-- @brief 相关符号（若有）。 / Related symbol, when available. -->
    pub symbol: Option<String>,
    /// 相关路径（若有）。 / Related path, when available.
    ///
    /// <!-- @brief 相关路径（若有）。 / Related path, when available. -->
    pub path: Option<Vec<String>>,
    /// 出现次数（若有）。 / Occurrence count, when available.
    ///
    /// <!-- @brief 出现次数（若有）。 / Occurrence count, when available. -->
    pub occurrences: Option<u64>,
    /// 补充说明。 / Explanatory label.
    ///
    /// <!-- @brief 补充说明。 / Explanatory label. -->
    pub message: String,
}

/// 跨宿主共享的结构化诊断。 / Structured diagnostic shared by all hosts.
///
/// <!-- @brief 跨宿主共享的结构化诊断。 / Structured diagnostic shared by all hosts. -->
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// 稳定错误码。 / Stable diagnostic code.
    ///
    /// <!-- @brief 稳定错误码。 / Stable diagnostic code. -->
    pub code: String,
    /// 诊断分类。 / Diagnostic category.
    ///
    /// <!-- @brief 诊断分类。 / Diagnostic category. -->
    pub category: DiagnosticCategory,
    /// 严重级别。 / Severity.
    ///
    /// <!-- @brief 严重级别。 / Severity. -->
    pub severity: Severity,
    /// 面向用户的说明。 / User-facing explanation.
    ///
    /// <!-- @brief 面向用户的说明。 / User-facing explanation. -->
    pub message: String,
    /// 较少使用的诊断细节，装箱以保持 `Result` 的错误分支紧凑。 /
    /// Less-frequent diagnostic details boxed to keep the `Result` error branch compact.
    ///
    /// <!-- @brief 较少使用的诊断细节，装箱以保持 Result 的错误分支紧凑。 / Less-frequent diagnostic details boxed to keep Result's error branch compact. -->
    #[serde(flatten)]
    pub details: Box<DiagnosticDetails>,
}

/// 诊断的定位、关联与原因细节。 / Location, relation, and cause details of a diagnostic.
///
/// <!-- @brief 诊断的定位、关联与原因细节。 / Location, relation, and cause details of a diagnostic. -->
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticDetails {
    /// 主要源区间。 / Primary source span.
    ///
    /// <!-- @brief 主要源区间。 / Primary source span. -->
    pub span: Option<SourceSpan>,
    /// 输入源名称。 / Input source name.
    ///
    /// <!-- @brief 输入源名称。 / Input source name. -->
    pub source: Option<String>,
    /// 相关实体。 / Related entities.
    ///
    /// <!-- @brief 相关实体。 / Related entities. -->
    pub related: Vec<RelatedDiagnostic>,
    /// 可执行提示。 / Actionable hints.
    ///
    /// <!-- @brief 可执行提示。 / Actionable hints. -->
    pub hints: Vec<String>,
    /// 不作为稳定接口的底层原因链。 / Non-contractual underlying cause chain.
    ///
    /// <!-- @brief 不作为稳定接口的底层原因链。 / Non-contractual underlying cause chain. -->
    pub causes: Vec<String>,
}

impl Deref for Diagnostic {
    type Target = DiagnosticDetails;

    /// 借用诊断细节并保留便捷字段访问。 / Borrow details while preserving convenient field access.
    ///
    /// # Returns
    ///
    /// 诊断细节。 / The diagnostic details.
    ///
    /// <!-- @brief 借用诊断细节并保留便捷字段访问。 / Borrow details while preserving convenient field access. -->
    /// <!-- @return 诊断细节。 / Diagnostic details. -->
    fn deref(&self) -> &Self::Target {
        &self.details
    }
}

impl DerefMut for Diagnostic {
    /// 可变借用诊断细节并保留便捷字段访问。 / Mutably borrow details while preserving convenient field access.
    ///
    /// # Returns
    ///
    /// 可变诊断细节。 / The mutable diagnostic details.
    ///
    /// <!-- @brief 可变借用诊断细节并保留便捷字段访问。 / Mutably borrow details while preserving convenient field access. -->
    /// <!-- @return 可变诊断细节。 / Mutable diagnostic details. -->
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.details
    }
}

impl Diagnostic {
    /// 构造一个错误诊断。 / Construct an error diagnostic.
    ///
    /// # Arguments
    ///
    /// - `code`: 稳定错误码。 / Stable error code.
    /// - `category`: 诊断分类。 / Diagnostic category.
    /// - `message`: 面向用户的说明。 / User-facing explanation.
    ///
    /// # Returns
    ///
    /// 严重级别为 [`Severity::Error`] 的新诊断。 /
    /// A new diagnostic whose severity is [`Severity::Error`].
    ///
    /// # Examples
    ///
    /// ```
    /// use promptr::{Diagnostic, DiagnosticCategory, Severity};
    ///
    /// let diagnostic = Diagnostic::error(
    ///     "PROMPTR_EXAMPLE",
    ///     DiagnosticCategory::Domain,
    ///     "invalid prompt",
    /// );
    /// assert_eq!(diagnostic.severity, Severity::Error);
    /// ```
    ///
    /// <!-- @brief 构造一个错误诊断。 / Construct an error diagnostic. -->
    /// <!-- @param code 稳定错误码。 / Stable error code. -->
    /// <!-- @param category 诊断分类。 / Diagnostic category. -->
    /// <!-- @param message 面向用户的说明。 / User-facing explanation. -->
    /// <!-- @return 新诊断。 / New diagnostic. -->
    pub fn error(
        code: impl Into<String>,
        category: DiagnosticCategory,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            category,
            severity: Severity::Error,
            message: message.into(),
            details: Box::default(),
        }
    }

    /// 附加源区间。 / Attach a source span.
    ///
    /// # Arguments
    ///
    /// - `span`: 要设置的源区间。 / Source span to set.
    ///
    /// # Returns
    ///
    /// 附加了源区间的诊断。 / The diagnostic with the source span attached.
    ///
    /// <!-- @brief 附加源区间。 / Attach a source span. -->
    /// <!-- @param span 源区间。 / Source span. -->
    /// <!-- @return 更新后的诊断。 / Updated diagnostic. -->
    #[must_use]
    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }

    /// 附加可执行提示。 / Attach an actionable hint.
    ///
    /// # Arguments
    ///
    /// - `hint`: 提示文本。 / Hint text.
    ///
    /// # Returns
    ///
    /// 附加了提示的诊断。 / The diagnostic with the hint attached.
    ///
    /// <!-- @brief 附加可执行提示。 / Attach an actionable hint. -->
    /// <!-- @param hint 提示文本。 / Hint text. -->
    /// <!-- @return 更新后的诊断。 / Updated diagnostic. -->
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(hint.into());
        self
    }

    /// 附加底层原因。 / Attach an underlying cause.
    ///
    /// # Arguments
    ///
    /// - `cause`: 原因文本。 / Cause text.
    ///
    /// # Returns
    ///
    /// 附加了底层原因的诊断。 / The diagnostic with the underlying cause attached.
    ///
    /// <!-- @brief 附加底层原因。 / Attach an underlying cause. -->
    /// <!-- @param cause 原因文本。 / Cause text. -->
    /// <!-- @return 更新后的诊断。 / Updated diagnostic. -->
    #[must_use]
    pub fn with_cause(mut self, cause: impl Into<String>) -> Self {
        self.causes.push(cause.into());
        self
    }
}

/// 以结构化诊断为错误项的通用结果。 / General result whose error is a diagnostic.
///
/// <!-- @brief 以结构化诊断为错误项的通用结果。 / General result whose error is a diagnostic. -->
pub type Result<T> = std::result::Result<T, Diagnostic>;
