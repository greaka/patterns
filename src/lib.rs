//! # Pattern matching library
//!
//! Allows you to search for a pattern within data via an iterator interface.
//! This library uses the core::simd abstraction and is fully no_std, no alloc.
//! Additionally, all panics are documented and are limited to pattern creation.
//!
//! ## Usage
//!
//! ```rs
//! use patterns::Pattern;
//!
//! let data = [0_u8; 1_000_00];
//! // Allows . and ? as wildcard.
//! // Any number of wildcard characters between spaces is considered a wildcard byte.
//! let pattern: Pattern = "01 02 00 ? 59 ff".parse().unwrap();
//! let mut iterator = pattern.matches(&data);
//!
//! for _found in iterator {
//!     // use _found
//! }
//! ```
//!
//! More advanced use cases may also specify a target alignment required to
//! match, or the LANE size with which to search:
//!
//! ```rs
//! use patterns::Pattern;
//!
//! static PATTERN: Pattern<4, 64> = Pattern::new("00 01 02 . ff");
//! ```
//!
//! ## Limitations
//!
//! - The maximum amount of bytes supported inside a pattern are determined by
//!   the chosen 2nd const parameter (default 64)
//! - Target alignment of the pattern to search for must be less or equal to
//!   that 2nd const parameter
//! - The pointer of data to search through must adhere to these bounds:
//!     - `data.as_ptr() - 64 > `[`usize::MIN`]
//!     - `data.as_ptr() + data.len() + 64 < `[`usize::MAX`]
//!
//! In practice, it's impossible to be outside of these bounds when using an OS.

// todos
// optimize pattern.len() <= alignment
// explore getting rid of pattern.length

#![feature(portable_simd)]
#![no_std]
// untested on big endian
#![cfg(target_endian = "little")]

pub use crate::{
    pattern::{ParsePatternError, Pattern},
    scanner::Scanner,
};

mod const_utils;
mod masks;
mod pattern;
mod scanner;

/// The type that holds a bit for each byte in `BYTES`
type BytesMask = u64;

const V128: usize = 16;
const V256: usize = 32;
const V512: usize = 64;
const VUNKNOWN: usize = V512;

/// Provides a constant optimizing `BYTES` (see [`Pattern`]) to target cpu simd
/// width. This is a best-effort, defaulting to maximum supported bytes.
///
/// Note that `BYTES` also determines maximum pattern length.
///
/// There was no benchmark performed comparing different values of BYTES to
/// assumed optimal platform target width.
pub const OPTIMAL_BYTES: usize = default_vector_target_width();

const fn default_vector_target_width() -> usize {
    if (cfg!(target_arch = "arm") || cfg!(target_arch = "aarch64")) && cfg!(target_feature = "neon")
    {
        return V128;
    }
    if cfg!(target_arch = "hexagon") {
        if cfg!(target_feature = "hvx-length128b") {
            // 1024 bits
            return V512;
        }
        if cfg!(target_feature = "hvx") {
            return V512;
        }
    }
    if cfg!(target_arch = "mips") && cfg!(target_feature = "msa") {
        return V128;
    }
    if cfg!(target_arch = "powerpc")
        && (cfg!(target_feature = "vsx") || cfg!(target_feature = "altivec"))
    {
        return V128;
    }
    if (cfg!(target_arch = "riscv32") || cfg!(target_arch = "riscv64"))
        && cfg!(target_feature = "v")
    {
        return V128;
    }
    if (cfg!(target_arch = "wasm32") || cfg!(target_arch = "wasm64"))
        && cfg!(target_feature = "simd128")
    {
        return V128;
    }
    if cfg!(target_arch = "x86") {
        if cfg!(target_feature = "avx512f") {
            return V512;
        }
        if cfg!(target_feature = "avx2") {
            return V256;
        }
        if cfg!(target_feature = "sse2") {
            return V128;
        }
    }
    VUNKNOWN
}
