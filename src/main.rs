#![cfg_attr(all(debug_assertions, feature = "nightly"), feature(backtrace))]

#[macro_use]
extern crate lazy_static;

#[macro_use]
mod util;

mod consts;
pub(crate) use consts::*;

mod loadorder;

mod entities;
pub use entities::extract_entity;

mod config;
mod pack;
mod unpack;

use pack::Packer;
use unpack::Unpacker;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
	use clap::*;
	use std::path::PathBuf;

	#[cfg(all(debug_assertions, feature = "nightly"))]
	use std::error::Error;

	let stdin = App::new("gluapack")
		.version(env!("CARGO_PKG_VERSION"))
		.setting(AppSettings::VersionlessSubcommands)
		.setting(AppSettings::SubcommandRequired)
		.author("William Venner <william@venner.io>")
		.about("Packs hundreds of Lua files into just a handful")
		.subcommand(
			App::new("pack")
			.setting(AppSettings::TrailingVarArg)
			.setting(AppSettings::AllowLeadingHyphen)
			.about("Packs an addon")
			.arg(
				Arg::with_name("path")
					.help("Path to addon root (directory containing lua/ folder)")
					.takes_value(true)
					.required(true)
					.index(1)
			)
		)
		.subcommand(
			App::new("unpack")
			.setting(AppSettings::TrailingVarArg)
			.setting(AppSettings::AllowLeadingHyphen)
			.about("Unpacks an addon")
			.arg(
				Arg::with_name("path")
					.help("Path to addon root (directory containing lua/ folder)")
					.takes_value(true)
					.required(true)
					.index(1)
			)
		)
		.arg(
			Arg::with_name("in-place")
				.global(true)
				.help("Modifies the addon in-place, rather than creating a copy of the addon")
				.long("in-place")
				.short("m")
				.alias("mod")
				.alias("modify")
				.multiple(false)
				.conflicts_with("no-copy")
		)
		.arg(
			Arg::with_name("no-copy")
				.global(true)
				.help("Do not create a copy of the addon in the output directory")
				.long("no-copy")
				.short("c")
				.multiple(false)
				.conflicts_with("in-place")
		)
		.arg(
			Arg::with_name("quiet")
				.global(true)
				.help("Silences stdout (does not silence stderr)")
				.long("quiet")
				.short("q")
				.multiple(false)
		)
		.arg(
			Arg::with_name("out")
				.global(true)
				.help("Specifies the name of the output directory. Relative to addon's parent directory. Can be an absolute path.")
				.long("out")
				.short("o")
				.alias("output")
				.takes_value(true)
				.required(false)
				.multiple(false)
		)
		.get_matches();

	macro_rules! addon_path {
		($args:ident) => {{
			let path = PathBuf::from($args.value_of("path").unwrap());
			if !path.join("lua").is_dir() {
				eprintln!("ERROR: Couldn't find an addon at this path containing a lua/ folder.");
				abort!();
			}
			let path = dunce::canonicalize(&path).unwrap_or_else(|_| path);
			path
		}}
	}

	macro_rules! out_path {
		($args:ident, $path:ident, $in_place:ident, $suffix_to:literal, $suffix_from:literal) => {
			if !$in_place {
				Some(match $args.value_of("out") {
					Some(out_dir) => {
						let out_dir = PathBuf::from(out_dir);
						if out_dir.is_absolute() {
							if out_dir == $path {
								eprintln!("ERROR: Output directory cannot be the same as the addon directory!");
								abort!();
							}
							out_dir
						} else {
							$path.parent().unwrap_or_else(|| $path.as_path()).join(out_dir)
						}
					},
					None => {
						let path = $path.file_name().unwrap().to_string_lossy();
						$path.parent().unwrap_or_else(|| $path.as_path()).join(format!(concat!("{}-", $suffix_to), path.strip_suffix(concat!("-", $suffix_from)).unwrap_or_else(|| &path)))
					}
				})
			} else {
				None
			}
		}
	}

	match stdin.subcommand() {
		("pack", Some(args)) => {
			let path = addon_path!(args);
			let in_place = args.is_present("in-place");
			let out_dir = out_path!(args, path, in_place, "packed", "unpacked");
			let no_copy = args.is_present("no-copy");
			let quiet = args.is_present("quiet");

			match (quiet, Packer::pack(path, out_dir, no_copy, quiet, None).await) {
				(true, Ok(_)) => {},
				(false, Ok(stats)) => {
					println!();
					println!("PACKED successfully!");
					println!("{}", stats.files());
					println!("{}", stats.size());
					println!("Took {}", stats.elapsed());
				},
				(_, Err(error)) => {
					if !quiet {
						println!();
					}
					eprintln!("ERROR: {}", error);
					#[cfg(all(feature = "nightly", debug_assertions))]
					eprintln!("{:#?}", error.backtrace());
					abort!();
				},
			}
		},

		("unpack", Some(args)) => {
			let path = addon_path!(args);
			let in_place = args.is_present("in-place");
			let out_dir = out_path!(args, path, in_place, "unpacked", "packed");
			let no_copy = args.is_present("no-copy");
			let quiet = args.is_present("quiet");

			match (quiet, Unpacker::unpack(path, out_dir, no_copy, quiet).await) {
				(true, Ok(_)) => {},
				(false, Ok(stats)) => {
					println!();
					println!("UNPACKED successfully!");
					println!("{}", stats.files());
					println!("{}", stats.size());
					println!("Took {}", stats.elapsed());
				},
				(_, Err(error)) => {
					if !quiet {
						println!();
					}
					eprintln!("ERROR: {}", error);
					#[cfg(all(feature = "nightly", debug_assertions))]
					eprintln!("{:#?}", error.backtrace());
					abort!();
				},
			}
		},

		_ => unreachable!()
	}
}
