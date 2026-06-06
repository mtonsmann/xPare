//! Individual transformation operations.
//!
//! Each operation is a pure function over text (`&str -> String`, plus parameters
//! where needed). The [`crate::pipeline`] dispatches [`crate::Operation`] values to
//! these. Keeping them as free functions (rather than a trait object soup) keeps
//! the core simple, allocation-obvious, and trivially fuzzable in isolation.
//!
//! Function *signatures* in this module are part of the frozen interface contract:
//! the pipeline and the fuzz targets call them by name. Implementations may change
//! freely as long as they stay pure, panic-free, and deterministic.

pub mod case;
pub mod defang;
pub mod html;
pub mod html_to_markdown;
pub(crate) mod indicators;
pub mod lines;
pub mod markdown;
pub mod mask;
pub mod urls;
pub mod whitespace;
