// SPDX-License-Identifier: AGPL-3.0-or-later

//! File opening and file comparison for products that decide for themselves which files
//! they admit. It builds on `rustix`; the process adapters of Pale and Gazette build on
//! it to open and recognise the executables they run. Each caller keeps its own rules
//! for which paths and file types it accepts.
//!
//! Only Unix is built; other platforms have no items yet.

#[cfg(unix)]
pub mod unix;
