use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::DomainError;

/// @brief 保留原始拼写的标签。 / Tag preserving its original spelling.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Tag(String);

impl Tag {
    /// @brief 构造非空标签。 / Construct a non-empty tag.
    /// @param value 标签文本。 / Tag text.
    /// @return 有效标签或领域错误。 / Valid tag or domain error.
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DomainError::EmptyTag);
        }
        Ok(Self(value))
    }

    /// @brief 借用标签文本。 / Borrow the tag text.
    /// @return 标签文本。 / Tag text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// @brief 取出标签文本。 / Consume and return tag text.
    /// @return 标签文本。 / Tag text.
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

/// @brief 用户拥有的节点元数据。 / User-owned node metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    /// @brief 可选描述；空字符串等价于清除。 / Optional description; empty means cleared.
    description: Option<String>,
    /// @brief 大小写敏感且确定排序的标签集合。 / Case-sensitive, deterministically ordered tag set.
    tags: BTreeSet<Tag>,
}

impl Metadata {
    /// @brief 构造元数据并规范化空描述。 / Construct metadata and normalize an empty description.
    /// @param description 可选描述。 / Optional description.
    /// @param tags 标签集合。 / Tag set.
    /// @return 规范化后的元数据。 / Normalized metadata.
    pub fn new(description: Option<String>, tags: impl IntoIterator<Item = Tag>) -> Self {
        Self {
            description: description.filter(|value| !value.is_empty()),
            tags: tags.into_iter().collect(),
        }
    }

    /// @brief 借用描述。 / Borrow the description.
    /// @return 可选描述。 / Optional description.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// @brief 按字节序迭代标签。 / Iterate tags in bytewise order.
    /// @return 标签迭代器。 / Tag iterator.
    pub fn tags(&self) -> impl ExactSizeIterator<Item = &Tag> {
        self.tags.iter()
    }

    /// @brief 完整替换描述。 / Replace the complete description.
    /// @param description 新描述，空字符串会清除。 / New description; empty clears it.
    pub fn set_description(&mut self, description: Option<String>) {
        self.description = description.filter(|value| !value.is_empty());
    }

    /// @brief 完整替换标签。 / Replace the complete tag set.
    /// @param tags 新标签。 / New tags.
    pub fn set_tags(&mut self, tags: impl IntoIterator<Item = Tag>) {
        self.tags = tags.into_iter().collect();
    }
}
