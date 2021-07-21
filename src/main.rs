use std::path::PathBuf;
#[cfg(test)]
mod tests;

mod gluapack;
mod config;
use gluapack::*;

use crate::config::{Config, GlobPattern};

#[derive(Debug, thiserror::Error)]
pub enum PackingError {
	#[error("IO error: {0}")]
	IoError(std::io::Error),

	#[error("gluapack.json error: {0}")]
	ConfigError(serde_json::Error),

	#[error("Realm conflict! This file is included in multiple realms: {0}\nPlease tinker your config and resolve the realm conflicts.")]
	RealmConflict(String),
}
impl From<std::io::Error> for PackingError {
	fn from(error: std::io::Error) -> Self {
		PackingError::IoError(error)
	}
}
impl From<glob::GlobError> for PackingError {
	fn from(error: glob::GlobError) -> Self {
		PackingError::IoError(error.into_error())
	}
}
impl From<serde_json::Error> for PackingError {
	fn from(error: serde_json::Error) -> Self {
		PackingError::ConfigError(error)
	}
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
	let mut args = std::env::args().skip(1);

	let dir = match args.next() {
		Some(dir) => {
			let mut path = PathBuf::from(dir);
			println!("Addon Path: {}", path.display());
			path.push("lua");
			path
		},
		None => {
			eprintln!("Please provide a path to the directory of the addon you want to pack (first argument)");
			std::process::exit(1);
		}
	};

	if !dir.is_dir() {
		eprintln!("No directory was found at this path, or lua/ wasn't found in this addon!");
		std::process::exit(1);
	}

	let mut conf = {
		let conf_path = dir.parent().unwrap().join("gluapack.json");
		if conf_path.is_file() {
			match Config::read(conf_path) {
				Ok(conf) => conf,
				Err(error) => {
					eprintln!("{}", error);
					std::process::exit(1);
				}
			}
		} else {
			println!("WARNING: Couldn't find gluapack.json in your addon. Using the default config.");
			Config::default()
		}
	};

	if conf.entry_cl.is_empty() && conf.entry_sh.is_empty() && conf.entry_sv.is_empty() {
		println!("WARNING: You have not specified any entry file patterns in your config. gluapack will do nothing after unpacking your addon.");
	}

	conf.include_sh.extend_from_slice(&conf.entry_sh);
	conf.include_sv.extend_from_slice(&conf.entry_sv);
	conf.include_cl.extend_from_slice(&conf.entry_cl);

	println!();

	// Make sure we exclude any previous gluapack files
	conf.exclude.push(GlobPattern::new("gluapack/*/*"));
	conf.exclude.push(GlobPattern::new("autorun/*_gluapack_*.lua"));

	let result = gluapack(conf, dir).await;

	println!();
	match result {
		Ok((unpacked_files, packed_files)) => {
			let pct_change = (((unpacked_files as f64) - (packed_files as f64)) / (unpacked_files as f64)) * 100.;
			let sign = if pct_change == 0. { "" } else if pct_change > 0. { "-" } else { "+" };
			println!("Successfully packed {} file(s) -> {} files ({}{:.2}%)", unpacked_files, packed_files, sign, pct_change.abs())
		},
		Err(error) => eprintln!("Packing error: {:#?}", error)
	}
}
