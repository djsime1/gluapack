#![cfg_attr(all(debug_assertions, feature = "nightly"), feature(backtrace))]

#[macro_use]
extern crate lazy_static;

#[macro_use]
mod util;

mod consts;
pub(crate) use consts::*;
pub use consts::MAX_LUA_SIZE;

mod load_order;

mod config;
pub use config::{GlobPattern, Config};
pub use util::glob;

mod pack;
pub use pack::PackingError;

mod unpack;
pub use unpack::UnpackingError;

mod ship;
pub use ship::*;

pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");

use std::{future::Future, path::PathBuf, time::Duration};

/// Packs an addon.
///
/// If a config is provided, it will override the addon's `gluapack.json` config.
pub fn pack<P: ToOwned<Owned = PathBuf>>(dir: P, out_dir: Option<P>, no_copy: bool, config: Option<Config>) -> impl Future<Output = Result<(usize, usize, Duration), PackingError>> {
	pack::Packer::pack(dir.to_owned(), out_dir.map(|x| x.to_owned()), no_copy, true, config)
}

/// Unpacks an addon.
pub fn unpack<P: ToOwned<Owned = PathBuf>>(dir: P, out_dir: Option<P>, no_copy: bool) -> impl Future<Output = Result<(usize, usize, Duration), UnpackingError>> {
	unpack::Unpacker::unpack(dir.to_owned(), out_dir.map(|x| x.to_owned()), no_copy, true)
}