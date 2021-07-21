mod pack;
mod config;

use pack::Packer;
use config::{Config, GlobPattern};

fn canonicalize(path: &std::path::PathBuf) -> impl std::fmt::Display {
	#[cfg(target_os = "windows")] {
		let path = path.canonicalize().as_ref().unwrap_or_else(|_| &path).to_string_lossy().into_owned();
		path.clone().strip_prefix(r#"\\?\"#).map(|str| str.to_owned()).unwrap_or(path)
	}

	#[cfg(not(target_os = "windows"))]
	path.canonicalize().as_ref().unwrap_or_else(|_| &path).display()
}

#[derive(Debug, thiserror::Error)]
pub enum PackingError {
	#[error("IO error: {0}")]
	IoError(std::io::Error),

	#[error("gluapack.json error: {0}")]
	ConfigError(serde_json::Error),

	#[error("Realm conflict! This file is included in multiple realms: {0}\nPlease tinker your config and resolve the realm conflicts.")]
	RealmConflict(String),
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
	use std::path::PathBuf;

	let mut args = std::env::args().skip(1);

	let dir = match args.next() {
		Some(dir) => PathBuf::from(dir),
		None => {
			eprintln!("Please provide a path to the directory of the addon you want to pack (first argument)");
			return;
		}
	};

	if !dir.is_dir() {
		eprintln!("No directory was found at this path, or lua/ wasn't found in this addon!");
		return;
	}

	let mut conf = {
		let conf_path = dir.join("gluapack.json");
		if conf_path.is_file() {
			match Config::read(conf_path) {
				Ok(conf) => conf,
				Err(error) => {
					eprintln!("{}", error);
					return;
				}
			}
		} else {
			println!("WARNING: Couldn't find gluapack.json in your addon. Using the default config.");
			Config::default()
		}
	};

	conf.dump_json();

	println!("Addon Path: {}", canonicalize(&dir));

	let out_dir = if true {
		let out_dir = match conf.out {
			Some(ref out) => {
				let out = PathBuf::from(out);
				if out.is_absolute() {
					if out == dir {
						eprintln!("ERROR: Output directory cannot be the same as the addon directory!");
						return;
					}
					out
				} else {
					dir.parent().unwrap_or_else(|| dir.as_path()).join(out)
				}
			},
			None => dir.parent().unwrap_or_else(|| dir.as_path()).join(&format!("{}-gluapack", dir.file_name().unwrap().to_string_lossy()))
		};

		if out_dir.is_dir() {
			tokio::fs::remove_dir_all(&out_dir).await.expect("Failed to delete existing output directory");
		} else if out_dir.is_file() {
			tokio::fs::remove_file(&out_dir).await.expect("Failed to delete existing output directory");
		}

		let result = tokio::fs::create_dir_all(&out_dir).await;

		println!("Output Path: {}", canonicalize(&out_dir));

		result.expect("Failed to create output directory");

		Some(out_dir)
	} else {
		println!("Output Path: In-place");
		None
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

	let result = Packer::pack(conf, dir, out_dir).await;

	println!();
	match result {
		Ok((unpacked_files, packed_files, elapsed)) => {
			let pct_change = (((unpacked_files as f64) - (packed_files as f64)) / (unpacked_files as f64)) * 100.;
			let sign = if pct_change == 0. { "" } else if pct_change > 0. { "-" } else { "+" };
			println!("Successfully packed {} file(s) -> {} files ({}{:.2}%)", unpacked_files, packed_files, sign, pct_change.abs());
			println!("Took {:?}", elapsed);
		},
		Err(error) => eprintln!("ERROR: {}", error)
	}
}
