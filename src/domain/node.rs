use std::{
    fmt,
    num::{NonZeroI64, NonZeroU64},
};

use serde::{Deserialize, Serialize};

use super::{DomainError, Metadata};

/// @brief 稳定的内部节点标识。 / Stable internal node identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct NodeId(NonZeroI64);

impl NodeId {
    /// @brief 从正 i64 构造标识。 / Construct an identity from a positive i64.
    /// @param value 持久化整数。 / Persistent integer.
    /// @return 节点标识或领域错误。 / Node identity or domain error.
    pub fn new(value: i64) -> Result<Self, DomainError> {
        NonZeroI64::new(value)
            .filter(|value| value.get() > 0)
            .map(Self)
            .ok_or(DomainError::InvalidPositiveInteger {
                kind: "node id",
                value: value as i128,
            })
    }

    /// @brief 返回持久化整数。 / Return the persistent integer.
    /// @return 正 i64。 / Positive i64.
    pub const fn get(self) -> i64 {
        self.0.get()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(formatter)
    }
}

impl TryFrom<i64> for NodeId {
    type Error = DomainError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<NodeId> for i64 {
    fn from(value: NodeId) -> Self {
        value.get()
    }
}

/// @brief 节点的乐观并发修订号。 / Optimistic-concurrency revision of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct Revision(NonZeroU64);

impl Revision {
    /// @brief 构造正修订号。 / Construct a positive revision.
    /// @param value 修订号。 / Revision number.
    /// @return 修订号或领域错误。 / Revision or domain error.
    pub fn new(value: u64) -> Result<Self, DomainError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(DomainError::InvalidPositiveInteger {
                kind: "revision",
                value: 0,
            })
    }
    /// @brief 返回修订号。 / Return the revision number.
    /// @return 正 u64。 / Positive u64.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
    /// @brief 计算下一修订号。 / Compute the next revision.
    /// @return 下一修订号，溢出时无值。 / Next revision, or none on overflow.
    pub fn checked_next(self) -> Option<Self> {
        self.get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .map(Self)
    }
}

impl TryFrom<u64> for Revision {
    type Error = DomainError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Revision> for u64 {
    fn from(value: Revision) -> Self {
        value.get()
    }
}

/// @brief 同时合法于 DSL 与 XML 标签的符号。 / Symbol valid in both the DSL and XML tags.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Symbol(String);

impl Symbol {
    /// @brief 按手写 ASCII 规则校验并构造符号。 / Validate with the handwritten ASCII rule and construct a symbol.
    /// @param value 候选符号。 / Candidate symbol.
    /// @return 有效符号或领域错误。 / Valid symbol or domain error.
    /// @note 规则等价于 `[A-Za-z_][A-Za-z0-9_-]*`。 / Rule is equivalent to `[A-Za-z_][A-Za-z0-9_-]*`.
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        let mut bytes = value.bytes();
        let first_valid = bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
        if !first_valid
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(DomainError::InvalidSymbol(value));
        }
        Ok(Self(value))
    }
    /// @brief 借用符号文本。 / Borrow symbol text.
    /// @return 符号文本。 / Symbol text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// @brief 取出符号文本。 / Consume and return symbol text.
    /// @return 符号文本。 / Symbol text.
    pub fn into_inner(self) -> String {
        self.0
    }
}
impl TryFrom<String> for Symbol {
    type Error = DomainError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<Symbol> for String {
    fn from(value: Symbol) -> Self {
        value.0
    }
}
impl fmt::Display for Symbol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// @brief 已验证的 XML 1.0 文本。 / Validated XML 1.0 text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct XmlText(String);

impl XmlText {
    /// @brief 验证并构造 XML 文本。 / Validate and construct XML text.
    /// @param value UTF-8 文本。 / UTF-8 text.
    /// @return 已验证文本或领域错误。 / Validated text or domain error.
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if let Some((byte_offset, character)) = value
            .char_indices()
            .find(|(_, character)| !is_xml_10_char(*character))
        {
            return Err(DomainError::InvalidXmlCharacter {
                character,
                byte_offset,
            });
        }
        Ok(Self(value))
    }
    /// @brief 借用文本。 / Borrow the text.
    /// @return XML 文本。 / XML text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// @brief 取出文本。 / Consume and return text.
    /// @return XML 文本。 / XML text.
    pub fn into_inner(self) -> String {
        self.0
    }
}
impl TryFrom<String> for XmlText {
    type Error = DomainError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<XmlText> for String {
    fn from(value: XmlText) -> Self {
        value.0
    }
}

/// @brief 判断字符是否可出现在 XML 1.0 第五版文本中。 / Test whether a character is allowed by XML 1.0 Fifth Edition.
/// @param character Unicode 标量值。 / Unicode scalar value.
/// @return 若允许则为真。 / True when allowed.
pub const fn is_xml_10_char(character: char) -> bool {
    matches!(character as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

/// @brief 节点种类。 / Node kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    Fragment,
    Prompt,
}

/// @brief 至少包含一个条目的有序子节点列表。 / Ordered child list containing at least one entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<NodeId>", into = "Vec<NodeId>")]
pub struct NonEmptyChildren(Vec<NodeId>);

impl NonEmptyChildren {
    /// @brief 构造非空子列表并保留重复项与顺序。 / Construct a non-empty child list preserving duplicates and order.
    /// @param children 子节点标识。 / Child identities.
    /// @return 非空列表或领域错误。 / Non-empty list or domain error.
    pub fn new(children: Vec<NodeId>) -> Result<Self, DomainError> {
        if children.is_empty() {
            Err(DomainError::EmptyChildren)
        } else {
            Ok(Self(children))
        }
    }
    /// @brief 借用子节点切片。 / Borrow the child slice.
    /// @return 有序且可重复的子节点。 / Ordered, duplicate-preserving children.
    pub fn as_slice(&self) -> &[NodeId] {
        &self.0
    }
    /// @brief 取出子节点向量。 / Consume and return the child vector.
    /// @return 子节点向量。 / Child vector.
    pub fn into_vec(self) -> Vec<NodeId> {
        self.0
    }
}
impl TryFrom<Vec<NodeId>> for NonEmptyChildren {
    type Error = DomainError;
    fn try_from(value: Vec<NodeId>) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<NonEmptyChildren> for Vec<NodeId> {
    fn from(value: NonEmptyChildren) -> Self {
        value.0
    }
}

/// @brief 节点负载。 / Node payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeBody {
    Fragment(XmlText),
    Prompt(NonEmptyChildren),
}
impl NodeBody {
    /// @brief 返回负载种类。 / Return the payload kind.
    /// @return 节点种类。 / Node kind.
    pub const fn kind(&self) -> NodeKind {
        match self {
            Self::Fragment(_) => NodeKind::Fragment,
            Self::Prompt(_) => NodeKind::Prompt,
        }
    }
}

/// @brief 节点身份、运维版本与元数据。 / Node identity, operational version, and metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeHeader {
    pub id: NodeId,
    pub symbol: Symbol,
    pub kind: NodeKind,
    pub revision: Revision,
    pub metadata: Metadata,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// @brief 完整领域节点。 / Complete domain node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "NodeDto", into = "NodeDto")]
pub struct Node {
    pub header: NodeHeader,
    pub body: NodeBody,
}

/// @brief 仅用于经过校验的节点序列化。 / Serialization DTO used only for validated node conversion.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeDto {
    /// @brief 持久化节点头。 / Persisted node header.
    header: NodeHeader,
    /// @brief 持久化节点体。 / Persisted node body.
    body: NodeBody,
}

impl TryFrom<NodeDto> for Node {
    type Error = DomainError;

    fn try_from(value: NodeDto) -> Result<Self, Self::Error> {
        Self::from_parts(value.header, value.body)
    }
}

impl From<Node> for NodeDto {
    fn from(value: Node) -> Self {
        Self {
            header: value.header,
            body: value.body,
        }
    }
}

impl Node {
    /// @brief 从持久化部件恢复节点并校验类型一致性。 / Rehydrate a node and validate kind consistency.
    /// @param header 节点头。 / Node header.
    /// @param body 节点负载。 / Node payload.
    /// @return 完整节点或领域错误。 / Complete node or domain error.
    pub fn from_parts(header: NodeHeader, body: NodeBody) -> Result<Self, DomainError> {
        let actual = body.kind();
        if header.kind != actual {
            return Err(DomainError::KindMismatch {
                declared: header.kind,
                actual,
            });
        }
        Ok(Self { header, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn symbol_rule_is_exact() {
        for valid in ["A", "_", "tool_call", "Agent-Legible", "cpp23"] {
            assert!(Symbol::new(valid).is_ok());
        }
        for invalid in ["", "1A", "foo.bar", "foo:bar", "hello world", "é"] {
            assert!(Symbol::new(invalid).is_err());
        }
    }

    #[test]
    fn xml_character_boundaries() {
        assert!(XmlText::new("\t\n\r <&> \u{D7FF}\u{E000}\u{FFFD}\u{10000}").is_ok());
        for invalid in ['\0', '\u{8}', '\u{B}', '\u{1F}'] {
            assert!(XmlText::new(invalid.to_string()).is_err());
        }
    }

    #[test]
    fn serde_rejects_non_positive_id_and_revision() {
        assert!(serde_json::from_str::<NodeId>("-1").is_err());
        assert!(serde_json::from_str::<NodeId>("0").is_err());
        assert_eq!(serde_json::from_str::<NodeId>("1").unwrap().get(), 1);
        assert!(serde_json::from_str::<Revision>("0").is_err());
        assert_eq!(serde_json::from_str::<Revision>("1").unwrap().get(), 1);
    }

    #[test]
    fn serde_revalidates_node_kind_against_body() {
        let json = r#"{
            "header": {
                "id": 1,
                "symbol": "Mismatch",
                "kind": "Prompt",
                "revision": 1,
                "metadata": {"description": null, "tags": []},
                "created_at_ms": 0,
                "updated_at_ms": 0
            },
            "body": {"Fragment": "text"}
        }"#;
        assert!(serde_json::from_str::<Node>(json).is_err());
    }

    proptest! {
        #[test]
        fn generated_valid_symbols_are_accepted(seed in proptest::collection::vec(any::<u8>(), 1..64)) {
            const FIRST: &[u8] = b"_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
            const REST: &[u8] = b"_-ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
            let mut value = String::with_capacity(seed.len());
            value.push(FIRST[usize::from(seed[0]) % FIRST.len()] as char);
            value.extend(seed[1..].iter().map(|byte| REST[usize::from(*byte) % REST.len()] as char));
            let symbol = Symbol::new(value.clone()).unwrap();
            prop_assert_eq!(symbol.as_str(), value);
        }
    }
}
