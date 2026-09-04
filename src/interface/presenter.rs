//! 人类与机器输出适配器。 / Human and machine output adapters.

use std::io::{self, Write};

use clap::ValueEnum;
use serde::Serialize;

use crate::{Diagnostic, Value};

/// 命令行输出格式。 / Command-line output format.
///
/// <!-- @brief 命令行输出格式。 / Command-line output format. -->
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    /// 面向人的稳定文本。 / Stable human-oriented text.
    ///
    /// <!-- @brief 面向人的稳定文本。 / Stable human-oriented text. -->
    #[default]
    Human,
    /// 单个版本化 JSON 文档。 / One versioned JSON document.
    ///
    /// <!-- @brief 单个版本化 JSON 文档。 / One versioned JSON document. -->
    Json,
    /// 仅规范 XML 字节。 / Canonical XML bytes only.
    ///
    /// <!-- @brief 仅规范 XML 字节。 / Canonical XML bytes only. -->
    Raw,
}

/// JSON 机器接口的顶层文档。 / Top-level JSON machine-interface document.
///
/// <!-- @brief JSON 机器接口的顶层文档。 / Top-level JSON machine-interface document. -->
#[derive(Debug, Serialize)]
struct JsonDocument<'a, T: Serialize + ?Sized> {
    /// 接口模式版本。 / Interface schema version.
    ///
    /// <!-- @brief 接口模式版本。 / Interface schema version. -->
    schema_version: u32,
    /// 操作是否成功。 / Whether the operation succeeded.
    ///
    /// <!-- @brief 操作是否成功。 / Whether the operation succeeded. -->
    ok: bool,
    /// 成功负载。 / Success payload.
    ///
    /// <!-- @brief 成功负载。 / Success payload. -->
    #[serde(skip_serializing_if = "Option::is_none")]
    values: Option<&'a T>,
    /// 失败诊断。 / Failure diagnostics.
    ///
    /// <!-- @brief 失败诊断。 / Failure diagnostics. -->
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a [Diagnostic]>,
}

/// 将类型化运行时值写入指定输出流。 / Write typed runtime values to a selected output stream.
///
/// # Arguments
///
/// - `writer`: 标准输出等目标。 / Destination such as standard output.
/// - `format`: 输出格式。 / Output format.
/// - `values`: 已提交的类型化值。 / Committed typed values.
///
/// # Returns
///
/// 写入成功或 I/O 错误。 / Success or an I/O error.
/// 将成功负载编码为版本化 JSON 文档。 / Encode a success payload as a versioned JSON document.
///
/// # Arguments
///
/// - `writer`: 输出目标。 / Output destination.
/// - `value`: 成功负载。 / Success payload.
///
/// # Returns
///
/// 写入成功时返回空值。 / Returns the unit value after successful writing.
///
/// # Errors
///
/// 当目标流写入失败，或原始格式（raw format）收到非 XML 值时返回错误。 / Returns an
/// error when the destination cannot be written or raw format receives a non-XML value.
///
/// <!-- @brief 将类型化运行时值写入指定输出流。 / Write typed runtime values to a selected output stream. -->
/// <!-- @param writer 标准输出等目标。 / Destination such as standard output. -->
/// <!-- @param format 输出格式。 / Output format. -->
/// <!-- @param values 已提交的类型化值。 / Committed typed values. -->
/// <!-- @return 写入成功或 I/O 错误。 / Success or an I/O error. -->
pub fn write_values(
    writer: &mut dyn Write,
    format: OutputFormat,
    values: &[Value],
) -> io::Result<()> {
    match format {
        OutputFormat::Human => write_human(writer, values),
        OutputFormat::Json => write_json_values(writer, values),
        OutputFormat::Raw => write_raw(writer, values),
    }
}

/// 流式写入值数组的成功 JSON 文档。 / Stream a successful JSON document containing values.
///
/// # Arguments
///
/// - `writer`: 输出目标。 / Output destination.
/// - `values`: 类型化值。 / Typed values.
///
/// # Returns
///
/// 写入成功或 I/O 错误。 / Success or an I/O error.
///
/// # Errors
///
/// 当目标流写入失败，或非 XML 值无法序列化为 JSON 时返回错误。 / Returns an error when
/// the destination cannot be written or a non-XML value cannot be serialized as JSON.
///
/// <!-- @brief 流式写入值数组的成功 JSON 文档。 / Stream a successful JSON document containing values. -->
/// <!-- @param writer 输出目标。 / Output destination. -->
/// <!-- @param values 类型化值。 / Typed values. -->
/// <!-- @return 写入成功或 I/O 错误。 / Success or an I/O error. -->
fn write_json_values(writer: &mut dyn Write, values: &[Value]) -> io::Result<()> {
    writer.write_all(br#"{"schema_version":1,"ok":true,"values":["#)?;
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            writer.write_all(b",")?;
        }
        match value {
            Value::Xml(xml) => {
                writer.write_all(br#"{"type":"xml","value":""#)?;
                xml.write_to(&mut JsonStringWriter::new(&mut *writer))?;
                writer.write_all(br#""}"#)?;
            }
            other => serde_json::to_writer(&mut *writer, other)?,
        }
    }
    writer.write_all(b"]}\n")
}

/// 把 UTF-8 字节流转义为 JSON 字符串内容。 / Escape a UTF-8 byte stream as JSON string content.
///
/// # Notes
///
/// 非 ASCII 字节保持原样；JSON 特殊字符与控制字节立即转义，不缓存完整值。 / Non-ASCII bytes are preserved; JSON specials and control bytes are escaped immediately without buffering the complete value.
///
/// <!-- @brief 把 UTF-8 字节流转义为 JSON 字符串内容。 / Escape a UTF-8 byte stream as JSON string content. -->
/// <!-- @note 非 ASCII 字节保持原样；JSON 特殊字符与控制字节立即转义，不缓存完整值。 / Non-ASCII bytes are preserved; JSON specials and control bytes are escaped immediately without buffering the complete value. -->
struct JsonStringWriter<'a> {
    /// 下游输出流。 / Downstream output stream.
    ///
    /// <!-- @brief 下游输出流。 / Downstream output stream. -->
    inner: &'a mut dyn Write,
}

impl<'a> JsonStringWriter<'a> {
    /// 创建 JSON 字符串转义写入器。 / Create a JSON-string escaping writer.
    ///
    /// # Arguments
    ///
    /// - `inner`: 下游输出流。 / Downstream output stream.
    ///
    /// # Returns
    ///
    /// 新写入器。 / New writer.
    ///
    /// <!-- @brief 创建 JSON 字符串转义写入器。 / Create a JSON-string escaping writer. -->
    /// <!-- @param inner 下游输出流。 / Downstream output stream. -->
    /// <!-- @return 新写入器。 / New writer. -->
    const fn new(inner: &'a mut dyn Write) -> Self {
        Self { inner }
    }
}

impl Write for JsonStringWriter<'_> {
    /// 转义并转发一个字节切片。 / Escape and forward one byte slice.
    ///
    /// # Arguments
    ///
    /// - `input`: 作为 JSON 字符串内容写入的 UTF-8 字节。 / UTF-8 bytes written as JSON
    ///   string content.
    ///
    /// # Returns
    ///
    /// 成功时返回已消费的输入字节数。 / Returns the consumed input byte count on success.
    ///
    /// # Errors
    ///
    /// 当下游输出流写入失败时返回错误。 / Returns an error when the downstream stream cannot
    /// be written.
    ///
    /// <!-- @brief 转义并转发一个字节切片。 / Escape and forward one byte slice. -->
    /// <!-- @param input 作为 JSON 字符串内容写入的 UTF-8 字节。 / UTF-8 bytes written as JSON string content. -->
    /// <!-- @return 成功时返回已消费的输入字节数。 / Returns the consumed input byte count on success. -->
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let mut start = 0;
        for (index, byte) in input.iter().copied().enumerate() {
            let escape: Option<&[u8]> = match byte {
                b'"' => Some(br#"\""#),
                b'\\' => Some(br#"\\"#),
                b'\x08' => Some(br#"\b"#),
                b'\t' => Some(br#"\t"#),
                b'\n' => Some(br#"\n"#),
                b'\x0c' => Some(br#"\f"#),
                b'\r' => Some(br#"\r"#),
                0x00..=0x1f => {
                    self.inner.write_all(&input[start..index])?;
                    let hex = b"0123456789abcdef";
                    self.inner.write_all(&[
                        b'\\',
                        b'u',
                        b'0',
                        b'0',
                        hex[usize::from(byte >> 4)],
                        hex[usize::from(byte & 0x0f)],
                    ])?;
                    start = index + 1;
                    None
                }
                _ => None,
            };
            if let Some(escape) = escape {
                self.inner.write_all(&input[start..index])?;
                self.inner.write_all(escape)?;
                start = index + 1;
            }
        }
        self.inner.write_all(&input[start..])?;
        Ok(input.len())
    }

    /// 刷新下游输出流。 / Flush the downstream output stream.
    ///
    /// # Returns
    ///
    /// 刷新成功时返回空值。 / Returns the unit value after a successful flush.
    ///
    /// # Errors
    ///
    /// 当下游输出流刷新失败时返回错误。 / Returns an error when the downstream stream cannot
    /// be flushed.
    ///
    /// <!-- @brief 刷新下游输出流。 / Flush the downstream output stream. -->
    /// <!-- @return 刷新成功时返回空值。 / Returns the unit value after a successful flush. -->
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// 写入任意可序列化的管理命令结果。 / Write any serializable management-command result.
///
/// # Arguments
///
/// - `writer`: 输出目标。 / Output destination.
/// - `format`: 输出格式。 / Output format.
/// - `value`: 成功负载。 / Success payload.
///
/// # Returns
///
/// 写入成功或 I/O 错误。 / Success or an I/O error.
///
/// # Errors
///
/// 当目标流写入或 JSON 序列化失败，或请求将管理结果编码为原始格式时返回错误。 /
/// Returns an error when writing or JSON serialization fails, or when raw format is requested for
/// a management result.
///
/// <!-- @brief 写入任意可序列化的管理命令结果。 / Write any serializable management-command result. -->
/// <!-- @param writer 输出目标。 / Output destination. -->
/// <!-- @param format 输出格式。 / Output format. -->
/// <!-- @param value 成功负载。 / Success payload. -->
/// <!-- @return 写入成功或 I/O 错误。 / Success or an I/O error. -->
pub fn write_data<T: Serialize + std::fmt::Debug>(
    writer: &mut dyn Write,
    format: OutputFormat,
    value: &T,
) -> io::Result<()> {
    match format {
        OutputFormat::Json => write_json_success(writer, value),
        OutputFormat::Human => writeln!(writer, "{value:#?}"),
        OutputFormat::Raw => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "raw format is only valid for DSL OUTPUT XML",
        )),
    }
}

/// 发布一个结构化失败。 / Publish one structured failure.
///
/// # Arguments
///
/// - `stdout`: 标准输出。 / Standard output.
/// - `stderr`: 标准错误。 / Standard error.
/// - `format`: 输出格式。 / Output format.
/// - `diagnostic`: 诊断。 / Diagnostic.
///
/// # Returns
///
/// 写入成功或 I/O 错误。 / Success or an I/O error.
///
/// # Errors
///
/// 当所选输出流写入失败或 JSON 诊断无法序列化时返回错误。 / Returns an error when the
/// selected output stream cannot be written or the JSON diagnostic cannot be serialized.
///
/// <!-- @brief 发布一个结构化失败。 / Publish one structured failure. -->
/// <!-- @param stdout 标准输出。 / Standard output. -->
/// <!-- @param stderr 标准错误。 / Standard error. -->
/// <!-- @param format 输出格式。 / Output format. -->
/// <!-- @param diagnostic 诊断。 / Diagnostic. -->
/// <!-- @return 写入成功或 I/O 错误。 / Success or an I/O error. -->
pub fn write_diagnostic(
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
    diagnostic: &Diagnostic,
) -> io::Result<()> {
    if format == OutputFormat::Json {
        let diagnostics = [diagnostic.clone()];
        serde_json::to_writer(
            &mut *stdout,
            &JsonDocument::<Vec<Value>> {
                schema_version: 1,
                ok: false,
                values: None,
                diagnostics: Some(&diagnostics),
            },
        )?;
        writeln!(stdout)
    } else {
        writeln!(stderr, "{}: {}", diagnostic.code, diagnostic.message)?;
        for hint in &diagnostic.hints {
            writeln!(stderr, "hint: {hint}")?;
        }
        Ok(())
    }
}

///
/// # Errors
///
/// 当 JSON 序列化或目标流写入失败时返回错误。 / Returns an error when JSON serialization or
/// destination writing fails.
///
/// <!-- @brief 将成功负载编码为版本化 JSON 文档。 / Encode a success payload as a versioned JSON document. -->
/// <!-- @param writer 输出目标。 / Output destination. -->
/// <!-- @param value 成功负载。 / Success payload. -->
/// <!-- @return 写入成功时返回空值。 / Returns the unit value after successful writing. -->
fn write_json_success<T: Serialize + ?Sized>(writer: &mut dyn Write, value: &T) -> io::Result<()> {
    serde_json::to_writer(
        &mut *writer,
        &JsonDocument {
            schema_version: 1,
            ok: true,
            values: Some(value),
            diagnostics: None,
        },
    )?;
    writeln!(writer)
}

/// 写入仅包含 XML 的原始结果流。 / Write a raw result stream containing only XML.
///
/// # Arguments
///
/// - `writer`: 输出目标。 / Output destination.
/// - `values`: 待写入的类型化值。 / Typed values to write.
///
/// # Returns
///
/// 写入成功时返回空值。 / Returns the unit value after successful writing.
///
/// # Errors
///
/// 当目标流写入失败，或值序列包含非 XML 值时返回错误。 / Returns an error when the
/// destination cannot be written or the values contain a non-XML value.
///
/// <!-- @brief 写入仅包含 XML 的原始结果流。 / Write a raw result stream containing only XML. -->
/// <!-- @param writer 输出目标。 / Output destination. -->
/// <!-- @param values 待写入的类型化值。 / Typed values to write. -->
/// <!-- @return 写入成功时返回空值。 / Returns the unit value after successful writing. -->
fn write_raw(writer: &mut dyn Write, values: &[Value]) -> io::Result<()> {
    if values.iter().any(|value| !matches!(value, Value::Xml(_))) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "raw format requires every result value to be XML",
        ));
    }
    for value in values {
        let Value::Xml(xml) = value else {
            unreachable!("validated above")
        };
        xml.write_to(writer)?;
    }
    Ok(())
}

/// 将类型化值写为面向人的文本。 / Write typed values as human-oriented text.
///
/// # Arguments
///
/// - `writer`: 输出目标。 / Output destination.
/// - `values`: 待呈现的类型化值。 / Typed values to present.
///
/// # Returns
///
/// 写入成功时返回空值。 / Returns the unit value after successful writing.
///
/// # Errors
///
/// 当目标流写入失败或暂存的 XML 无法读取时返回错误。 / Returns an error when the destination
/// cannot be written or spooled XML cannot be read.
///
/// <!-- @brief 将类型化值写为面向人的文本。 / Write typed values as human-oriented text. -->
/// <!-- @param writer 输出目标。 / Output destination. -->
/// <!-- @param values 待呈现的类型化值。 / Typed values to present. -->
/// <!-- @return 写入成功时返回空值。 / Returns the unit value after successful writing. -->
fn write_human(writer: &mut dyn Write, values: &[Value]) -> io::Result<()> {
    for value in values {
        match value {
            Value::Unit => {}
            Value::Text(text) => writeln!(writer, "{text}")?,
            Value::Xml(xml) => xml.write_to(writer)?,
            other => {
                serde_json::to_writer_pretty(&mut *writer, other)?;
                writeln!(writer)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::SpillWriter;

    /// 与生产 XML 内存阈值一致的测试规模。 / Test size matching the production XML memory threshold.
    ///
    /// <!-- @brief 与生产 XML 内存阈值一致的测试规模。 / Test size matching the production XML memory threshold. -->
    const XML_MEMORY_LIMIT: usize = 16 * 1024 * 1024;

    /// 以常量内存统计并散列输出。 / Count and hash output with constant memory.
    ///
    /// <!-- @brief 以常量内存统计并散列输出。 / Count and hash output with constant memory. -->
    struct CountingHashWriter {
        /// 已写字节数。 / Number of bytes written.
        ///
        /// <!-- @brief 已写字节数。 / Number of bytes written. -->
        count: usize,
        /// FNV-1a 流式散列状态。 / Streaming FNV-1a hash state.
        ///
        /// <!-- @brief FNV-1a 流式散列状态。 / Streaming FNV-1a hash state. -->
        hash: u64,
    }

    impl CountingHashWriter {
        /// 创建具有标准偏移基的写入器。 / Create a writer with the standard offset basis.
        ///
        /// # Returns
        ///
        /// 空统计器。 / Empty counter.
        ///
        /// <!-- @brief 创建具有标准偏移基的写入器。 / Create a writer with the standard offset basis. -->
        /// <!-- @return 空统计器。 / Empty counter. -->
        const fn new() -> Self {
            Self {
                count: 0,
                hash: 0xcbf2_9ce4_8422_2325,
            }
        }
    }

    impl Default for CountingHashWriter {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Write for CountingHashWriter {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            self.count += input.len();
            for byte in input {
                self.hash ^= u64::from(*byte);
                self.hash = self.hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            Ok(input.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn json_is_exactly_one_versioned_document() {
        let mut output = Vec::new();
        write_values(&mut output, OutputFormat::Json, &[Value::Text("ok".into())]).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert_eq!(text.lines().count(), 1);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "schema_version": 1,
                "ok": true,
                "values": [{"type": "text", "value": "ok"}]
            })
        );
    }

    #[test]
    fn raw_refuses_non_xml_values() {
        let mut output = Vec::new();
        assert!(write_values(&mut output, OutputFormat::Raw, &[Value::Unit]).is_err());
        assert!(output.is_empty());
    }

    #[test]
    fn human_xml_preserves_canonical_bytes_without_an_extra_newline() {
        let mut output = Vec::new();
        let xml = "<Root>text</Root>\n";
        write_values(&mut output, OutputFormat::Human, &[Value::Xml(xml.into())]).unwrap();
        assert_eq!(output, xml.as_bytes());
    }

    #[test]
    fn raw_streams_a_spilled_value_byte_exactly() {
        let expected = b"<Root>streamed</Root>\n";
        let mut spool = SpillWriter::with_threshold(4);
        spool.write_all(expected).unwrap();
        let value = Value::Xml(spool.finish().unwrap());
        let mut output = Vec::new();
        write_values(&mut output, OutputFormat::Raw, &[value]).unwrap();
        assert_eq!(output, expected);
    }

    #[test]
    fn json_streams_and_escapes_spilled_utf8_xml() {
        let expected = "<Root>\"\\\n猫\t\r</Root>\n";
        let mut spool = SpillWriter::with_threshold(4);
        spool.write_all(expected.as_bytes()).unwrap();
        let xml = spool.finish().unwrap();
        assert!(xml.spilled_path().is_some());

        let mut output = Vec::new();
        write_values(&mut output, OutputFormat::Json, &[Value::Xml(xml)]).unwrap();
        let document: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(document["schema_version"], 1);
        assert_eq!(document["ok"], true);
        assert_eq!(document["values"][0]["type"], "xml");
        assert_eq!(document["values"][0]["value"], expected);
    }

    #[test]
    fn json_streams_more_than_memory_limit_into_a_hash_sink() {
        let xml_len = XML_MEMORY_LIMIT + 1;
        let chunk = [b'x'; 8192];
        let mut spool = SpillWriter::new();
        let mut remaining = xml_len;
        while remaining != 0 {
            let size = remaining.min(chunk.len());
            spool.write_all(&chunk[..size]).unwrap();
            remaining -= size;
        }
        let xml = spool.finish().unwrap();
        assert!(xml.spilled_path().is_some());

        let mut actual = CountingHashWriter::new();
        write_values(&mut actual, OutputFormat::Json, &[Value::Xml(xml)]).unwrap();

        let mut expected = CountingHashWriter::new();
        expected
            .write_all(br#"{"schema_version":1,"ok":true,"values":[{"type":"xml","value":""#)
            .unwrap();
        remaining = xml_len;
        while remaining != 0 {
            let size = remaining.min(chunk.len());
            expected.write_all(&chunk[..size]).unwrap();
            remaining -= size;
        }
        expected.write_all(b"\"}]}\n").unwrap();

        assert_eq!(actual.count, expected.count);
        assert_eq!(actual.hash, expected.hash);
        assert!(actual.count > XML_MEMORY_LIMIT);
    }
}
