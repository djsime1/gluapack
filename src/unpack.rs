// FIXME for new format
// TODO test if UnpackingStatistics is correct

use std::{collections::HashSet, ffi::OsString, io::{BufRead, Seek}, path::{Path, PathBuf}, time::Duration};

use futures_util::future;

use crate::{config::GlobPattern, MAX_LUA_SIZE, TERMINATOR_HACK, MEM_PREALLOCATE_MAX, util};

lazy_static! {
	static ref LOADER_GLOB: GlobPattern = GlobPattern::new("autorun/*_gluapack_*.lua");
	static ref CHUNK_FILE_GLOB: GlobPattern = GlobPattern::new("gluapack/*/*.lua");
	static ref CHUNK_DIR_GLOB: GlobPattern = GlobPattern::new("gluapack/*");
	static ref LUA_FOLDER_OS_STRING: OsString = OsString::from("lua");
	static ref GLUAPACK_DIR: PathBuf = PathBuf::from("gluapack");
}

pub struct UnpackingStatistics {
	pub total_unpacked_files: usize,
	pub total_unpacked_size: usize,
	pub total_packed_files: usize,
	pub total_packed_size: usize,
	pub elapsed: Duration,
}
impl UnpackingStatistics {
	pub fn files(&self) -> String {
		let pct_change = (((self.total_unpacked_files as f64) - (self.total_packed_files as f64)) / (self.total_unpacked_files as f64)) * 100.;
		let sign = if pct_change == 0. { "" } else if pct_change > 0. { "-" } else { "+" };

		format!("{} files -> {} file(s) ({}{:.2}%)", self.total_unpacked_files, self.total_packed_files, sign, pct_change.abs())
	}

	pub fn size(&self) -> String {
		let pct_change = (((self.total_unpacked_size as f64) - (self.total_packed_size as f64)) / (self.total_unpacked_size as f64)) * 100.;
		let sign = if pct_change == 0. { "" } else if pct_change > 0. { "-" } else { "+" };

		format!("{:.2} -> {:.2} ({}{:.2}%)", util::file_size(self.total_unpacked_size), util::file_size(self.total_packed_size), sign, pct_change.abs())
	}

	pub fn elapsed(&self) -> String {
		format!("{:?}", self.elapsed)
	}
}

pub(crate) struct Unpacker {
	pub(crate) dir: PathBuf,
	pub(crate) out_dir: PathBuf
}
impl Unpacker {
	pub(crate) async fn unpack(dir: PathBuf, out_dir: Option<PathBuf>, no_copy: bool, quiet: bool) -> Result<UnpackingStatistics, UnpackingError> {
		quietln!(quiet, "Addon Path: {}", util::canonicalize(&dir).display());

		let out_dir = if let Some(out_dir) = out_dir {
			util::prepare_output_dir(quiet, &out_dir).await;
			out_dir
		} else {
			quietln!(quiet, "Output Path: In-place");
			dir.clone()
		};

		quietln!(quiet);

		// Start unpacking
		let mut unpacker = Unpacker {
			out_dir,
			dir
		};

		let started = std::time::Instant::now();

		let (sv_packed_file, cl_chunk_files, sh_chunk_files) = if no_copy {
			quietln!(quiet, "Discovering chunk files...");

			let (mut cl_chunk_files, mut sh_chunk_files) = (vec![], vec![]);

			for entry in util::glob(unpacker.dir.join("lua/gluapack/*/*.lua").to_string_lossy()).unwrap().filter_map(|result| result.ok()) {
				let file_name = entry.file_name().as_ref().unwrap().to_string_lossy();
				if file_name.ends_with(".sh.lua") {
					sh_chunk_files.push(entry.clone());
				} else if file_name.ends_with(".cl.lua") {
					cl_chunk_files.push(entry.clone());
				}
			}

			(
				util::glob(unpacker.dir.join("lua/gluapack/autorun/*_gluapack_*.lua").to_string_lossy()).unwrap().find_map(|result| result.ok()),
				cl_chunk_files,
				sh_chunk_files
			)
		} else {
			quietln!(quiet, "Copying addon to output directory...");
			let dir = unpacker.dir.clone();
			let out_dir = unpacker.out_dir.clone();
			tokio::task::spawn_blocking(move || Unpacker::copy_addon(dir, out_dir)).await.expect("Failed to join thread")?
		};

		unpacker.out_dir.push("lua");
		unpacker.dir.push("lua");

		let total_packed_files = cl_chunk_files.len() + sh_chunk_files.len() + if sv_packed_file.is_some() { 1 } else { 0 };
		let mut total_unpacked_files = 0;

		let (out_dir_1, out_dir_2, out_dir_3) = (unpacker.out_dir.to_owned(), unpacker.out_dir.to_owned(), unpacker.out_dir.to_owned());

		let ((sv_n, sv_unpacked_size, sv_packed_size), (cl_n, cl_unpacked_size, cl_packed_size), (sh_n, sh_unpacked_size, sh_packed_size)) = future::try_join3(
			async move {
				if let Some(sv_packed_file) = sv_packed_file {
					quietln!(quiet, "Unpacking serverside files...");
					// Parse the serverside pack file and unpack it!
					Unpacker::parse_sv_packed_file(out_dir_1, sv_packed_file).await
				} else {
					Ok((0, 0, 0))
				}
			},
			async move {
				quietln!(quiet, "Unpacking clientside files...");
				Unpacker::parse_packed_files(out_dir_2, cl_chunk_files).await
			},
			async move {
				quietln!(quiet, "Unpacking shared files...");
				Unpacker::parse_packed_files(out_dir_3, sh_chunk_files).await
			}
		).await?;

		total_unpacked_files += sv_n + cl_n + sh_n;

		Ok(UnpackingStatistics {
			total_unpacked_files,
			total_packed_files,
			total_packed_size: sv_packed_size + sh_packed_size + cl_packed_size,
			total_unpacked_size: sv_unpacked_size + sh_unpacked_size + cl_unpacked_size,
			elapsed: started.elapsed()
		})
	}

	fn copy_addon(dir: PathBuf, out_dir: PathBuf) -> Result<(Option<PathBuf>, Vec<PathBuf>, Vec<PathBuf>), std::io::Error> {
		std::fs::create_dir_all(&out_dir)?;

		fn copy_addon(visited_symlinks: &mut HashSet<PathBuf>, lua_folder: &Path, from: PathBuf, to: PathBuf, sv_packed_file: &mut Option<PathBuf>, cl_chunk_files: &mut Vec<PathBuf>, sh_chunk_files: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
			#[cfg(target_os = "windows")]
			const FILE_ATTRIBUTE_HIDDEN: u32 = 0x02;

			for dir_entry in from.read_dir()? {
				let dir_entry = dir_entry?;

				let entry;
				if dir_entry.file_type()?.is_symlink() {
					let path = dir_entry.path();
					if visited_symlinks.insert(path.clone()) {
						entry = path.read_link()?;
					} else {
						continue;
					}
				} else {
					entry = dir_entry.path();
				}

				let file_name = entry.file_name().as_ref().unwrap().to_string_lossy();

				// If we're in <dir>/lua
				let skip_copy = if let Ok(lua_relative) = entry.strip_prefix(lua_folder) {
					// Skip gluapack files
					if entry.is_dir() {
						lua_relative == &*GLUAPACK_DIR || CHUNK_DIR_GLOB.matches_path(lua_relative)
					} else {
						if LOADER_GLOB.matches_path(lua_relative) {
							continue;
						} else if CHUNK_FILE_GLOB.matches_path(lua_relative) {
							// Remember chunk files for later
							if &file_name == "gluapack.sv.lua" {
								debug_assert!(sv_packed_file.is_none());
								*sv_packed_file = Some(entry.clone());
							} else if file_name.ends_with(".sh.lua") {
								sh_chunk_files.push(entry.clone());
							} else if file_name.ends_with(".cl.lua") {
								cl_chunk_files.push(entry.clone());
							}
							continue;
						} else {
							false
						}
					}
				} else {
					false
				};

				if file_name.starts_with(".") || file_name == "gluapack.json" {
					// Skip hidden files/dirs and gluapack.json
					continue;
				}

				#[cfg(target_os = "windows")]
				if std::os::windows::fs::MetadataExt::file_attributes(&entry.metadata()?) & FILE_ATTRIBUTE_HIDDEN != 0 {
					// Skip hidden files (Windows)
					continue;
				}

				let file_name = file_name.into_owned();

				if entry.is_dir() {
					let dir = to.join(&file_name);
					if !skip_copy {
						std::fs::create_dir_all(&dir)?;
					}
					copy_addon(visited_symlinks, lua_folder, entry, dir, sv_packed_file, cl_chunk_files, sh_chunk_files)?;
				} else if entry.is_file() && !skip_copy {
					std::fs::copy(entry, to.join(&file_name))?;
				}
			}
			Ok(())
		}

		let mut sv_packed_file = None;
		let mut cl_chunk_files = vec![];
		let mut sh_chunk_files = vec![];

		let mut visited_symlinks = HashSet::new();
		copy_addon(&mut visited_symlinks, &dir.join("lua"), dir, out_dir, &mut sv_packed_file, &mut cl_chunk_files, &mut sh_chunk_files)?;

		Ok((sv_packed_file, cl_chunk_files, sh_chunk_files))
	}

	async fn parse_sv_packed_file(out_dir: PathBuf, sv_packed_file: PathBuf) -> Result<(usize, usize, usize), UnpackingError> {
		use std::{fs::File, io::{BufReader, Read}};

		let mut entries = 0;

		let f = File::open(sv_packed_file)?;

		let total_size = f.metadata().map(|metadata| metadata.len() as usize).unwrap_or_default();

		let mut f = BufReader::new(f);
		fn read_entry(out_dir: &PathBuf, f: &mut BufReader<File>) -> Result<bool, std::io::Error> {
			let mut path = Vec::with_capacity(255);
			f.read_until(0, &mut path)?;

			if path.is_empty() {
				return Ok(true);
			}

			let mut len = [0u8; 4];
			f.read_exact(&mut len)?;
			let len = u32::from_le_bytes(len);

			let path = out_dir.join(String::from_utf8_lossy(&path[0..path.len()-1]).as_ref());

			if let Some(parent) = path.parent() {
				std::fs::create_dir_all(parent)?;
			}

			let mut out = File::create(path)?;
			std::io::copy(&mut f.by_ref().take(len as u64), &mut out)?;

			Ok(false)
		}
		loop {
			match read_entry(&out_dir, &mut f) {
				Ok(true) => break,
				Ok(false) => entries += 1,
				Err(error) => if let std::io::ErrorKind::UnexpectedEof = error.kind() {
					break;
				} else {
					return Err(error!(UnpackingError::IoError(error)));
				},
			}
		}

		Ok((entries, total_size, total_size))
	}

	async fn parse_packed_files(out_dir: PathBuf, packed_files: Vec<PathBuf>) -> Result<(usize, usize, usize), UnpackingError> {
		use std::{fs::File, io::{SeekFrom, BufReader, Read, Cursor}};

		let mut entries = 0;

		fn read_commented_file<P: AsRef<std::path::Path>>(packed_file: P) -> Result<Vec<u8>, std::io::Error> {
			let mut buf = Vec::with_capacity(packed_file.as_ref().metadata()?.len() as usize);
			let mut f = BufReader::new(File::open(packed_file)?);
			loop {
				let mut line = String::new();
				f.seek(SeekFrom::Current(2))?;
				if f.read_line(&mut line)? == 0 {
					break;
				}
				buf.extend_from_slice(&line.as_bytes())
			}
			Ok(buf)
		}

		let mut superchunk = Vec::with_capacity((MAX_LUA_SIZE * packed_files.len()).min(MEM_PREALLOCATE_MAX));
		for packed_file in packed_files {
			superchunk.extend_from_slice(&read_commented_file(packed_file)?);
		}

		fn read_entry(out_dir: &PathBuf, f: &mut std::io::Cursor<Vec<u8>>, total_unpacked_size: &mut usize) -> Result<bool, UnpackingError> {
			let mut path = Vec::with_capacity(255);
			f.read_until(TERMINATOR_HACK, &mut path)?;

			if path.is_empty() {
				return Ok(true);
			}

			let mut len = Vec::with_capacity(16);
			f.read_until(TERMINATOR_HACK, &mut len)?;

			let len = u32::from_str_radix(std::str::from_utf8(&len[0..len.len()-1])?, 16)?;

			*total_unpacked_size += len as usize;

			let path = out_dir.join(String::from_utf8_lossy(&path[0..path.len()-1]).as_ref());

			if let Some(parent) = path.parent() {
				std::fs::create_dir_all(parent)?;
			}

			let mut out = File::create(path)?;
			std::io::copy(&mut f.by_ref().take(len as u64), &mut out)?;

			Ok(false)
		}

		let mut total_unpacked_size = 0;
		let total_packed_size = superchunk.len();

		let mut f = Cursor::new(superchunk);
		loop {
			match read_entry(&out_dir, &mut f, &mut total_unpacked_size) {
				Ok(true) => break,
				Ok(false) => entries += 1,
				Err(UnpackingError::IoError { error, .. }) => if let std::io::ErrorKind::UnexpectedEof = error.kind() {
					break;
				} else {
					return Err(error!(UnpackingError::IoError(error)));
				}
				Err(error) => return Err(error),
			}
		}

		Ok((entries, total_unpacked_size, total_packed_size))
	}
}

#[derive(Debug, thiserror::Error)]
pub enum UnpackingError {
	#[error("IO error: {error}")]
	IoError {
		error: std::io::Error,
		#[cfg(all(debug_assertions, feature = "nightly"))]
		backtrace: std::backtrace::Backtrace
	},

	#[error("UTF-8 error: {error}")]
	Utf8Error {
		error: std::str::Utf8Error,
		#[cfg(all(debug_assertions, feature = "nightly"))]
		backtrace: std::backtrace::Backtrace
	},

	#[error("File format error: {error}")]
	ParseIntError {
		error: std::num::ParseIntError,
		#[cfg(all(debug_assertions, feature = "nightly"))]
		backtrace: std::backtrace::Backtrace
	},
}
impl_error!(std::io::Error, UnpackingError::IoError);
impl_error!(std::str::Utf8Error, UnpackingError::Utf8Error);
impl_error!(std::num::ParseIntError, UnpackingError::ParseIntError);
