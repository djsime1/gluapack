use std::{collections::HashSet, ffi::OsString, io::{BufRead, Seek}, path::{Path, PathBuf}, time::Duration};

use crate::{config::GlobPattern, MAX_LUA_SIZE, TERMINATOR_HACK, MEM_PREALLOCATE_MAX, util};

lazy_static! {
	static ref LOADER_GLOB: GlobPattern = GlobPattern::new("autorun/*_gluapack_*.lua");
	static ref CHUNK_FILE_GLOB: GlobPattern = GlobPattern::new("gluapack/*/*.lua");
	static ref CHUNK_DIR_GLOB: GlobPattern = GlobPattern::new("gluapack/*");
	static ref LUA_FOLDER_OS_STRING: OsString = OsString::from("lua");
	static ref GLUAPACK_DIR: PathBuf = PathBuf::from("gluapack");
}

pub struct Unpacker {
	pub dir: PathBuf,
	pub out_dir: PathBuf,
	pub quiet: bool
}
impl Unpacker {
	pub async fn unpack(dir: PathBuf, out_dir: Option<PathBuf>, quiet: bool) -> Result<(usize, usize, Duration), UnpackingError> {
		quietln!(quiet, "Addon Path: {}", util::canonicalize(&dir));

		let (in_place, out_dir) = util::prepare_output_dir(quiet, &dir, out_dir).await;

		quietln!(quiet);

		// Start unpacking
		let mut unpacker = Unpacker {
			out_dir,
			dir,
			quiet
		};

		let started = std::time::Instant::now();

		if !in_place && unpacker.out_dir.exists() {
			quietln!(quiet, "Deleting old output directory...");
			tokio::fs::remove_dir_all(&unpacker.out_dir).await?;
		}

		let (sv_packed_file, cl_chunk_files, sh_chunk_files) = {
			let (mut sv_packed_file, mut cl_chunk_files, mut sh_chunk_files) = (None, Vec::new(), Vec::new());

			quietln!(quiet, "Copying addon to output directory...");
			tokio::fs::create_dir_all(&unpacker.out_dir).await?;

			let mut visited_symlinks = HashSet::new();
			fn copy_addon(visited_symlinks: &mut HashSet<PathBuf>, from: PathBuf, to: PathBuf, sv_packed_file: &mut Option<PathBuf>, cl_chunk_files: &mut Vec<PathBuf>, sh_chunk_files: &mut Vec<PathBuf>, lua_folder: &Path) -> Result<(), std::io::Error> {
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
						copy_addon(visited_symlinks, entry, dir, sv_packed_file, cl_chunk_files, sh_chunk_files, lua_folder)?;
					} else if entry.is_file() && !skip_copy {
						std::fs::copy(entry, to.join(&file_name))?;
					}
				}
				Ok(())
			}
			copy_addon(&mut visited_symlinks, unpacker.dir.clone(), unpacker.out_dir.clone(), &mut sv_packed_file, &mut cl_chunk_files, &mut sh_chunk_files, &unpacker.dir.join("lua"))?;

			(sv_packed_file, cl_chunk_files, sh_chunk_files)
		};

		unpacker.out_dir.push("lua");
		unpacker.dir.push("lua");

		let mut total_packed_files = cl_chunk_files.len() + sh_chunk_files.len();
		let mut total_unpacked_files = 0;

		if let Some(sv_packed_file) = sv_packed_file {
			total_packed_files += 1;

			quietln!(quiet, "Unpacking serverside files...");
			// Parse the serverside pack file and unpack it!
			total_unpacked_files += unpacker.parse_sv_packed_file(sv_packed_file).await?;
		}

		quietln!(quiet, "Unpacking clientside files...");
		total_unpacked_files += unpacker.parse_packed_files(cl_chunk_files).await?;

		quietln!(quiet, "Unpacking shared files...");
		total_unpacked_files += unpacker.parse_packed_files(sh_chunk_files).await?;

		Ok((total_unpacked_files, total_packed_files + 2, started.elapsed()))
	}

	async fn parse_sv_packed_file(&self, sv_packed_file: PathBuf) -> Result<usize, UnpackingError> {
		use std::{fs::File, io::{BufReader, Read}};

		let mut entries = 0;

		let mut f = BufReader::new(File::open(sv_packed_file)?);
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
			match read_entry(&self.out_dir, &mut f) {
				Ok(true) => break,
				Ok(false) => entries += 1,
				Err(error) => if let std::io::ErrorKind::UnexpectedEof = error.kind() {
					break;
				} else {
					return Err(UnpackingError::IoError(error));
				},
			}
		}

		Ok(entries)
	}

	async fn parse_packed_files(&self, packed_files: Vec<PathBuf>) -> Result<usize, UnpackingError> {
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

		fn read_entry(out_dir: &PathBuf, f: &mut std::io::Cursor<Vec<u8>>) -> Result<bool, UnpackingError> {
			let mut path = Vec::with_capacity(255);
			f.read_until(TERMINATOR_HACK, &mut path)?;

			if path.is_empty() {
				return Ok(true);
			}

			let mut len = Vec::with_capacity(16);
			f.read_until(TERMINATOR_HACK, &mut len)?;

			let len = u32::from_str_radix(std::str::from_utf8(&len[0..len.len()-1])?, 16)?;

			let path = out_dir.join(String::from_utf8_lossy(&path[0..path.len()-1]).as_ref());

			if let Some(parent) = path.parent() {
				std::fs::create_dir_all(parent)?;
			}

			println!("{}", path.display());

			let mut out = File::create(path)?;
			std::io::copy(&mut f.by_ref().take(len as u64), &mut out)?;

			Ok(false)
		}

		let mut f = Cursor::new(superchunk);
		loop {
			match read_entry(&self.out_dir, &mut f) {
				Ok(true) => break,
				Ok(false) => entries += 1,
				Err(UnpackingError::IoError(error)) => if let std::io::ErrorKind::UnexpectedEof = error.kind() {
					break;
				} else {
					return Err(UnpackingError::IoError(error));
				}
				Err(error) => return Err(error),
			}
		}

		Ok(entries)
	}
}

#[derive(Debug, thiserror::Error)]
pub enum UnpackingError {
	#[error("IO error: {0}")]
	IoError(std::io::Error),

	#[error("UTF-8 error: {0}")]
	Utf8Error(std::str::Utf8Error),

	#[error("File format error: {0}")]
	ParseIntError(std::num::ParseIntError),
}
impl From<std::io::Error> for UnpackingError {
	fn from(error: std::io::Error) -> Self {
		Self::IoError(error)
	}
}
impl From<std::str::Utf8Error> for UnpackingError {
	fn from(error: std::str::Utf8Error) -> Self {
		Self::Utf8Error(error)
	}
}
impl From<std::num::ParseIntError> for UnpackingError {
	fn from(error: std::num::ParseIntError) -> Self {
		Self::ParseIntError(error)
	}
}
