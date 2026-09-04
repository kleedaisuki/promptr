//! 递归下降语法分析器。 / Recursive-descent parser.

use super::ast::*;
use super::lexer::{LexError, Token, TokenKind, lex};

/// 语法分析器的内部结果类型。 / Internal parser result type.
type PResult<T> = Result<T, ParseFailure>;

/// 区分可通过继续输入修复与确定无效的分析失败。 / Parser failure distinguishing continuable and definite errors.
enum ParseFailure {
    /// 还需要更多输入。 / More input is required.
    Incomplete(ParseDiagnostic),
    /// 输入已确定无效。 / Input is definitely invalid.
    Invalid(ParseDiagnostic),
}

/// 解析完整 DSL 程序 / Parses a complete DSL program.
///
/// <!-- @brief 解析完整 DSL 程序 / Parses a complete DSL program. -->
///
/// # Arguments
///
/// - `source`：UTF-8 DSL 源码 / UTF-8 DSL source.
///
/// <!-- @param source UTF-8 DSL 源码 / UTF-8 DSL source. -->
///
/// # Returns
///
/// 完整、待续或无效结果 / Complete, incomplete, or invalid outcome.
///
/// <!-- @return 完整、待续或无效结果 / Complete, incomplete, or invalid outcome. -->
///
/// # Examples
///
/// ```
/// use promptr::language::{ParseOutcome, parse};
///
/// let outcome = parse("fragment Greeting;");
/// assert!(matches!(outcome, ParseOutcome::Complete(_)));
/// ```
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

/// 在词法单元流上维护游标的递归下降分析器。 / Cursor-based recursive-descent parser over a token stream.
struct Parser {
    /// 包含结束哨兵的词法单元。 / Tokens including the end-of-input sentinel.
    tokens: Vec<Token>,
    /// 下一个待消费词法单元的索引。 / Index of the next token to consume.
    cursor: usize,
}

impl Parser {
    /// 分析整个程序并要求消费所有语句。 / Parses a whole program and consumes every statement.
    ///
    /// # Errors
    ///
    /// 任一语句不完整或无效时返回对应的 [`ParseFailure`]。 / Returns the corresponding [`ParseFailure`] when any statement is incomplete or invalid.
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

    /// 分析一条语句并保留完整源码区间。 / Parses one statement and preserves its full source span.
    ///
    /// # Errors
    ///
    /// 语句缺失词法单元或不符合 DSL 语法时返回 [`ParseFailure`]。 / Returns [`ParseFailure`] when tokens are missing or violate the DSL grammar.
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

    /// 分析提示词的非空子符号列表。 / Parses a prompt's non-empty child-symbol list.
    ///
    /// # Errors
    ///
    /// 列表为空、未闭合或子符号无效时返回 [`ParseFailure`]。 / Returns [`ParseFailure`] for an empty, unterminated, or invalid child list.
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

    /// 分析 `List` 的可选节点类型过滤器。 / Parses the optional node-kind filter of `List`.
    ///
    /// # Errors
    ///
    /// 过滤器不是受支持的关键字时返回 [`ParseFailure::Invalid`]。 / Returns [`ParseFailure::Invalid`] for an unsupported filter keyword.
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

    /// 分析全局 `Search` 语句。 / Parses a global `Search` statement.
    ///
    /// # Errors
    ///
    /// 查询字符串或字段选项缺失、无效时返回 [`ParseFailure`]。 / Returns [`ParseFailure`] when the query string or field option is missing or invalid.
    fn search(&mut self) -> PResult<Statement> {
        let query = self.expect_string("搜索字符串")?;
        let field = self.optional_field()?;
        Ok(Statement::Search { query, field })
    }

    /// 分析限定提示词范围的 `Find` 语句。 / Parses a prompt-scoped `Find` statement.
    ///
    /// # Errors
    ///
    /// 查询、`in`、提示词符号或字段选项缺失、无效时返回 [`ParseFailure`]。 / Returns [`ParseFailure`] when the query, `in`, prompt symbol, or field option is missing or invalid.
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

    /// 分析可选的 `in title|content|mixed` 字段选择。 / Parses an optional `in title|content|mixed` field selection.
    ///
    /// # Errors
    ///
    /// `in` 之后缺少或包含未知字段名时返回 [`ParseFailure`]。 / Returns [`ParseFailure`] when `in` is followed by a missing or unknown field name.
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

    /// 分析描述或标签的完整元数据替换。 / Parses a complete description or tag metadata replacement.
    ///
    /// # Errors
    ///
    /// 目标符号、元数据键或值无效时返回 [`ParseFailure`]。 / Returns [`ParseFailure`] when the target symbol, metadata key, or value is invalid.
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

    /// 分析可为空的逗号分隔字符串列表。 / Parses a possibly empty comma-separated string list.
    ///
    /// # Errors
    ///
    /// 字符串或标点缺失、无效时返回 [`ParseFailure`]。 / Returns [`ParseFailure`] for missing or invalid strings or punctuation.
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

    /// 消费一个符号标识符。 / Consumes one symbol identifier.
    ///
    /// # Errors
    ///
    /// 下一词法单元不是标识符或符号格式无效时返回 [`ParseFailure`]。 / Returns [`ParseFailure`] when the next token is not an identifier or is not a valid symbol.
    fn expect_symbol(&mut self, expected: &str) -> PResult<Spanned<String>> {
        self.expect_ident(expected)
    }

    /// 消费一个标识符。 / Consumes one identifier.
    ///
    /// # Errors
    ///
    /// 输入结束时返回 [`ParseFailure::Incomplete`]，下一词法单元类型不匹配时返回 [`ParseFailure::Invalid`]。 / Returns [`ParseFailure::Incomplete`] at end of input and [`ParseFailure::Invalid`] for a mismatched token.
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

    /// 消费一个字符串字面量。 / Consumes one string literal.
    ///
    /// # Errors
    ///
    /// 输入结束时返回 [`ParseFailure::Incomplete`]，下一词法单元类型不匹配时返回 [`ParseFailure::Invalid`]。 / Returns [`ParseFailure::Incomplete`] at end of input and [`ParseFailure::Invalid`] for a mismatched token.
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

    /// 以 ASCII 大小写不敏感方式消费指定关键字。 / Consumes a keyword using ASCII case-insensitive comparison.
    ///
    /// # Errors
    ///
    /// 关键字缺失或不匹配时返回 [`ParseFailure`]。 / Returns [`ParseFailure`] when the keyword is missing or mismatched.
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

    /// 消费指定标点词法单元。 / Consumes the requested punctuation token.
    ///
    /// # Errors
    ///
    /// 标点缺失或不匹配时返回 [`ParseFailure`]。 / Returns [`ParseFailure`] when punctuation is missing or mismatched.
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

    /// 如果下一词法单元是指定标点，则消费它。 / Consumes the next token when it is the requested punctuation.
    fn consume_punct(&mut self, expected: &TokenKind) -> bool {
        if self.check_punct(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    /// 检查下一词法单元是否为指定标点。 / Tests whether the next token is the requested punctuation.
    fn check_punct(&self, expected: &TokenKind) -> bool {
        same_punct(&self.peek().kind, expected)
    }

    /// 查看下一词法单元而不消费。 / Peeks at the next token without consuming it.
    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    /// 返回最近消费的词法单元。 / Returns the most recently consumed token.
    fn previous(&self) -> &Token {
        &self.tokens[self.cursor - 1]
    }

    /// 为当前输入结束位置构造待续诊断。 / Builds a continuation diagnostic at the current end-of-input position.
    fn incomplete(&self, expected: &str) -> ParseFailure {
        ParseFailure::Incomplete(ParseDiagnostic {
            code: DiagnosticCode::UnexpectedToken,
            message: format!("输入尚未完成，此处需要{expected}"),
            span: self.peek().span,
        })
    }

    /// 为当前词法单元构造确定无效的诊断。 / Builds a definite-invalid diagnostic for the current token.
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

/// 以 ASCII 大小写不敏感方式比较关键字。 / Compares keywords using ASCII case-insensitive matching.
fn keyword(actual: &str, expected: &str) -> bool {
    actual.eq_ignore_ascii_case(expected)
}

/// 比较两个词法单元类别是否表示同一标点。 / Tests whether two token kinds represent the same punctuation.
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
