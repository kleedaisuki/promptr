//! Promptr 的可复用库入口。 / Reusable library entry point for Promptr.
//!
//! 所有宿主共享同一解析、编译、事务与解释语义；本模块只暴露稳定门面。
//! All hosts share one parsing, compilation, transaction, and interpretation path.

pub mod application;
pub mod diagnostic;
pub mod domain;
pub mod infrastructure;
pub mod language;

pub use application::{InvocationPolicy, Promptr, PromptrOptions, Value};
pub use diagnostic::{Diagnostic, DiagnosticCategory, Severity, SourceSpan};

