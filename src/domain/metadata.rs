use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::DomainError;

/// 保留原始拼写的标签。 / Tag preserving its original spelling.
///
/// <!-- @brief 保留原始拼写的标签。 / Tag preserving its original spelling. -->
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Tag(String);

impl Tag {
    /// 构造非空标签。 / Construct a non-empty tag.
    ///
    /// # Arguments
    ///
    /// - `value` — 标签文本。 / Tag text.
    ///
    /// # Returns
    /// 有效标签或领域错误。 / Valid tag or domain error.
    ///
    /// # Errors
    ///
    /// 当 `value` 为空或仅含空白时返回 [`DomainError::EmptyTag`]。 /
    /// Returns [`DomainError::EmptyTag`] when `value` is empty or whitespace-only.
    ///
    /// <!-- @brief 构造非空标签。 / Construct a non-empty tag. -->
    /// <!-- @param value 标签文本。 / Tag text. -->
    /// <!-- @return 有效标签或领域错误。 / Valid tag or domain error. -->
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DomainError::EmptyTag);
        }
        Ok(Self(value))
    }

    /// 借用标签文本。 / Borrow the tag text.
    ///
    /// # Returns
    /// 标签文本。 / Tag text.
    ///
    /// <!-- @brief 借用标签文本。 / Borrow the tag text. -->
    /// <!-- @return 标签文本。 / Tag text. -->
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 取出标签文本。 / Consume and return tag text.
    ///
    /// # Returns
    /// 标签文本。 / Tag text.
    ///
    /// <!-- @brief 取出标签文本。 / Consume and return tag text. -->
    /// <!-- @return 标签文本。 / Tag text. -->
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl TryFrom<String> for Tag {
    type Error = DomainError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Tag> for String {
    fn from(value: Tag) -> Self {
        value.0
    }
}

/// 用户拥有的节点元数据。 / User-owned node metadata.
///
/// <!-- @brief 用户拥有的节点元数据。 / User-owned node metadata. -->
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    /// 可选描述；空字符串等价于清除。 / Optional description; empty means cleared.
    ///
    /// <!-- @brief 可选描述；空字符串等价于清除。 / Optional description; empty means cleared. -->
    description: Option<String>,
    /// 大小写敏感且确定排序的标签集合。 / Case-sensitive, deterministically ordered tag set.
    ///
    /// <!-- @brief 大小写敏感且确定排序的标签集合。 / Case-sensitive, deterministically ordered tag set. -->
    tags: BTreeSet<Tag>,
}

impl Metadata {
    /// 构造元数据并规范化空描述。 / Construct metadata and normalize an empty description.
    ///
    /// # Arguments
    ///
    /// - `description` — 可选描述。 / Optional description.
    /// - `tags` — 标签集合。 / Tag set.
    ///
    /// # Returns
    /// 规范化后的元数据。 / Normalized metadata.
    ///
    /// <!-- @brief 构造元数据并规范化空描述。 / Construct metadata and normalize an empty description. -->
    /// <!-- @param description 可选描述。 / Optional description. -->
    /// <!-- @param tags 标签集合。 / Tag set. -->
    /// <!-- @return 规范化后的元数据。 / Normalized metadata. -->
    pub fn new(description: Option<String>, tags: impl IntoIterator<Item = Tag>) -> Self {
        Self {
            description: description.filter(|value| !value.is_empty()),
            tags: tags.into_iter().collect(),
        }
    }

    /// 借用描述。 / Borrow the description.
    ///
    /// # Returns
    /// 可选描述。 / Optional description.
    ///
    /// <!-- @brief 借用描述。 / Borrow the description. -->
    /// <!-- @return 可选描述。 / Optional description. -->
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// 按字节序迭代标签。 / Iterate tags in bytewise order.
    ///
    /// # Returns
    /// 标签迭代器。 / Tag iterator.
    ///
    /// <!-- @brief 按字节序迭代标签。 / Iterate tags in bytewise order. -->
    /// <!-- @return 标签迭代器。 / Tag iterator. -->
    pub fn tags(&self) -> impl ExactSizeIterator<Item = &Tag> {
        self.tags.iter()
    }

    /// 完整替换描述。 / Replace the complete description.
    ///
    /// # Arguments
    ///
    /// - `description` — 新描述，空字符串会清除。 / New description; empty clears it.
    ///
    /// <!-- @brief 完整替换描述。 / Replace the complete description. -->
    /// <!-- @param description 新描述，空字符串会清除。 / New description; empty clears it. -->
    pub fn set_description(&mut self, description: Option<String>) {
        self.description = description.filter(|value| !value.is_empty());
    }

    /// 完整替换标签。 / Replace the complete tag set.
    ///
    /// # Arguments
    ///
    /// - `tags` — 新标签。 / New tags.
    ///
    /// <!-- @brief 完整替换标签。 / Replace the complete tag set. -->
    /// <!-- @param tags 新标签。 / New tags. -->
    pub fn set_tags(&mut self, tags: impl IntoIterator<Item = Tag>) {
        self.tags = tags.into_iter().collect();
    }
}
