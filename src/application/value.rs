//! 解释器返回的宿主无关值。 / Host-independent values returned by the interpreter.

use crate::domain::{Metadata, NodeId, NodeKind, Revision, SearchHit, Symbol};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt,
    io::{self, BufWriter, Read, Write},
    sync::Arc,
};
use tempfile::{Builder, NamedTempFile};

/// XML 在内存中保留的最大字节数。 / Maximum XML bytes retained in memory.
///
/// <!-- @brief XML 在内存中保留的最大字节数。 / Maximum XML bytes retained in memory. -->
pub const XML_MEMORY_LIMIT: usize = 16 * 1024 * 1024;

/// 落盘 XML 的固定写缓冲区字节数。 / Fixed write-buffer size for spilled XML.
///
/// <!-- @brief 落盘 XML 的固定写缓冲区字节数。 / Fixed write-buffer size for spilled XML. -->
const XML_SPILL_BUFFER_SIZE: usize = 64 * 1024;

/// 规范 XML 的共享存储。 / Shared storage for canonical XML.
///
/// <!-- @brief 规范 XML 的共享存储。 / Shared storage for canonical XML. -->
enum XmlStorage {
    Memory(Vec<u8>),
    Spilled(NamedTempFile),
}

/// 可克隆、自清理的规范 XML 值对象。 / Cloneable, self-cleaning canonical XML value object.
///
/// <!-- @brief 可克隆、自清理的规范 XML 值对象。 / Cloneable, self-cleaning canonical XML value object. -->
///
/// # Notes
/// 大于 16 MiB 的值使用拥有权临时文件，在最后一个克隆销毁时删除。 / Values larger than 16 MiB use an owned temporary file, removed with the last clone.
///
/// <!-- @note 大于 16 MiB 的值使用拥有权临时文件，在最后一个克隆销毁时删除。 / Values larger than 16 MiB use an owned temporary file, removed with the last clone. -->
#[derive(Clone)]
pub struct CanonicalXml {
    storage: Arc<XmlStorage>,
    len: usize,
}

impl CanonicalXml {
    /// 返回 XML UTF-8 字节数。 / Return the XML UTF-8 byte length.
    ///
    /// <!-- @brief 返回 XML UTF-8 字节数。 / Return the XML UTF-8 byte length. -->
    ///
    /// # Returns
    /// 字节数。 / Byte length.
    ///
    /// <!-- @return 字节数。 / Byte length. -->
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 判断 XML 是否为空。 / Return whether the XML is empty.
    ///
    /// <!-- @brief 判断 XML 是否为空。 / Return whether the XML is empty. -->
    ///
    /// # Returns
    /// 空值返回 true。 / True for an empty value.
    ///
    /// <!-- @return 空值返回 true。 / True for an empty value. -->
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 把 XML 流式写入输出汇。 / Stream XML into an output sink.
    ///
    /// <!-- @brief 把 XML 流式写入输出汇。 / Stream XML into an output sink. -->
    ///
    /// # Arguments
    /// - `writer`: 目标字节流。 / Destination byte stream.
    /// <!-- @param writer 目标字节流。 / Destination byte stream. -->
    ///
    /// # Returns
    /// 写入成功或 I/O 错误。 / Success or an I/O error.
    ///
    /// <!-- @return 写入成功或 I/O 错误。 / Success or an I/O error. -->
    ///
    /// # Errors
    /// 当落盘值无法重新打开、读取，或目标输出汇拒绝写入时返回 I/O 错误。 /
    /// Returns an I/O error when a spilled value cannot be reopened or read, or when the destination
    /// sink rejects a write.
    pub fn write_to(&self, writer: &mut dyn Write) -> io::Result<()> {
        match self.storage.as_ref() {
            XmlStorage::Memory(bytes) => writer.write_all(bytes),
            XmlStorage::Spilled(file) => {
                io::copy(&mut file.reopen()?, writer)?;
                Ok(())
            }
        }
    }

    /// 为 JSON/API 边界读取完整 UTF-8 字符串。 / Read the complete UTF-8 string for JSON/API boundaries.
    ///
    /// <!-- @brief 为 JSON/API 边界读取完整 UTF-8 字符串。 / Read the complete UTF-8 string for JSON/API boundaries. -->
    ///
    /// # Returns
    /// 完整文本或 I/O/UTF-8 错误。 / Complete text or an I/O/UTF-8 error.
    ///
    /// <!-- @return 完整文本或 I/O/UTF-8 错误。 / Complete text or an I/O/UTF-8 error. -->
    ///
    /// # Errors
    /// 当落盘值无法重新打开或读取，或存储字节不是有效 UTF-8 时返回 I/O 错误。 /
    /// Returns an I/O error when a spilled value cannot be reopened or read, or when its bytes are
    /// not valid UTF-8.
    ///
    /// # Notes
    /// 该 API 按完整值分配内存；常规展示应使用 `write_to`。 / This allocates for the full value; normal presentation should use `write_to`.
    ///
    /// <!-- @note 该 API 按完整值分配内存；常规展示应使用 `write_to`。 / This allocates for the full value; normal presentation should use `write_to`. -->
    pub fn read_to_string(&self) -> io::Result<String> {
        match self.storage.as_ref() {
            XmlStorage::Memory(bytes) => String::from_utf8(bytes.clone())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            XmlStorage::Spilled(file) => {
                let mut s = String::with_capacity(self.len);
                file.reopen()?.read_to_string(&mut s)?;
                Ok(s)
            }
        }
    }

    /// 以有界内存构造显示预览。 / Build a display preview with bounded memory.
    ///
    /// <!-- @brief 以有界内存构造显示预览。 / Build a display preview with bounded memory. -->
    ///
    /// # Arguments
    /// - `budget`: 最大 UTF-8 字节数。 / Maximum UTF-8 bytes.
    /// <!-- @param budget 最大 UTF-8 字节数。 / Maximum UTF-8 bytes. -->
    ///
    /// # Returns
    /// 不拆分字符的前缀。 / Prefix that does not split a character.
    ///
    /// <!-- @return 不拆分字符的前缀。 / Prefix that does not split a character. -->
    ///
    /// # Errors
    /// 当落盘值无法重新打开或读取，或内部字节违反规范 UTF-8 不变量时返回 I/O 错误。 /
    /// Returns an I/O error when a spilled value cannot be reopened or read, or when internal bytes
    /// violate the canonical UTF-8 invariant.
    pub fn preview(&self, budget: usize) -> io::Result<String> {
        let prefix_len = budget.min(self.len);
        let mut bytes = match self.storage.as_ref() {
            XmlStorage::Memory(bytes) => bytes[..prefix_len].to_vec(),
            XmlStorage::Spilled(file) => {
                // 最多多读一个 UTF-8 标量值的尾部；绝不遍历整个溢出文件。
                // Read at most one UTF-8 scalar tail; never scan the whole spilled file.
                let read_limit = budget.saturating_add(3).min(self.len);
                let mut bytes = Vec::with_capacity(read_limit);
                file.reopen()?
                    .take(read_limit as u64)
                    .read_to_end(&mut bytes)?;
                bytes.truncate(prefix_len);
                bytes
            }
        };
        while std::str::from_utf8(&bytes).is_err() {
            bytes.pop();
        }
        String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// 返回 XML 是否包含文本模式。 / Return whether XML contains a text pattern.
    ///
    /// <!-- @brief 返回 XML 是否包含文本模式。 / Return whether XML contains a text pattern. -->
    ///
    /// # Arguments
    /// - `pattern`: 待查找模式。 / Pattern to find.
    /// <!-- @param pattern 待查找模式。 / Pattern to find. -->
    ///
    /// # Returns
    /// 包含时返回 true。 / True when contained.
    ///
    /// <!-- @return 包含时返回 true。 / True when contained. -->
    pub fn contains(&self, pattern: &str) -> bool {
        self.read_to_string().is_ok_and(|xml| xml.contains(pattern))
    }

    #[cfg(test)]
    pub(crate) fn spilled_path(&self) -> Option<std::path::PathBuf> {
        match self.storage.as_ref() {
            XmlStorage::Spilled(file) => Some(file.path().to_owned()),
            XmlStorage::Memory(_) => None,
        }
    }

    /// 返回该值实际占用的聚合内存预算。 / Return the aggregate memory budget occupied by this value.
    ///
    /// <!-- @brief 返回该值实际占用的聚合内存预算。 / Return the aggregate memory budget occupied by this value. -->
    ///
    /// # Returns
    /// 内存值的字节数；落盘值返回零。 / Byte length for memory values; zero for spilled values.
    ///
    /// <!-- @return 内存值的字节数；落盘值返回零。 / Byte length for memory values; zero for spilled values. -->
    pub(crate) fn resident_len(&self) -> usize {
        match self.storage.as_ref() {
            XmlStorage::Memory(bytes) => bytes.len(),
            XmlStorage::Spilled(_) => 0,
        }
    }
}

impl From<String> for CanonicalXml {
    fn from(value: String) -> Self {
        Self {
            len: value.len(),
            storage: Arc::new(XmlStorage::Memory(value.into_bytes())),
        }
    }
}
impl From<&str> for CanonicalXml {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
impl fmt::Debug for CanonicalXml {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CanonicalXml")
            .field("len", &self.len)
            .field(
                "storage",
                &match self.storage.as_ref() {
                    XmlStorage::Memory(_) => "memory",
                    XmlStorage::Spilled(_) => "temporary_file",
                },
            )
            .finish()
    }
}
impl PartialEq for CanonicalXml {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        match (self.storage.as_ref(), other.storage.as_ref()) {
            (XmlStorage::Memory(a), XmlStorage::Memory(b)) => a == b,
            _ => equal_streams(self, other).unwrap_or(false),
        }
    }
}

/// 以固定内存精确比较两个 XML 流。 / Exactly compare two XML streams with fixed memory.
///
/// <!-- @brief 以固定内存精确比较两个 XML 流。 / Exactly compare two XML streams with fixed memory. -->
///
/// # Arguments
/// - `left`: 左值。 / Left value.
/// <!-- @param left 左值。 / Left value. -->
/// - `right`: 右值。 / Right value.
/// <!-- @param right 右值。 / Right value. -->
///
/// # Returns
/// 是否逐字节相等，或 I/O 错误。 / Byte equality or an I/O error.
///
/// <!-- @return 是否逐字节相等，或 I/O 错误。 / Byte equality or an I/O error. -->
///
/// # Errors
/// 当任一落盘值无法重新打开或读取时返回 I/O 错误。 /
/// Returns an I/O error when either spilled value cannot be reopened or read.
fn equal_streams(left: &CanonicalXml, right: &CanonicalXml) -> io::Result<bool> {
    fn reader(xml: &CanonicalXml) -> io::Result<Box<dyn Read + '_>> {
        Ok(match xml.storage.as_ref() {
            XmlStorage::Memory(bytes) => Box::new(io::Cursor::new(bytes.as_slice())),
            XmlStorage::Spilled(file) => Box::new(file.reopen()?),
        })
    }
    let mut remaining = left.len;
    let (mut left, mut right) = (reader(left)?, reader(right)?);
    let (mut left_buf, mut right_buf) = ([0_u8; 8192], [0_u8; 8192]);
    while remaining != 0 {
        let chunk = remaining.min(left_buf.len());
        left.read_exact(&mut left_buf[..chunk])?;
        right.read_exact(&mut right_buf[..chunk])?;
        if left_buf[..chunk] != right_buf[..chunk] {
            return Ok(false);
        }
        remaining -= chunk;
    }
    Ok(true)
}
impl Serialize for CanonicalXml {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.read_to_string()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for CanonicalXml {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Into::into)
    }
}

/// 在内存与拥有权临时文件间自动切换的 XML 写入器。 / XML writer that automatically switches from memory to an owned temporary file.
///
/// <!-- @brief 在内存与拥有权临时文件间自动切换的 XML 写入器。 / XML writer that automatically switches from memory to an owned temporary file. -->
pub struct SpillWriter {
    state: SpillState,
    len: usize,
    threshold: usize,
}
enum SpillState {
    Memory(Vec<u8>),
    Spilled(BufWriter<NamedTempFile>),
}

impl SpillWriter {
    /// 使用 16 MiB 阈值创建写入器。 / Create a writer with the 16 MiB threshold.
    ///
    /// <!-- @brief 使用 16 MiB 阈值创建写入器。 / Create a writer with the 16 MiB threshold. -->
    ///
    /// # Returns
    /// 空写入器。 / Empty writer.
    ///
    /// <!-- @return 空写入器。 / Empty writer. -->
    pub fn new() -> Self {
        Self::with_threshold(XML_MEMORY_LIMIT)
    }

    /// 使用指定阈值创建写入器。 / Create a writer with a specified threshold.
    ///
    /// <!-- @brief 使用指定阈值创建写入器。 / Create a writer with a specified threshold. -->
    ///
    /// # Arguments
    /// - `threshold`: 内存中允许的最大字节数。 / Maximum bytes allowed in memory.
    /// <!-- @param threshold 内存中允许的最大字节数。 / Maximum bytes allowed in memory. -->
    ///
    /// # Returns
    /// 空写入器。 / Empty writer.
    ///
    /// <!-- @return 空写入器。 / Empty writer. -->
    ///
    /// # Notes
    /// 超过 16 MiB 的阈值会被限制到 16 MiB。 / Thresholds above 16 MiB are clamped to 16 MiB.
    ///
    /// <!-- @note 超过 16 MiB 的阈值会被限制到 16 MiB。 / Thresholds above 16 MiB are clamped to 16 MiB. -->
    pub fn with_threshold(threshold: usize) -> Self {
        Self {
            state: SpillState::Memory(Vec::new()),
            len: 0,
            threshold: threshold.min(XML_MEMORY_LIMIT),
        }
    }

    /// 完成写入并转移为共享值对象。 / Finish writing and transfer into a shared value object.
    ///
    /// <!-- @brief 完成写入并转移为共享值对象。 / Finish writing and transfer into a shared value object. -->
    ///
    /// # Returns
    /// 规范 XML 或刷新错误。 / Canonical XML or a flush error.
    ///
    /// <!-- @return 规范 XML 或刷新错误。 / Canonical XML or a flush error. -->
    ///
    /// # Errors
    /// 当落盘缓冲区无法刷新或解包为拥有权临时文件时返回 I/O 错误。 /
    /// Returns an I/O error when the spill buffer cannot be flushed or converted back into its owned
    /// temporary file.
    pub fn finish(mut self) -> io::Result<CanonicalXml> {
        self.flush()?;
        let storage = match self.state {
            SpillState::Memory(v) => XmlStorage::Memory(v),
            SpillState::Spilled(writer) => XmlStorage::Spilled(
                writer
                    .into_inner()
                    .map_err(std::io::IntoInnerError::into_error)?,
            ),
        };
        Ok(CanonicalXml {
            storage: Arc::new(storage),
            len: self.len,
        })
    }

    fn spill(memory: &[u8]) -> io::Result<BufWriter<NamedTempFile>> {
        let file = Builder::new().prefix("promptr-xml-").tempfile()?;
        let mut writer = BufWriter::with_capacity(XML_SPILL_BUFFER_SIZE, file);
        writer.write_all(memory)?;
        Ok(writer)
    }
}
impl Default for SpillWriter {
    fn default() -> Self {
        Self::new()
    }
}
impl Write for SpillWriter {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let next_len = self
            .len
            .checked_add(input.len())
            .ok_or_else(|| io::Error::other("XML length overflow"))?;
        if let SpillState::Memory(memory) = &mut self.state {
            if next_len <= self.threshold {
                memory.extend_from_slice(input);
                self.len = next_len;
                return Ok(input.len());
            }
            let mut writer = Self::spill(memory)?;
            writer.write_all(input)?;
            self.state = SpillState::Spilled(writer);
            self.len = next_len;
            return Ok(input.len());
        }
        let SpillState::Spilled(writer) = &mut self.state else {
            unreachable!()
        };
        let written = writer.write(input)?;
        self.len += written;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        match &mut self.state {
            SpillState::Memory(_) => Ok(()),
            SpillState::Spilled(writer) => writer.flush(),
        }
    }
}

/// 面向宿主的节点投影。 / Node projection exposed to hosts.
///
/// <!-- @brief 面向宿主的节点投影。 / Node projection exposed to hosts. -->
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeView {
    /// 稳定内部标识。 / Stable internal identity.
    ///
    /// <!-- @brief 稳定内部标识。 / Stable internal identity. -->
    pub id: NodeId,
    /// 可见符号。 / Visible symbol.
    ///
    /// <!-- @brief 可见符号。 / Visible symbol. -->
    pub symbol: Symbol,
    /// 节点种类。 / Node kind.
    ///
    /// <!-- @brief 节点种类。 / Node kind. -->
    pub kind: NodeKind,
    /// 修订号。 / Revision.
    ///
    /// <!-- @brief 修订号。 / Revision. -->
    pub revision: Revision,
    /// 有序直接子符号。 / Ordered direct child symbols.
    ///
    /// <!-- @brief 有序直接子符号。 / Ordered direct child symbols. -->
    pub children: Vec<Symbol>,
    /// 片段字节数。 / Fragment byte length.
    ///
    /// <!-- @brief 片段字节数。 / Fragment byte length. -->
    pub byte_size: Option<usize>,
    /// 用户元数据。 / User metadata.
    ///
    /// <!-- @brief 用户元数据。 / User metadata. -->
    pub metadata: Metadata,
}

/// 搜索命中的稳定接口投影。 / Stable interface projection of a search hit.
///
/// <!-- @brief 搜索命中的稳定接口投影。 / Stable interface projection of a search hit. -->
pub type SearchHitView = SearchHit;

/// 解释器产生的类型化值。 / Typed value produced by the interpreter.
///
/// <!-- @brief 解释器产生的类型化值。 / Typed value produced by the interpreter. -->
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Value {
    /// 无负载的成功。 / Successful operation without a payload.
    ///
    /// <!-- @brief 无负载的成功。 / Successful operation without a payload. -->
    Unit,
    /// 人类向文本。 / Human-oriented text.
    ///
    /// <!-- @brief 人类向文本。 / Human-oriented text. -->
    Text(String),
    /// 规范 XML 字节的 UTF-8 表示。 / UTF-8 representation of canonical XML bytes.
    ///
    /// <!-- @brief 规范 XML 字节的 UTF-8 表示。 / UTF-8 representation of canonical XML bytes. -->
    Xml(CanonicalXml),
    /// 单节点投影。 / One node projection.
    ///
    /// <!-- @brief 单节点投影。 / One node projection. -->
    Node(NodeView),
    /// 节点列表。 / Node list.
    ///
    /// <!-- @brief 节点列表。 / Node list. -->
    Nodes(Vec<NodeView>),
    /// 搜索结果。 / Search results.
    ///
    /// <!-- @brief 搜索结果。 / Search results. -->
    SearchResults(Vec<SearchHitView>),
    /// 用户元数据。 / User metadata.
    ///
    /// <!-- @brief 用户元数据。 / User metadata. -->
    Metadata(Metadata),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spills_exact_bytes_and_clone_owns_file_lifetime() {
        let bytes = vec![b'x'; XML_MEMORY_LIMIT + 1];
        let mut writer = SpillWriter::new();
        writer.write_all(&bytes).unwrap();
        let xml = writer.finish().unwrap();
        let path = xml.spilled_path().unwrap();
        assert!(path.exists());
        let clone = xml.clone();
        drop(xml);
        assert!(path.exists());
        let mut actual = Vec::new();
        clone.write_to(&mut actual).unwrap();
        assert_eq!(actual, bytes);
        drop(clone);
        assert!(!path.exists());
    }

    #[test]
    fn spilled_writer_preserves_many_small_writes() {
        let mut writer = SpillWriter::with_threshold(31);
        let mut expected = Vec::new();
        for index in 0..200_000_u32 {
            let byte = b'a' + (index % 26) as u8;
            writer.write_all(&[byte]).unwrap();
            expected.push(byte);
        }
        let xml = writer.finish().unwrap();
        assert!(xml.spilled_path().is_some());
        let mut actual = Vec::new();
        xml.write_to(&mut actual).unwrap();
        assert_eq!(actual, expected);
    }
    #[test]
    fn json_keeps_xml_as_a_string() {
        assert_eq!(
            serde_json::to_value(Value::Xml("<Leaf/>\n".into())).unwrap(),
            serde_json::json!({"type":"xml", "value":"<Leaf/>\n"})
        );
    }

    #[test]
    fn spilled_preview_reads_a_bounded_utf8_prefix() {
        let prefix = "x".repeat(XML_MEMORY_LIMIT);
        let content = format!("{prefix}猫tail-that-must-not-be-scanned");
        let mut writer = SpillWriter::new();
        writer.write_all(content.as_bytes()).unwrap();
        let xml = writer.finish().unwrap();
        assert!(xml.spilled_path().is_some());

        assert_eq!(xml.preview(7).unwrap(), "xxxxxxx");
        assert_eq!(xml.preview(XML_MEMORY_LIMIT + 1).unwrap(), prefix);
        assert_eq!(xml.preview(0).unwrap(), "");
    }
}
