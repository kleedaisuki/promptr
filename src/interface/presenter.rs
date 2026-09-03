//! 人类与机器输出适配器。 / Human and machine output adapters.

use std::io::{self, Write};

use clap::ValueEnum;
use serde::Serialize;

use crate::{Diagnostic, Value};

/// @brief 命令行输出格式。 / Command-line output format.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    /// @brief 面向人的稳定文本。 / Stable human-oriented text.
    #[default]
    Human,
    /// @brief 单个版本化 JSON 文档。 / One versioned JSON document.
    Json,
    /// @brief 仅规范 XML 字节。 / Canonical XML bytes only.
    Raw,
}

/// @brief JSON 机器接口的顶层文档。 / Top-level JSON machine-interface document.
#[derive(Debug, Serialize)]
struct JsonDocument<'a, T: Serialize + ?Sized> {
    /// @brief 接口模式版本。 / Interface schema version.
    schema_version: u32,
    /// @brief 操作是否成功。 / Whether the operation succeeded.
    ok: bool,
    /// @brief 成功负载。 / Success payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    values: Option<&'a T>,
    /// @brief 失败诊断。 / Failure diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a [Diagnostic]>,
}

/// @brief 将类型化运行时值写入指定输出流。 / Write typed runtime values to a selected output stream.
/// @param writer 标准输出等目标。 / Destination such as standard output.
/// @param format 输出格式。 / Output format.
/// @param values 已提交的类型化值。 / Committed typed values.
/// @return 写入成功或 I/O 错误。 / Success or an I/O error.
pub fn write_values(
    writer: &mut dyn Write,
    format: OutputFormat,
    values: &[Value],
) -> io::Result<()> {
    match format {
        OutputFormat::Human => write_human(writer, values),
        OutputFormat::Json => write_json_success(writer, values),
        OutputFormat::Raw => write_raw(writer, values),
    }
}

/// @brief 写入任意可序列化的管理命令结果。 / Write any serializable management-command result.
/// @param writer 输出目标。 / Output destination.
/// @param format 输出格式。 / Output format.
/// @param value 成功负载。 / Success payload.
/// @return 写入成功或 I/O 错误。 / Success or an I/O error.
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

/// @brief 发布一个结构化失败。 / Publish one structured failure.
/// @param stdout 标准输出。 / Standard output.
/// @param stderr 标准错误。 / Standard error.
/// @param format 输出格式。 / Output format.
/// @param diagnostic 诊断。 / Diagnostic.
/// @return 写入成功或 I/O 错误。 / Success or an I/O error.
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

    #[test]
    fn json_is_exactly_one_versioned_document() {
        let mut output = Vec::new();
        write_values(&mut output, OutputFormat::Json, &[Value::Text("ok".into())]).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert_eq!(text.lines().count(), 1);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["ok"], true);
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
}
