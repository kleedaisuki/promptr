//! Crossterm 终端生命周期与幂等恢复。 / Crossterm terminal lifecycle and idempotent restoration.

use std::{
    io::{self, Write},
    panic,
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
};

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
pub trait TerminalOps: Send + 'static {
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

type PanicCallback = dyn Fn(&panic::PanicHookInfo<'_>) + Sync + Send + 'static;
static PANIC_HOOK_LOCK: Mutex<()> = Mutex::new(());

struct PanicHookGuard {
    previous: Arc<Mutex<Option<Box<PanicCallback>>>>,
}

impl Drop for PanicHookGuard {
    fn drop(&mut self) {
        let _installed = panic::take_hook();
        if let Some(previous) = self
            .previous
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            panic::set_hook(previous);
        }
    }
}

/// RAII 终端会话，恢复操作可安全重复调用。 / RAII terminal session with safely repeatable restoration.
///
/// <!-- @brief RAII 终端会话，恢复操作可安全重复调用。 / RAII terminal session with safely repeatable restoration. -->
pub struct TerminalSession<O: TerminalOps = CrosstermOps> {
    ops: Arc<Mutex<O>>,
    active: Arc<AtomicBool>,
    mouse: bool,
    hook: Option<PanicHookGuard>,
    hook_lock: Option<std::sync::MutexGuard<'static, ()>>,
}

impl<O: TerminalOps> TerminalSession<O> {
    /// 进入终端；失败时补偿任何已发生的部分副作用。 / Enter the terminal, compensating any partial side effects on failure.
    ///
    /// # Arguments / 参数
    ///
    /// - `ops` — 生命周期后端。 / Lifecycle backend.
    /// - `active` — 会话活动状态。 / Session activity state.
    /// - `mouse` — 是否启用鼠标捕获。 / Whether mouse capture is enabled.
    ///
    /// # Returns / 返回值
    ///
    /// 成功时返回空值，失败时返回原始进入错误。 / Unit on success, or the original entry error on failure.
    ///
    /// <!-- @brief 进入终端；失败时补偿任何已发生的部分副作用。 / Enter the terminal, compensating any partial side effects on failure. -->
    /// <!-- @param ops 生命周期后端。 / Lifecycle backend. -->
    /// <!-- @param active 会话活动状态。 / Session activity state. -->
    /// <!-- @param mouse 是否启用鼠标捕获。 / Whether mouse capture is enabled. -->
    /// <!-- @return 成功时返回空值，失败时返回原始进入错误。 / Unit on success, or the original entry error on failure. -->
    fn enter(ops: &Arc<Mutex<O>>, active: &AtomicBool, mouse: bool) -> io::Result<()> {
        active.store(false, Ordering::Release);
        let mut ops = ops.lock().unwrap_or_else(|error| error.into_inner());
        match ops.enter(mouse) {
            Ok(()) => {
                active.store(true, Ordering::Release);
                Ok(())
            }
            Err(error) => {
                let _ = ops.restore(mouse);
                Err(error)
            }
        }
    }

    /// 进入终端并安装链式 panic 恢复钩子。 / Enter the terminal and install a chained panic-restoration hook.
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
    /// <!-- @brief 进入终端并安装链式 panic 恢复钩子。 / Enter the terminal and install a chained panic-restoration hook. -->
    /// <!-- @param ops 生命周期后端。 / Lifecycle backend. -->
    /// <!-- @param mouse 是否启用鼠标捕获。 / Whether mouse capture is enabled. -->
    /// <!-- @return 活跃会话或底层 I/O 错误。 / Active session or underlying I/O error. -->
    pub fn start(ops: O, mouse: bool) -> io::Result<Self> {
        let hook_lock = PANIC_HOOK_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let ops = Arc::new(Mutex::new(ops));
        let active = Arc::new(AtomicBool::new(false));
        Self::enter(&ops, &active, mouse)?;
        let previous = Arc::new(Mutex::new(Some(panic::take_hook())));
        let hook_ops = Arc::clone(&ops);
        let hook_active = Arc::clone(&active);
        let hook_previous = Arc::clone(&previous);
        panic::set_hook(Box::new(move |info| {
            if hook_active.swap(false, Ordering::AcqRel) {
                match hook_ops.try_lock() {
                    Ok(mut ops) => {
                        let _ = ops.restore(mouse);
                    }
                    Err(TryLockError::Poisoned(error)) => {
                        let _ = error.into_inner().restore(mouse);
                    }
                    Err(TryLockError::WouldBlock) => {
                        // 当前线程可能正因后端操作 panic 而持锁；让展开阶段的 Drop 重试。
                        // The current thread may hold the lock while the backend panics; let Drop retry during unwinding.
                        hook_active.store(true, Ordering::Release);
                    }
                }
            }
            if let Some(previous) = hook_previous
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
            {
                previous(info);
            }
        }));
        Ok(Self {
            ops,
            active,
            mouse,
            hook: Some(PanicHookGuard { previous }),
            hook_lock: Some(hook_lock),
        })
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
        if !self.active.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        self.ops
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .restore(self.mouse)
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
        Self::enter(&self.ops, &self.active, self.mouse)?;
        Ok(result)
    }
}

impl<O: TerminalOps> Drop for TerminalSession<O> {
    fn drop(&mut self) {
        let _ = self.restore();
        self.hook.take();
        self.hook_lock.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct State {
        enters: usize,
        restores: usize,
        fail_enter: Option<usize>,
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
            self.0.lock().unwrap().restores += 1;
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
        assert!(!session.active.load(Ordering::Acquire));
        drop(session);
        let state = state.lock().unwrap();
        assert_eq!((state.enters, state.restores), (2, 2));
    }
}
