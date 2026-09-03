//! 内置编辑器的宿主桥接。 / Host bridge for the built-in editor.

use super::{EditRequest, EditorError, EditorOutcome, PreparedEdit, TextProvider};

/// @brief 由 TUI 实现的内存编辑回调。 / In-memory editing callback implemented by the TUI.
pub trait BuiltinEditorHost {
    /// @brief 打开内置编辑会话。 / Open a built-in editing session.
    /// @param initial_text 编辑器的初始文本。 / Initial editor text.
    /// @return `Some` 表示显式保存，`None` 表示取消。 / `Some` means explicit save; `None` means cancellation.
    fn edit_text(&mut self, initial_text: &str) -> Result<Option<String>, EditorError>;
}

impl<F> BuiltinEditorHost for F
where
    F: FnMut(&str) -> Result<Option<String>, EditorError>,
{
    fn edit_text(&mut self, initial_text: &str) -> Result<Option<String>, EditorError> {
        self(initial_text)
    }
}

/// @brief 将宿主内存编辑会话适配为文本提供者。 / Adapt a host in-memory editing session into a text provider.
pub struct BuiltinTextProvider<H> {
    /// @brief 真正拥有 UI 会话的宿主回调。 / Host callback that owns the actual UI session.
    host: H,
}

impl<H> BuiltinTextProvider<H> {
    /// @brief 创建内置文本提供者。 / Create a built-in text provider.
    /// @param host 不持有存储的宿主编辑回调。 / Store-free host editing callback.
    /// @return 内置文本提供者。 / Built-in text provider.
    pub fn new(host: H) -> Self {
        Self { host }
    }

    /// @brief 取回宿主回调。 / Recover the host callback.
    /// @return 原始宿主回调。 / Original host callback.
    pub fn into_inner(self) -> H {
        self.host
    }
}

impl<H: BuiltinEditorHost> TextProvider for BuiltinTextProvider<H> {
    fn edit(&mut self, request: EditRequest) -> Result<EditorOutcome, EditorError> {
        let edited_text = self.host.edit_text(&request.original_text)?;
        Ok(match edited_text {
            Some(text) => EditorOutcome::Save(PreparedEdit::from_request(request, text)),
            None => EditorOutcome::Cancel,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::editor::EditTarget;

    fn request(text: &str) -> EditRequest {
        EditRequest {
            target: EditTarget {
                node_id: None,
                base_revision: None,
            },
            original_text: text.to_owned(),
        }
    }

    #[test]
    fn saved_content_round_trips_without_normalization() {
        let expected = "雪\r\n\0prompt\n".to_owned();
        let saved = expected.clone();
        let mut provider = BuiltinTextProvider::new(move |initial: &str| {
            assert_eq!(initial, "original");
            Ok(Some(saved.clone()))
        });

        let outcome = provider.edit(request("original")).unwrap();

        let EditorOutcome::Save(edit) = outcome else {
            panic!("expected save")
        };
        assert_eq!(edit.original_text, "original");
        assert_eq!(edit.edited_text, expected);
    }

    #[test]
    fn cancellation_never_constructs_a_prepared_edit() {
        let mut provider = BuiltinTextProvider::new(|_: &str| Ok(None));
        assert_eq!(
            provider.edit(request("draft")).unwrap(),
            EditorOutcome::Cancel
        );
    }
}
