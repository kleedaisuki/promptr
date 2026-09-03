//! UTF-8 字节精确词法器。

use super::ast::{DiagnosticCode, ParseDiagnostic, Span};

/// @brief 词法单元类别 / Lexical token kind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TokenKind {
    Ident(String),
    String(String),
    Colon,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Eof,
}

/// @brief 带位置的词法单元 / A positioned lexical token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) span: Span,
}

/// @brief 词法失败类型 / Lexing failure classification.
pub(crate) enum LexError {
    Incomplete(ParseDiagnostic),
    Invalid(ParseDiagnostic),
}

/// @brief 将源码切分为词法单元 / Tokenizes source text.
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

fn is_symbol_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn push_punct(tokens: &mut Vec<Token>, kind: TokenKind, start: usize, offset: &mut usize) {
    *offset += 1;
    tokens.push(Token {
        kind,
        span: Span::new(start, *offset),
    });
}

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

fn diag(code: DiagnosticCode, message: impl Into<String>, span: Span) -> ParseDiagnostic {
    ParseDiagnostic {
        code,
        message: message.into(),
        span,
    }
}
