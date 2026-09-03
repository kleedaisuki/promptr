//! 递归下降语法分析器。

use super::ast::*;
use super::lexer::{LexError, Token, TokenKind, lex};

type PResult<T> = Result<T, ParseFailure>;

enum ParseFailure {
    Incomplete(ParseDiagnostic),
    Invalid(ParseDiagnostic),
}

/// @brief 解析完整 DSL 程序 / Parses a complete DSL program.
/// @param source UTF-8 DSL 源码 / UTF-8 DSL source.
/// @return 完整、待续或无效结果 / Complete, incomplete, or invalid outcome.
pub fn parse(source: &str) -> ParseOutcome {
    let tokens = match lex(source) {
        Ok(tokens) => tokens,
        Err(LexError::Incomplete(diagnostic)) => return ParseOutcome::Incomplete(diagnostic),
        Err(LexError::Invalid(diagnostic)) => return ParseOutcome::Invalid(diagnostic),
    };
    match (Parser { tokens, cursor: 0 }).program(source.len()) {
        Ok(program) => ParseOutcome::Complete(program),
        Err(ParseFailure::Incomplete(diagnostic)) => ParseOutcome::Incomplete(diagnostic),
        Err(ParseFailure::Invalid(diagnostic)) => ParseOutcome::Invalid(diagnostic),
    }
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    fn program(&mut self, source_len: usize) -> PResult<Program> {
        let mut statements = Vec::new();
        while !matches!(self.peek().kind, TokenKind::Eof) {
            statements.push(self.statement()?);
        }
        Ok(Program {
            statements,
            span: Span::new(0, source_len),
        })
    }

    fn statement(&mut self) -> PResult<Spanned<Statement>> {
        let start = self.peek().span.start;
        let leading = self.expect_ident("语句或提示词名称")?;
        let statement = if self.consume_punct(&TokenKind::Colon) {
            self.prompt(leading)?
        } else if keyword(&leading.value, "FRAGMENT") {
            Statement::Fragment {
                symbol: self.expect_symbol("片段名称")?,
            }
        } else if keyword(&leading.value, "RENAME") {
            let old = self.expect_symbol("原名称")?;
            self.expect_keyword("TO")?;
            Statement::Rename {
                old,
                new: self.expect_symbol("新名称")?,
            }
        } else if keyword(&leading.value, "DELETE") {
            Statement::Delete {
                symbol: self.expect_symbol("待删除名称")?,
            }
        } else if keyword(&leading.value, "LIST") {
            Statement::List {
                filter: self.list_filter()?,
            }
        } else if keyword(&leading.value, "PRINT") {
            Statement::Print {
                symbol: self.expect_symbol("待检查名称")?,
            }
        } else if keyword(&leading.value, "OUTPUT") {
            Statement::Output {
                symbol: self.expect_symbol("待输出名称")?,
            }
        } else if keyword(&leading.value, "SEARCH") {
            self.search()?
        } else if keyword(&leading.value, "FIND") {
            self.find()?
        } else if keyword(&leading.value, "METADATA") {
            self.metadata()?
        } else {
            return Err(self.invalid(
                DiagnosticCode::UnexpectedToken,
                format!(
                    "未知语句 `{}`；提示词声明需要在名称后使用 `:`",
                    leading.value
                ),
                leading.span,
            ));
        };
        let semi = self.expect_punct(TokenKind::Semicolon, "`;`")?;
        Ok(Spanned::new(statement, Span::new(start, semi.span.end)))
    }

    fn prompt(&mut self, symbol: Spanned<String>) -> PResult<Statement> {
        self.expect_punct(TokenKind::LBracket, "`[`")?;
        if self.consume_punct(&TokenKind::RBracket) {
            return Err(self.invalid(
                DiagnosticCode::EmptyPrompt,
                "提示词至少需要一个子节点",
                self.previous().span,
            ));
        }
        let mut children = vec![self.expect_symbol("子节点名称")?];
        while self.consume_punct(&TokenKind::Comma) {
            children.push(self.expect_symbol("逗号后的子节点名称")?);
        }
        self.expect_punct(TokenKind::RBracket, "`]`")?;
        Ok(Statement::Prompt { symbol, children })
    }

    fn list_filter(&mut self) -> PResult<ListFilter> {
        if self.check_punct(&TokenKind::Semicolon) {
            return Ok(ListFilter::All);
        }
        let token = self.expect_ident("FRAGMENTS、PROMPTS 或 `;`")?;
        if keyword(&token.value, "FRAGMENTS") {
            Ok(ListFilter::Fragments)
        } else if keyword(&token.value, "PROMPTS") {
            Ok(ListFilter::Prompts)
        } else {
            Err(self.invalid(
                DiagnosticCode::UnexpectedToken,
                "LIST 仅接受 FRAGMENTS 或 PROMPTS",
                token.span,
            ))
        }
    }

    fn search(&mut self) -> PResult<Statement> {
        let query = self.expect_string("搜索字符串")?;
        let field = self.optional_field()?;
        Ok(Statement::Search { query, field })
    }

    fn find(&mut self) -> PResult<Statement> {
        let query = self.expect_string("搜索字符串")?;
        self.expect_keyword("ON")?;
        let prompt = self.expect_symbol("搜索范围提示词")?;
        let field = self.optional_field()?;
        Ok(Statement::Find {
            query,
            prompt,
            field,
        })
    }

    fn optional_field(&mut self) -> PResult<Option<SearchField>> {
        if self.check_punct(&TokenKind::Semicolon) {
            return Ok(None);
        }
        self.expect_keyword("FROM")?;
        let field = self.expect_ident("TITLE、CONTENT 或 MIXED")?;
        if keyword(&field.value, "TITLE") {
            Ok(Some(SearchField::Title))
        } else if keyword(&field.value, "CONTENT") {
            Ok(Some(SearchField::Content))
        } else if keyword(&field.value, "MIXED") {
            Ok(Some(SearchField::Mixed))
        } else {
            Err(self.invalid(
                DiagnosticCode::UnexpectedToken,
                "FROM 仅接受 TITLE、CONTENT 或 MIXED",
                field.span,
            ))
        }
    }

    fn metadata(&mut self) -> PResult<Statement> {
        let symbol = self.expect_symbol("元数据节点名称")?;
        let kind = self.expect_ident("DESCRIPTION 或 TAGS")?;
        let value = if keyword(&kind.value, "DESCRIPTION") {
            MetadataValue::Description(self.expect_string("描述字符串")?)
        } else if keyword(&kind.value, "TAGS") {
            MetadataValue::Tags(self.string_list()?)
        } else {
            return Err(self.invalid(
                DiagnosticCode::UnexpectedToken,
                "METADATA 仅接受 DESCRIPTION 或 TAGS",
                kind.span,
            ));
        };
        Ok(Statement::Metadata { symbol, value })
    }

    fn string_list(&mut self) -> PResult<Vec<Spanned<String>>> {
        self.expect_punct(TokenKind::LBracket, "`[`")?;
        let mut values = Vec::new();
        if self.consume_punct(&TokenKind::RBracket) {
            return Ok(values);
        }
        values.push(self.expect_string("标签字符串")?);
        while self.consume_punct(&TokenKind::Comma) {
            values.push(self.expect_string("逗号后的标签字符串")?);
        }
        self.expect_punct(TokenKind::RBracket, "`]`")?;
        Ok(values)
    }

    fn expect_symbol(&mut self, expected: &str) -> PResult<Spanned<String>> {
        self.expect_ident(expected)
    }

    fn expect_ident(&mut self, expected: &str) -> PResult<Spanned<String>> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Ident(value) => {
                self.cursor += 1;
                Ok(Spanned::new(value, token.span))
            }
            TokenKind::Eof => Err(self.incomplete(expected)),
            _ => Err(self.invalid(
                DiagnosticCode::UnexpectedToken,
                format!("此处需要{expected}"),
                token.span,
            )),
        }
    }

    fn expect_string(&mut self, expected: &str) -> PResult<Spanned<String>> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::String(value) => {
                self.cursor += 1;
                Ok(Spanned::new(value, token.span))
            }
            TokenKind::Eof => Err(self.incomplete(expected)),
            _ => Err(self.invalid(
                DiagnosticCode::UnexpectedToken,
                format!("此处需要{expected}"),
                token.span,
            )),
        }
    }

    fn expect_keyword(&mut self, expected: &'static str) -> PResult<()> {
        let token = self.expect_ident(expected)?;
        if keyword(&token.value, expected) {
            Ok(())
        } else {
            Err(self.invalid(
                DiagnosticCode::UnexpectedToken,
                format!("此处需要关键字 {expected}"),
                token.span,
            ))
        }
    }

    fn expect_punct(&mut self, expected: TokenKind, label: &str) -> PResult<Token> {
        let token = self.peek().clone();
        if same_punct(&token.kind, &expected) {
            self.cursor += 1;
            Ok(token)
        } else if matches!(token.kind, TokenKind::Eof) {
            Err(self.incomplete(label))
        } else {
            Err(self.invalid(
                DiagnosticCode::UnexpectedToken,
                format!("此处需要{label}"),
                token.span,
            ))
        }
    }

    fn consume_punct(&mut self, expected: &TokenKind) -> bool {
        if self.check_punct(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn check_punct(&self, expected: &TokenKind) -> bool {
        same_punct(&self.peek().kind, expected)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.cursor - 1]
    }

    fn incomplete(&self, expected: &str) -> ParseFailure {
        ParseFailure::Incomplete(ParseDiagnostic {
            code: DiagnosticCode::UnexpectedToken,
            message: format!("输入尚未完成，此处需要{expected}"),
            span: self.peek().span,
        })
    }

    fn invalid(
        &self,
        code: DiagnosticCode,
        message: impl Into<String>,
        span: Span,
    ) -> ParseFailure {
        ParseFailure::Invalid(ParseDiagnostic {
            code,
            message: message.into(),
            span,
        })
    }
}

fn keyword(actual: &str, expected: &str) -> bool {
    actual.eq_ignore_ascii_case(expected)
}

fn same_punct(actual: &TokenKind, expected: &TokenKind) -> bool {
    matches!(
        (actual, expected),
        (TokenKind::Colon, TokenKind::Colon)
            | (TokenKind::LBracket, TokenKind::LBracket)
            | (TokenKind::RBracket, TokenKind::RBracket)
            | (TokenKind::Comma, TokenKind::Comma)
            | (TokenKind::Semicolon, TokenKind::Semicolon)
            | (TokenKind::Eof, TokenKind::Eof)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(source: &str) -> Program {
        match parse(source) {
            ParseOutcome::Complete(program) => program,
            outcome => panic!("expected complete parse, got {outcome:?}"),
        }
    }

    #[test]
    fn parses_every_statement_and_multiple_statements() {
        let source = concat!(
            "fragment Notice; Root: [Notice, Notice]; ",
            "rename Notice to Note; delete Note; list; LIST fragments; list PROMPTS; ",
            "print Root; output Root; search \"x\"; SEARCH \"x\" FROM title; ",
            "find \"y\" on Root from CONTENT; ",
            "metadata Root description \"doc\"; metadata Root tags [\"a\", \"b\"];"
        );
        let program = complete(source);
        assert_eq!(program.statements.len(), 14);
        assert_eq!(program.span, Span::new(0, source.len()));
    }

    #[test]
    fn preserves_omitted_search_fields_for_runtime_defaults() {
        let program =
            complete("SEARCH \"a\"; SEARCH \"b\" FROM MIXED; FIND \"c\" ON Root FROM TITLE;");
        assert!(matches!(
            program.statements[0].value,
            Statement::Search { field: None, .. }
        ));
        assert!(matches!(
            program.statements[1].value,
            Statement::Search {
                field: Some(SearchField::Mixed),
                ..
            }
        ));
        assert!(matches!(
            program.statements[2].value,
            Statement::Find {
                field: Some(SearchField::Title),
                ..
            }
        ));
    }

    #[test]
    fn keywords_are_contextual_and_case_insensitive() {
        let program = complete("FRAGMENT Fragment; list: [Fragment]; OUTPUT Output;");
        assert!(matches!(
            program.statements[1].value,
            Statement::Prompt { .. }
        ));
    }

    #[test]
    fn spans_are_utf8_byte_offsets_and_strings_are_decoded() {
        let source = "SEARCH \"你好\\n🌍\";";
        let program = complete(source);
        let Statement::Search { query, .. } = &program.statements[0].value else {
            panic!()
        };
        assert_eq!(query.value, "你好\n🌍");
        assert_eq!(&source[query.span.start..query.span.end], "\"你好\\n🌍\"");
        assert_eq!(program.statements[0].span.end, source.len());
    }

    #[test]
    fn reports_incomplete_prefixes() {
        for source in [
            "FRAGMENT",
            "FRAGMENT A",
            "A: [B,",
            "SEARCH \"open",
            "METADATA A TAGS [\"x\",",
        ] {
            assert!(
                matches!(parse(source), ParseOutcome::Incomplete(_)),
                "{source}"
            );
        }
    }

    #[test]
    fn rejects_definitely_invalid_syntax_and_symbols() {
        for source in [
            "A: [];",
            "FRAGMENT 123Foo;",
            "FRAGMENT foo.bar;",
            "A: [B [C];",
            "WAT A;",
        ] {
            assert!(
                matches!(parse(source), ParseOutcome::Invalid(_)),
                "{source}"
            );
        }
    }

    #[test]
    fn only_documented_escapes_are_accepted() {
        let program = complete(r#"SEARCH "\"\\\n\r\t";"#);
        let Statement::Search { query, .. } = &program.statements[0].value else {
            panic!()
        };
        assert_eq!(query.value, "\"\\\n\r\t");
        let ParseOutcome::Invalid(diagnostic) = parse(r#"SEARCH "\q";"#) else {
            panic!()
        };
        assert_eq!(diagnostic.code, DiagnosticCode::InvalidEscape);
    }

    #[test]
    fn metadata_accepts_clear_values() {
        let program = complete("METADATA A DESCRIPTION \"\"; METADATA A TAGS [];");
        assert!(
            matches!(program.statements[0].value, Statement::Metadata { value: MetadataValue::Description(ref text), .. } if text.value.is_empty())
        );
        assert!(
            matches!(program.statements[1].value, Statement::Metadata { value: MetadataValue::Tags(ref tags), .. } if tags.is_empty())
        );
    }

    #[test]
    fn shell_syntax_has_stable_repl_only_diagnostic() {
        for source in ["! echo hi", "  ! echo hi\n"] {
            let ParseOutcome::Invalid(diagnostic) = parse(source) else {
                panic!()
            };
            assert_eq!(diagnostic.code, DiagnosticCode::ReplOnlyShell);
            assert_eq!(diagnostic.code.as_str(), "E0006");
        }
    }
}
