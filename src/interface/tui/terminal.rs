//! Crossterm 终端生命周期与幂等恢复。 / Crossterm terminal lifecycle and idempotent restoration.

use std::io::{self, Write};

use crossterm::{
    cursor::Show,
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

/// 可替换的终端生命周期后端。 / Replaceable terminal-lifecycle backend.
///
/// <!-- @brief 可替换的终端生命周期后端。 / Replaceable terminal-lifecycle backend. -->
pub trait TerminalOps {
    /// 进入交互终端模式。 / Enter interactive terminal mode.
    ///
    /// <!-- @brief 进入交互终端模式。 / Enter interactive terminal mode. -->
    ///
    /// # Errors
    ///
    /// 当后端无法启用任一请求的终端能力时返回 I/O 错误。 /
    /// Returns an I/O error when the backend cannot enable a requested terminal capability.
    fn enter(&mut self, mouse: bool) -> io::Result<()>;
    /// 恢复普通终端模式。 / Restore ordinary terminal mode.
    ///
    /// <!-- @brief 恢复普通终端模式。 / Restore ordinary terminal mode. -->
    ///
    /// # Errors
    ///
    /// 当后端无法恢复一个或多个终端能力时返回 I/O 错误。 /
    /// Returns an I/O error when the backend cannot restore one or more terminal capabilities.
    fn restore(&mut self, mouse: bool) -> io::Result<()>;
}

/// 使用标准输出的 Crossterm 生命周期后端。 / Crossterm lifecycle backend using standard output.
///
/// <!-- @brief 使用标准输出的 Crossterm 生命周期后端。 / Crossterm lifecycle backend using standard output. -->
#[derive(Debug, Default)]
pub struct CrosstermOps;

impl TerminalOps for CrosstermOps {
    fn enter(&mut self, mouse: bool) -> io::Result<()> {
        enable_raw_mode()?;
        let mut output = io::stdout();
        execute!(
            output,
            EnterAlternateScreen,
            EnableFocusChange,
            EnableBracketedPaste
        )?;
        if mouse {
            execute!(output, EnableMouseCapture)?;
        }
        output.flush()
    }

    fn restore(&mut self, mouse: bool) -> io::Result<()> {
        let mut output = io::stdout();
        if mouse {
            let _ = execute!(output, DisableMouseCapture);
        }
        let command_result = execute!(
            output,
            DisableBracketedPaste,
            DisableFocusChange,
            LeaveAlternateScreen,
            Show
        );
        let raw_result = disable_raw_mode();
        command_result.and(raw_result)
    }
}

/// RAII 终端会话，恢复操作可安全重复调用。 / RAII terminal session with safely repeatable restoration.
///
/// <!-- @brief RAII 终端会话，恢复操作可安全重复调用。 / RAII terminal session with safely repeatable restoration. -->
pub struct TerminalSession<O: TerminalOps = CrosstermOps> {
    ops: O,
    active: bool,
    mouse: bool,
}

impl<O: TerminalOps> TerminalSession<O> {
    /// 进入终端；失败时补偿任何已发生的部分副作用。 / Enter the terminal, compensating any partial side effects on failure.
    ///
    /// # Arguments / 参数
    ///
    /// - `self` — 当前终端会话。 / Current terminal session.
    ///
    /// # Returns / 返回值
    ///
    /// 成功时返回空值，失败时返回原始进入错误。 / Unit on success, or the original entry error on failure.
    ///
    /// <!-- @brief 进入终端；失败时补偿任何已发生的部分副作用。 / Enter the terminal, compensating any partial side effects on failure. -->
    /// <!-- @param self 当前终端会话。 / Current terminal session. -->
    /// <!-- @return 成功时返回空值，失败时返回原始进入错误。 / Unit on success, or the original entry error on failure. -->
    fn enter(&mut self) -> io::Result<()> {
        self.active = false;
        match self.ops.enter(self.mouse) {
            Ok(()) => {
                self.active = true;
                Ok(())
            }
            Err(error) => {
                let _ = self.ops.restore(self.mouse);
                Err(error)
            }
        }
    }

    /// 进入终端，恢复由 RAII 析构统一负责。 / Enter the terminal, leaving restoration to RAII destruction.
    ///
    /// # Arguments / 参数
    ///
    /// - `ops` — 生命周期后端。 / Lifecycle backend.
    /// - `mouse` — 是否启用鼠标捕获。 / Whether mouse capture is enabled.
    ///
    /// # Returns / 返回值
    ///
    /// 活跃会话或底层 I/O 错误。 / Active session or underlying I/O error.
    ///
    /// # Errors
    ///
    /// 当进入原始模式、备用屏幕或可选鼠标捕获失败时返回 I/O 错误；已发生的部分副作用会被补偿。 /
    /// Returns an I/O error when entering raw mode, the alternate screen, or optional mouse capture
    /// fails; any partial side effects are compensated.
    ///
    /// <!-- @brief 进入终端，恢复由 RAII 析构统一负责。 / Enter the terminal, leaving restoration to RAII destruction. -->
    /// <!-- @param ops 生命周期后端。 / Lifecycle backend. -->
    /// <!-- @param mouse 是否启用鼠标捕获。 / Whether mouse capture is enabled. -->
    /// <!-- @return 活跃会话或底层 I/O 错误。 / Active session or underlying I/O error. -->
    pub fn start(ops: O, mouse: bool) -> io::Result<Self> {
        let mut session = Self {
            ops,
            active: false,
            mouse,
        };
        session.enter()?;
        Ok(session)
    }

    /// 幂等恢复终端。 / Restore the terminal idempotently.
    ///
    /// <!-- @brief 幂等恢复终端。 / Restore the terminal idempotently. -->
    ///
    /// # Errors
    ///
    /// 当活动后端无法撤销终端能力时返回 I/O 错误；非活动会话总是成功。 /
    /// Returns an I/O error when the active backend cannot undo terminal capabilities; an inactive
    /// session always succeeds.
    pub fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.ops.restore(self.mouse)?;
        self.active = false;
        Ok(())
    }

    /// 暂停 TUI 执行外部编辑器，并在返回后恢复 TUI。 / Suspend the TUI around an external editor and resume afterward.
    ///
    /// # Arguments / 参数
    ///
    /// - `operation` — 在普通终端模式执行的操作。 / Operation run in ordinary terminal mode.
    ///
    /// # Returns / 返回值
    ///
    /// 外部操作结果，或重新进入终端的 I/O 错误。 / External result, or an I/O error while re-entering.
    ///
    /// # Errors
    ///
    /// 当会话无法恢复普通终端状态，或操作完成后无法重新进入交互模式时返回 I/O 错误。 /
    /// Returns an I/O error when the session cannot restore ordinary terminal state or cannot
    /// re-enter interactive mode after the operation.
    ///
    /// <!-- @brief 暂停 TUI 执行外部编辑器，并在返回后恢复 TUI。 / Suspend the TUI around an external editor and resume afterward. -->
    /// <!-- @param operation 在普通终端模式执行的操作。 / Operation run in ordinary terminal mode. -->
    /// <!-- @return 外部操作结果，或重新进入终端的 I/O 错误。 / External result, or an I/O error while re-entering. -->
    pub fn suspend<T>(&mut self, operation: impl FnOnce() -> T) -> io::Result<T> {
        self.restore()?;
        let result = operation();
        self.enter()?;
        Ok(result)
    }
}

impl<O: TerminalOps> Drop for TerminalSession<O> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        panic::{self, AssertUnwindSafe, PanicHookInfo},
        sync::{Arc, Mutex},
    };

    type TestPanicHook = dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static;
    static HOOK_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct OriginalHook(Option<Box<TestPanicHook>>);

    impl Drop for OriginalHook {
        fn drop(&mut self) {
            let _test_hook = panic::take_hook();
            if let Some(original) = self.0.take() {
                panic::set_hook(original);
            }
        }
    }

    fn hook_address(hook: &TestPanicHook) -> *const () {
        hook as *const TestPanicHook as *const ()
    }

    #[derive(Default)]
    struct State {
        enters: usize,
        restores: usize,
        fail_enter: Option<usize>,
        fail_restore: Option<usize>,
    }
    #[derive(Clone)]
    struct FakeOps(Arc<Mutex<State>>);
    impl TerminalOps for FakeOps {
        fn enter(&mut self, _: bool) -> io::Result<()> {
            let mut state = self.0.lock().unwrap();
            state.enters += 1;
            if state.fail_enter == Some(state.enters) {
                return Err(io::Error::other("partial enter failure"));
            }
            Ok(())
        }
        fn restore(&mut self, _: bool) -> io::Result<()> {
            let mut state = self.0.lock().unwrap();
            state.restores += 1;
            if state.fail_restore == Some(state.restores) {
                return Err(io::Error::other("restore failure"));
            }
            Ok(())
        }
    }

    #[test]
    fn restore_is_idempotent_and_suspend_reenters() {
        let state = Arc::new(Mutex::new(State::default()));
        let mut session = TerminalSession::start(FakeOps(Arc::clone(&state)), true).unwrap();
        session.restore().unwrap();
        session.restore().unwrap();
        {
            let state = state.lock().unwrap();
            assert_eq!((state.enters, state.restores), (1, 1));
        }
        session.suspend(|| ()).unwrap();
        drop(session);
        let state = state.lock().unwrap();
        assert_eq!((state.enters, state.restores), (2, 2));
    }

    #[test]
    fn start_compensates_a_partial_enter_failure() {
        let state = Arc::new(Mutex::new(State {
            fail_enter: Some(1),
            ..State::default()
        }));

        let result = TerminalSession::start(FakeOps(Arc::clone(&state)), true);

        assert!(result.is_err());
        let state = state.lock().unwrap();
        assert_eq!((state.enters, state.restores), (1, 1));
    }

    #[test]
    fn suspend_compensates_a_partial_reentry_failure() {
        let state = Arc::new(Mutex::new(State {
            fail_enter: Some(2),
            ..State::default()
        }));
        let mut session = TerminalSession::start(FakeOps(Arc::clone(&state)), false).unwrap();

        let result = session.suspend(|| 42);

        assert!(result.is_err());
        assert!(!session.active);
        drop(session);
        let state = state.lock().unwrap();
        assert_eq!((state.enters, state.restores), (2, 2));
    }

    #[test]
    fn failed_restore_remains_retryable() {
        let state = Arc::new(Mutex::new(State {
            fail_restore: Some(1),
            ..State::default()
        }));
        let mut session = TerminalSession::start(FakeOps(Arc::clone(&state)), false).unwrap();

        assert!(session.restore().is_err());
        assert!(session.active);
        session.restore().unwrap();
        drop(session);

        let state = state.lock().unwrap();
        assert_eq!((state.enters, state.restores), (1, 2));
    }

    #[test]
    fn unwinding_restores_the_terminal_through_drop() {
        let state = Arc::new(Mutex::new(State::default()));
        let unwind_state = Arc::clone(&state);

        let result = panic::catch_unwind(AssertUnwindSafe(move || {
            let _session = TerminalSession::start(FakeOps(unwind_state), false).unwrap();
            panic!("exercise terminal-session unwinding");
        }));

        assert!(result.is_err());
        let state = state.lock().unwrap();
        assert_eq!((state.enters, state.restores), (1, 1));
    }

    #[test]
    fn session_preserves_the_host_panic_hook_identity() {
        let _lock = HOOK_TEST_LOCK.lock().unwrap();
        let original = OriginalHook(Some(panic::take_hook()));
        let host_hook: Box<TestPanicHook> = Box::new(|_| {});
        let expected = hook_address(host_hook.as_ref());
        panic::set_hook(host_hook);

        let state = Arc::new(Mutex::new(State::default()));
        drop(TerminalSession::start(FakeOps(state), false).unwrap());

        let installed = panic::take_hook();
        let actual = hook_address(installed.as_ref());
        panic::set_hook(installed);
        assert_eq!(actual, expected);
        drop(original);
    }
}
