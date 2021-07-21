#[macro_use]
extern crate lazy_static;

#[macro_use]
mod util;

mod pack;
mod unpack;
mod config;

use pack::Packer;
use unpack::Unpacker;

/// The maximum size of a chunk.
///
/// This should be 64 KiB as Garry's Mod will not network a Lua file larger than this.
pub const MAX_LUA_SIZE: usize = 65535;
pub const MEM_PREALLOCATE_MAX: usize = 1024 * 1024 * 1024;
pub const TERMINATOR_HACK: u8 = '|' as u8;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
	use clap::*;
	use std::path::PathBuf;

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
			let quiet = args.is_present("quiet");

			match (quiet, Packer::pack(path, out_dir, quiet).await) {
				(true, Ok(_)) => {},
				(false, Ok((unpacked_files, packed_files, elapsed))) => {
					println!();
					let pct_change = (((unpacked_files as f64) - (packed_files as f64)) / (unpacked_files as f64)) * 100.;
					let sign = if pct_change == 0. { "" } else if pct_change > 0. { "-" } else { "+" };
					println!("Successfully PACKED {} file(s) -> {} files ({}{:.2}%)", unpacked_files, packed_files, sign, pct_change.abs());
					println!("Took {:?}", elapsed);
				},
				(_, Err(error)) => {
					if !quiet {
						println!();
					}
					eprintln!("ERROR: {}", error);
					abort!();
				},
			}
		},

		("unpack", Some(args)) => {
			let path = addon_path!(args);
			let in_place = args.is_present("in-place");
			let out_dir = out_path!(args, path, in_place, "unpacked", "packed");
			let quiet = args.is_present("quiet");

			match (quiet, Unpacker::unpack(path, out_dir, quiet).await) {
				(true, Ok(_)) => {},
				(false, Ok((packed_files, unpacked_files, elapsed))) => {
					println!();
					let pct_change = (((unpacked_files as f64) - (packed_files as f64)) / (unpacked_files as f64)) * 100.;
					let sign = if pct_change == 0. { "" } else if pct_change > 0. { "-" } else { "+" };
					println!("Successfully UNPACKED {} files -> {} file(s) ({}{:.2}%)", unpacked_files, packed_files, sign, pct_change.abs());
					println!("Took {:?}", elapsed);
				},
				(_, Err(error)) => {
					if !quiet {
						println!();
					}
					eprintln!("ERROR: {}", error);
					abort!();
				},
			}
		},

		_ => unreachable!()
	}
}
