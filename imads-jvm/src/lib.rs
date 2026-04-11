//! JVM bindings via Foreign Function & Memory API (JDK 22+).
//!
//! This crate produces a `cdylib` that re-exports the `imads-ffi` C ABI symbols.
//! On the JVM side, `java.lang.foreign` (FFM/Panama) is used to load and
//! call these functions directly — no JNI adapter code is needed.
//!
//! Use `jextract` on the generated C header to produce Java bindings automatically.

// Re-export all public items from imads-ffi so the linker includes them.
pub use imads_ffi::*;
