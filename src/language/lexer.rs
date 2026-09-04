//! UTF-8 字节精确词法器。 / UTF-8 byte-accurate lexer.

use super::ast::{DiagnosticCode, ParseDiagnostic, Span};

/// 词法单元类别 / Lexical token kind.
///
/// <!-- @brief 词法单元类别 / Lexical token kind. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TokenKind {
    /// 未加引号的标识符。 / Unquoted identifier.
    Ident(String),
    /// 已解码的字符串字面量。 / Decoded string literal.
    String(String),
    /// `:` 标点。 / `:` punctuation.
    Colon,
    /// `[` 标点。 / `[` punctuation.
    LBracket,
    /// `]` 标点。 / `]` punctuation.
    RBracket,
    /// `,` 标点。 / `,` punctuation.
    Comma,
    /// `;` 标点。 / `;` punctuation.
    Semicolon,
    /// 输入结束哨兵。 / End-of-input sentinel.
    Eof,
}

/// 带位置的词法单元 / A positioned lexical token.
///
/// <!-- @brief 带位置的词法单元 / A positioned lexical token. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Token {
    /// 词法单元的类别与值。 / Token kind and value.
    pub(crate) kind: TokenKind,
    /// 词法单元在源码中的区间。 / Token's source span.
    pub(crate) span: Span,
}

/// 词法失败类型 / Lexing failure classification.
///
/// <!-- @brief 词法失败类型 / Lexing failure classification. -->
pub(crate) enum LexError {
    /// 输入前缀合法，但仍需要更多字符。 / Valid prefix requiring more input.
    Incomplete(ParseDiagnostic),
    /// 输入已确定无效。 / Definitely invalid input.
    Invalid(ParseDiagnostic),
}

/// 将源码切分为词法单元 / Tokenizes source text.
///
/// <!-- @brief 将源码切分为词法单元 / Tokenizes source text. -->
///
/// # Arguments
///
/// - `source`：要切分的 UTF-8 DSL 源码。 / UTF-8 DSL source to tokenize.
///
/// <!-- @param source 要切分的 UTF-8 DSL 源码。 / UTF-8 DSL source to tokenize. -->
///
/// # Errors
///
/// 当字符、转义或 REPL 专用语法无效时返回 [`LexError::Invalid`]；当字符串尚未闭合时返回 [`LexError::Incomplete`]。 /
/// Returns [`LexError::Invalid`] for invalid characters, escapes, or REPL-only syntax, and
/// [`LexError::Incomplete`] for an unterminated string.
///
/// <!-- @return 词法单元，或包含精确源码位置的词法错误。 / Tokens or a lexical error with an exact source span. -->
pub(crate) fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    let mut line_start = true;
    while offset < source.len() {
        let ch = source[offset..]
            .chars()
            .next()
            .expect("offset is a char boundary");
        if ch.is_whitespace() {
            line_start = ch == '\n' || (line_start && ch != '\r');
            offset += ch.len_utf8();
            continue;
        }
        let start = offset;
        if ch == '!' && line_start {
            let end = source[offset..]
                .find('\n')
                .map_or(source.len(), |n| offset + n);
            return Err(LexError::Invalid(diag(
                DiagnosticCode::ReplOnlyShell,
                "shell 转义是 REPL 元命令，不属于 DSL 程序",
                Span::new(start, end),
            )));
        }
        line_start = false;
        if is_symbol_start(ch) {
            offset += 1;
            while offset < source.len() {
                let next = source.as_bytes()[offset];
                if next.is_ascii_alphanumeric() || next == b'_' || next == b'-' {
                    offset += 1;
                } else {
                    break;
                }
            }
            tokens.push(Token {
                kind: TokenKind::Ident(source[start..offset].to_owned()),
                span: Span::new(start, offset),
            });
            continue;
        }
        if ch.is_ascii_digit() || ch == '-' {
            offset += 1;
            while offset < source.len() {
                let next = source.as_bytes()[offset];
                if next.is_ascii_alphanumeric() || next == b'_' || next == b'-' {
                    offset += 1;
                } else {
                    break;
                }
            }
            return Err(LexError::Invalid(diag(
                DiagnosticCode::InvalidSymbol,
                "符号必须以 ASCII 字母或下划线开头",
                Span::new(start, offset),
            )));
        }
        match ch {
            ':' => push_punct(&mut tokens, TokenKind::Colon, start, &mut offset),
            '[' => push_punct(&mut tokens, TokenKind::LBracket, start, &mut offset),
            ']' => push_punct(&mut tokens, TokenKind::RBracket, start, &mut offset),
            ',' => push_punct(&mut tokens, TokenKind::Comma, start, &mut offset),
            ';' => push_punct(&mut tokens, TokenKind::Semicolon, start, &mut offset),
            '"' => tokens.push(read_string(source, &mut offset)?),
            _ => {
                return Err(LexError::Invalid(diag(
                    DiagnosticCode::InvalidCharacter,
                    format!("DSL 中不允许字符 `{ch}`"),
                    Span::new(start, start + ch.len_utf8()),
                )));
            }
        }
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span::new(source.len(), source.len()),
    });
    Ok(tokens)
}

/// 判断字符能否开始 DSL 符号。 / Tests whether a character can start a DSL symbol.
fn is_symbol_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

/// 记录一个 ASCII 标点并前移字节偏移。 / Records one ASCII punctuation token and advances the byte offset.
fn push_punct(tokens: &mut Vec<Token>, kind: TokenKind, start: usize, offset: &mut usize) {
    *offset += 1;
    tokens.push(Token {
        kind,
        span: Span::new(start, *offset),
    });
}

/// 从当前双引号处读取并解码字符串。 / Reads and decodes a string starting at the current double quote.
///
/// # Errors
///
/// 转义无效时返回 [`LexError::Invalid`]，字符串未闭合时返回 [`LexError::Incomplete`]。 /
/// Returns [`LexError::Invalid`] for invalid escapes and [`LexError::Incomplete`] when the
/// closing quote is absent.
fn read_string(source: &str, offset: &mut usize) -> Result<Token, LexError> {
    let start = *offset;
    *offset += 1;
    let mut value = String::new();
    while *offset < source.len() {
        let ch = source[*offset..]
            .chars()
            .next()
            .expect("offset is a char boundary");
        if ch == '"' {
            *offset += 1;
            return Ok(Token {
                kind: TokenKind::String(value),
                span: Span::new(start, *offset),
            });
        }
        if ch != '\\' {
            value.push(ch);
            *offset += ch.len_utf8();
            continue;
        }
        let escape_start = *offset;
        *offset += 1;
        if *offset == source.len() {
            return Err(LexError::Incomplete(diag(
                DiagnosticCode::UnexpectedToken,
                "字符串转义尚未完成",
                Span::new(escape_start, *offset),
            )));
        }
        let escaped = source[*offset..]
            .chars()
            .next()
            .expect("offset is a char boundary");
        let decoded = match escaped {
            '"' => '"',
            '\\' => '\\',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            _ => {
                return Err(LexError::Invalid(diag(
                    DiagnosticCode::InvalidEscape,
                    format!(r#"不支持转义 `\{escaped}`；仅支持 \"、\\、\n、\r、\t"#),
                    Span::new(escape_start, *offset + escaped.len_utf8()),
                )));
            }
        };
        value.push(decoded);
        *offset += escaped.len_utf8();
    }
    Err(LexError::Incomplete(diag(
        DiagnosticCode::UnexpectedToken,
        "字符串字面量尚未闭合",
        Span::new(start, source.len()),
    )))
}

/// 构造一个带稳定代码和源码位置的诊断。 / Builds a diagnostic with a stable code and source span.
fn diag(code: DiagnosticCode, message: impl Into<String>, span: Span) -> ParseDiagnostic {
    ParseDiagnostic {
        code,
        message: message.into(),
        span,
    }
}
