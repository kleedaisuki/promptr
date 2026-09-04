//! Promptr 可执行程序入口。 / Promptr executable entry point.
//!
//! <!-- @brief Promptr 可执行程序入口。 / Promptr executable entry point. -->

use std::process::ExitCode;

/// 启动命令行宿主并返回稳定退出码。 / Start the CLI host and return a stable exit code.
///
/// # Returns
///
/// CLI 退出码。 / CLI exit code.
///
/// <!-- @brief 启动命令行宿主并返回稳定退出码。 / Start the CLI host and return a stable exit code. -->
/// <!-- @return CLI 退出码。 / CLI exit code. -->
fn main() -> ExitCode {
    promptr::interface::cli::main_entry()
}
