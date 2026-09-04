use std::fmt;

use super::NodeId;

/// 领域不变量错误。 / Domain-invariant error.
///
/// <!-- @brief 领域不变量错误。 / Domain-invariant error. -->
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// 数值不是有效的正整数。 / A numeric identity is not positive.
    ///
    /// <!-- @brief 数值不是有效的正整数。 / A numeric identity is not positive. -->
    InvalidPositiveInteger {
        /// 标识数值种类。 / Identity value kind.
        ///
        /// <!-- @brief 标识数值种类。 / Identity value kind. -->
        kind: &'static str,
        /// 被拒绝的数值。 / Rejected value.
        ///
        /// <!-- @brief 被拒绝的数值。 / Rejected value. -->
        value: i128,
    },
    /// 符号不符合严格的 XML 名子集。 / A symbol violates the strict XML-name subset.
    ///
    /// <!-- @brief 符号不符合严格的 XML 名子集。 / A symbol violates the strict XML-name subset. -->
    InvalidSymbol(
        /// 被拒绝的符号文本。 / Rejected symbol text.
        ///
        /// <!-- @brief 被拒绝的符号文本。 / Rejected symbol text. -->
        String,
    ),
    /// 文本包含 XML 1.0 禁止的字符。 / Text contains an XML 1.0-forbidden character.
    ///
    /// <!-- @brief 文本包含 XML 1.0 禁止的字符。 / Text contains an XML 1.0-forbidden character. -->
    InvalidXmlCharacter {
        /// 被拒绝的 Unicode 标量值。 / Rejected Unicode scalar value.
        ///
        /// <!-- @brief 被拒绝的 Unicode 标量值。 / Rejected Unicode scalar value. -->
        character: char,
        /// 字符在 UTF-8 文本中的字节偏移。 / Character byte offset in the UTF-8 text.
        ///
        /// <!-- @brief 字符在 UTF-8 文本中的字节偏移。 / Character byte offset in the UTF-8 text. -->
        byte_offset: usize,
    },
    /// Prompt 的子节点列表为空。 / A prompt child list is empty.
    ///
    /// <!-- @brief Prompt 的子节点列表为空。 / A prompt child list is empty. -->
    EmptyChildren,
    /// 标签为空或仅含空白。 / A tag is empty or whitespace-only.
    ///
    /// <!-- @brief 标签为空或仅含空白。 / A tag is empty or whitespace-only. -->
    EmptyTag,
    /// 节点头类型与节点体类型不一致。 / Node header and body kinds disagree.
    ///
    /// <!-- @brief 节点头类型与节点体类型不一致。 / Node header and body kinds disagree. -->
    KindMismatch {
        /// 节点头声明的种类。 / Kind declared by the node header.
        ///
        /// <!-- @brief 节点头声明的种类。 / Kind declared by the node header. -->
        declared: super::NodeKind,
        /// 根据节点体得到的实际种类。 / Actual kind derived from the node body.
        ///
        /// <!-- @brief 根据节点体得到的实际种类。 / Actual kind derived from the node body. -->
        actual: super::NodeKind,
    },
    /// 快照包含重复的节点标识。 / A snapshot contains a duplicate node id.
    ///
    /// <!-- @brief 快照包含重复的节点标识。 / A snapshot contains a duplicate node id. -->
    DuplicateNodeId(
        /// 重复的节点标识。 / Duplicate node identity.
        ///
        /// <!-- @brief 重复的节点标识。 / Duplicate node identity. -->
        NodeId,
    ),
    /// 快照包含重复的符号。 / A snapshot contains a duplicate symbol.
    ///
    /// <!-- @brief 快照包含重复的符号。 / A snapshot contains a duplicate symbol. -->
    DuplicateSymbol(
        /// 重复的符号文本。 / Duplicate symbol text.
        ///
        /// <!-- @brief 重复的符号文本。 / Duplicate symbol text. -->
        String,
    ),
    /// 图引用了不存在的节点。 / The graph references a missing node.
    ///
    /// <!-- @brief 图引用了不存在的节点。 / The graph references a missing node. -->
    MissingNode(
        /// 无法解析的节点标识。 / Unresolved node identity.
        ///
        /// <!-- @brief 无法解析的节点标识。 / Unresolved node identity. -->
        NodeId,
    ),
    /// 图中存在有向环。 / The graph contains a directed cycle.
    ///
    /// <!-- @brief 图中存在有向环。 / The graph contains a directed cycle. -->
    CycleDetected {
        /// 检测到环的位置。 / Node where the cycle was detected.
        ///
        /// <!-- @brief 检测到环的位置。 / Node where the cycle was detected. -->
        at: NodeId,
    },
    /// 展开路径计数超出 u64。 / Expanded path count exceeds u64.
    ///
    /// <!-- @brief 展开路径计数超出 u64。 / Expanded path count exceeds u64. -->
    OccurrenceOverflow {
        /// 计数溢出的节点。 / Node whose count overflowed.
        ///
        /// <!-- @brief 计数溢出的节点。 / Node whose count overflowed. -->
        at: NodeId,
    },
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPositiveInteger { kind, value } => {
                write!(formatter, "{kind} must be positive, got {value}")
            }
            Self::InvalidSymbol(value) => write!(formatter, "invalid symbol `{value}`"),
            Self::InvalidXmlCharacter {
                character,
                byte_offset,
            } => write!(
                formatter,
                "character U+{:04X} at byte {byte_offset} is not valid XML 1.0 text",
                *character as u32
            ),
            Self::EmptyChildren => formatter.write_str("a prompt must contain at least one child"),
            Self::EmptyTag => formatter.write_str("a tag must not be empty or whitespace-only"),
            Self::KindMismatch { declared, actual } => write!(
                formatter,
                "declared node kind {declared:?} does not match body kind {actual:?}"
            ),
            Self::DuplicateNodeId(id) => write!(formatter, "duplicate node id {id}"),
            Self::DuplicateSymbol(symbol) => write!(formatter, "duplicate symbol `{symbol}`"),
            Self::MissingNode(id) => write!(formatter, "missing node {id}"),
            Self::CycleDetected { at } => write!(formatter, "cycle detected at node {at}"),
            Self::OccurrenceOverflow { at } => {
                write!(formatter, "occurrence count overflow at node {at}")
            }
        }
    }
}

impl std::error::Error for DomainError {}
