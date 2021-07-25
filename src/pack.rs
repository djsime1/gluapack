// The order of operations should be: sv cl sh

use crate::{MAX_LUA_SIZE, MEM_PREALLOCATE_MAX, TERMINATOR_HACK, util, config::{Config, GlobPattern}};
use std::{collections::HashSet, convert::TryInto, path::PathBuf, time::Duration};
use futures_util::{FutureExt, future};
use sha2::Digest;

/// Lua comment
const COMMENT_START: &'static [u8; 2] = b"--";

/// Prepends `--` to every line in the byte vector.
fn commentify(bytes: Vec<u8>) -> Vec<u8> {
	const NEWLINE: u8 = '\n' as u8;
	let mut escaped = Vec::with_capacity(bytes.len());
	escaped.push('-' as u8);
	escaped.push('-' as u8);
	for byte in bytes {
		escaped.push(byte);
		if byte == NEWLINE {
			escaped.reserve(2);
			escaped.push('-' as u8);
			escaped.push('-' as u8);
		}
	}
	escaped
}

struct LuaFile {
	path: String,
	contents: Vec<u8>
}
impl std::hash::Hash for LuaFile {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		self.path.hash(state)
	}
}
impl PartialEq for LuaFile {
	fn eq(&self, other: &Self) -> bool {
		self.path == other.path
	}
}
impl Eq for LuaFile {}

pub struct Packer {
	pub dir: PathBuf,
	pub out_dir: PathBuf,
	pub config: Config,
	pub unique_id: Option<String>,
	pub quiet: bool
}
impl Packer {
	pub async fn pack(dir: PathBuf, out_dir: Option<PathBuf>, quiet: bool) -> Result<(usize, usize, Duration), PackingError> {
		let mut config = {
			let config_path = dir.join("gluapack.json");
			if config_path.is_file() {
				Config::read(config_path)?
			} else {
				quietln!(quiet, "WARNING: Couldn't find gluapack.json in your addon. Using the default config.");
				Config::default()
			}
		};

		if !quiet {
			config.dump_json();
			println!("Addon Path: {}", util::canonicalize(&dir).display());
		}

		let (in_place, out_dir) = if let Some(out_dir) = out_dir {
			util::prepare_output_dir(quiet, &out_dir).await;
			(false, out_dir)
		} else {
			quietln!(quiet, "Output Path: In-place");
			(true, dir.clone())
		};

		if config.entry_cl.is_empty() && config.entry_sh.is_empty() && config.entry_sv.is_empty() {
			quietln!(quiet, "WARNING: You have not specified any entry file patterns in your config. gluapack will do nothing after unpacking your addon.");
		}

		quietln!(quiet);

		// Make sure we exclude any previous gluapack files
		config.exclude.push(GlobPattern::new("gluapack/*/*"));
		config.exclude.push(GlobPattern::new("autorun/*_gluapack_*.lua"));

		// Start packing
		let mut packer = Packer {
			out_dir,
			dir,
			config,
			unique_id: None,
			quiet
		};

		let started = std::time::Instant::now();

		quietln!(quiet, "Collecting Lua files...");

		packer.out_dir.push("lua");
		packer.dir.push("lua");

		let ((sv, sv_entry_files), (cl, cl_entry_files), (sh, sh_entry_files)) = tokio::try_join!(
			packer.collect_lua_files(&packer.config.include_sv, &packer.config.exclude, &packer.config.entry_sv),
			packer.collect_lua_files(&packer.config.include_cl, &packer.config.exclude, &packer.config.entry_cl),
			packer.collect_lua_files(&packer.config.include_sh, &packer.config.exclude, &packer.config.entry_sh),
		)?;

		{
			quietln!(quiet, "Checking realms...");
			let mut all_lua_files = HashSet::new();
			for lua_file in sv.iter().chain(sh.iter()).chain(cl.iter()) {
				if !all_lua_files.insert(lua_file.path.clone()) {
					return Err(PackingError::RealmConflict(lua_file.path.clone()));
				}
			}
		}

		let total_unpacked_files = sv.len() + cl.len() + sh.len();
		if total_unpacked_files == 0 {
			return Err(PackingError::NoLuaFiles);
		}

		if !in_place {
			quietln!(quiet, "Copying addon to output directory...");
			packer.copy_addon().await?;
		} else {
			quietln!(quiet, "Deleting old gluapack files...");
			packer.delete_old_gluapack_files().await?;
		}

		quietln!(quiet, "Packing...");

		let ((sv_paths, sv), (cl_paths, cl), (sh_paths, sh)) = tokio::try_join!(
			tokio::task::spawn_blocking(move || Packer::pack_lua_files(sv, false)),
			tokio::task::spawn_blocking(move || Packer::pack_lua_files(cl, true)),
			tokio::task::spawn_blocking(move || Packer::pack_lua_files(sh, true))
		).expect("Failed to join threads");

		packer.unique_id = Some(packer.config.unique_id.as_ref().map(|x| x.to_owned()).unwrap_or_else(|| {
			const HASH_SUBHEX_LENGTH: usize = 16;

			quietln!(quiet, "Calculating hash...");

			let mut sha256 = sha2::Sha256::new();
			sha256.update(&sv);
			sha256.update(&sh);
			format!("{:x}", sha256.finalize())[0..HASH_SUBHEX_LENGTH].to_string()
		}));

		tokio::fs::create_dir_all(packer.out_dir.join(&format!("gluapack/{}", packer.unique_id()))).await.expect("Failed to create gluapack directory");

		if !sv.is_empty() {
			quietln!(quiet, "Writing packed serverside files...");
			tokio::fs::write(packer.out_dir.join(&format!("gluapack/{}/gluapack.sv.lua", packer.unique_id())), sv).await?;
		}

		let total_packed_files = if !cl.is_empty() || !sh.is_empty() {
			quietln!(quiet, "Chunking...");

			let ((hashes_cl, chunk_n_cl), (hashes_sh, chunk_n_sh)) = tokio::try_join!(
				packer.write_packed_chunks(cl, "cl"),
				packer.write_packed_chunks(sh, "sh"),
			)?;

			if !hashes_cl.is_empty() || !hashes_sh.is_empty() {
				quietln!(quiet, "Generating clientside Lua cache manifest...");
				packer.generate_cache_manifest(hashes_cl, hashes_sh).await?;
			}

			chunk_n_cl + chunk_n_sh
		} else {
			0
		};

		quietln!(quiet, "Injecting loader...");
		packer.write_loader(sv_entry_files, cl_entry_files, sh_entry_files).await?;

		if !in_place {
			quietln!(quiet, "Deleting unpacked files...");
			packer.delete_unpacked(sv_paths, cl_paths, sh_paths).await?;
		}

		Ok((total_unpacked_files, total_packed_files + 3, started.elapsed()))
	}

	fn unique_id(&self) -> &String {
		debug_assert!(self.unique_id.is_some());
		self.unique_id.as_ref().unwrap()
	}

	async fn collect_lua_files(&self, patterns: &[GlobPattern], excludes: &[GlobPattern], entries: &[GlobPattern]) -> Result<(HashSet<LuaFile>, Vec<String>), PackingError> {
		let mut lua_files = HashSet::new();
		let mut entry_files = vec![];
		let mut abort_handles = vec![];

		let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Result<(Vec<u8>, String), std::io::Error>>();

		for pattern in patterns.iter().chain(entries.iter()) {
			for path in {
				util::glob(&self.dir.join(pattern.as_str()).to_string_lossy())
					.expect("Failed to construct glob when joining addon directory")
					.filter(|result| {
						match result {
							Ok(path) => excludes.iter().find(|exclude| exclude.matches_path(path.strip_prefix(&self.dir).unwrap())).is_none(),
							Err(_) => true,
						}
					})
			} {
				let fs_path = path?;
				let path = fs_path.strip_prefix(&self.dir).unwrap().to_string_lossy().into_owned().replace('\\', "/");
				let tx = tx.clone();

				if !lua_files.insert(LuaFile {
					path: path.to_owned(),
					contents: vec![]
				}) {
					// We've already included this file, skip it.
					continue;
				}

				abort_handles.push(
					tokio::spawn(async move {
						tx.send(
							tokio::fs::read(fs_path.clone()).map(|result| {
								result.map(|contents| {
									(contents, path)
								})
							}).await
						).ok();
					})
				);
			}
		}

		drop(tx);

		while let Some(result) = rx.recv().await {
			let (contents, path) = match result {
				Ok(data) => data,
				Err(error) => {
					abort_handles.into_iter().for_each(|handle| handle.abort());
					return Err(PackingError::IoError(error));
				}
			};

			for entry in entries {
				if entry.matches(&path) {
					entry_files.push(path.to_owned());
				}
			}

			lua_files.replace(LuaFile {
				path,
				contents
			});
		}

		Ok((lua_files, entry_files))
	}

	async fn copy_addon(&self) -> Result<(), std::io::Error> {
		let out_dir = self.out_dir.parent().unwrap(); // pop lua/

		tokio::fs::remove_dir_all(out_dir).await?;
		tokio::fs::create_dir_all(out_dir).await?;

		fn copy_addon(visited_symlinks: &mut HashSet<PathBuf>, from: PathBuf, to: PathBuf) -> Result<(), std::io::Error> {
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
					std::fs::create_dir_all(&dir)?;
					copy_addon(visited_symlinks, entry, dir)?;
				} else if entry.is_file() {
					std::fs::copy(entry, to.join(&file_name))?;
				}
			}
			Ok(())
		}

		let from = self.dir.parent().unwrap().to_path_buf();
		let to = out_dir.to_path_buf();

		tokio::task::spawn_blocking(move || {
			let mut visited_symlinks = HashSet::new();
			copy_addon(&mut visited_symlinks, from, to)
		}).await.expect("Failed to join thread")
	}

	async fn delete_old_gluapack_files(&self) -> Result<(), PackingError> {
		async fn delete<I, V>(gluapack_dir: I, gluapack_loader: V) -> Result<(), PackingError>
		where
			I: Iterator<Item = Result<PathBuf, glob::GlobError>>,
			V: Iterator<Item = Result<PathBuf, glob::GlobError>>
		{
			for gluapack_loader in gluapack_loader {
				tokio::fs::remove_file(gluapack_loader?).await?;
			}
			for gluapack_dir in gluapack_dir {
				tokio::fs::remove_dir_all(gluapack_dir?).await?;
			}
			Ok(())
		}

		if !self.quiet {
			let mut gluapack_dir = util::glob(&self.out_dir.join("gluapack/*").to_string_lossy()).unwrap()
				.filter(|result| match result {
					Ok(path) => path.is_dir(),
					Err(_) => true
				})
				.peekable();

			let mut gluapack_loader = util::glob(&self.out_dir.join("autorun/*_gluapack_*.lua").to_string_lossy()).unwrap().peekable();

			if gluapack_dir.peek().is_some() || gluapack_loader.peek().is_some() {
				println!("Deleting old gluapack files...");
				delete(gluapack_dir, gluapack_loader).await?;
			} else {
				return Ok(());
			}
		} else {
			let gluapack_dir = util::glob(&self.out_dir.join("gluapack/*").to_string_lossy()).unwrap()
				.filter(|result| match result {
					Ok(path) => path.is_dir(),
					Err(_) => true
				});

			let gluapack_loader = util::glob(&self.out_dir.join("autorun/*_gluapack_*.lua").to_string_lossy()).unwrap();

			delete(gluapack_dir, gluapack_loader).await?;
		};

		Ok(())
	}

	fn pack_lua_files(lua_files: HashSet<LuaFile>, is_sent_to_client: bool) -> (Vec<String>, Vec<u8>) {
		use std::io::Write;

		let mut file_list = Vec::with_capacity(lua_files.len());

		let mut superchunk: Vec<u8> = Vec::with_capacity((lua_files.len() * MAX_LUA_SIZE).min(MEM_PREALLOCATE_MAX));
		for mut lua_file in lua_files.into_iter() {
			superchunk.reserve_exact(lua_file.contents.len() + lua_file.path.len() + 4);

			superchunk.write_all(&mut lua_file.path.as_bytes()).expect("Failed to write script path into superchunk");
			if is_sent_to_client {
				// We can't use NUL to terminate because clientside Lua files will only send up to the NUL byte (fucking C strings)
				// We can just use a | instead
				superchunk.push(TERMINATOR_HACK);

				// Write the length of the file as a hex string since we can't use NUL to terminate
				superchunk.write_all(format!("{:x}", lua_file.contents.len()).as_bytes()).expect("Failed to write Lua file length into superchunk");
				superchunk.push(TERMINATOR_HACK);
			} else {
				superchunk.push(0);

				debug_assert_eq!((lua_file.contents.len() as u32).to_le_bytes().len(), 4);
				for byte in (lua_file.contents.len() as u32).to_le_bytes().iter() {
					superchunk.push(*byte);
				}
			}

			superchunk.write_all(&mut lua_file.contents).expect("Failed to write Lua file into superchunk");

			file_list.push(lua_file.path);
		}

		(file_list, superchunk)
	}

	async fn write_packed_chunks(&self, bytes: Vec<u8>, chunk_name: &'static str) -> Result<(Vec<[u8; 20]>, usize), PackingError> {
		use tokio::io::AsyncWriteExt;

		let gluapack_dir = self.out_dir.join(format!("gluapack/{}", self.unique_id()));

		let is_sent_to_client = matches!(chunk_name, "sh" | "cl");
		if is_sent_to_client {
			let mut chunk_n = 0;

			let hashes = future::try_join_all(
				commentify(bytes).chunks(MAX_LUA_SIZE).enumerate().map(|(i, chunk)| {
					chunk_n += 1;
					let file_name = format!("gluapack.{}.{}.lua", i + 1, chunk_name);
					let path = gluapack_dir.join(&file_name);
					async move {
						if !chunk.starts_with(COMMENT_START) {
							let mut f = tokio::fs::File::create(&path).await?;
							f.write_all(COMMENT_START).await?;
							f.write_all(&chunk).await?;

							Result::<[u8; 20], std::io::Error>::Ok({
								let mut sha256 = sha2::Sha256::new();
								sha256.update(COMMENT_START);
								sha256.update(chunk);
								sha256.update(&[0u8]);

								let sha256 = sha256.finalize();
								sha256[0..20].try_into().unwrap()
							})
						} else {
							tokio::fs::write(&path, &chunk).await?;

							Result::<[u8; 20], std::io::Error>::Ok({
								let mut sha256 = sha2::Sha256::new();
								sha256.update(chunk);
								sha256.update(&[0u8]);

								let sha256 = sha256.finalize();
								sha256[0..20].try_into().unwrap()
							})
						}
					}
				})
			).await?;

			Ok((hashes, chunk_n))
		} else {
			let mut chunk_n = 0;

			future::try_join_all(
				bytes.chunks(MAX_LUA_SIZE).enumerate().map(|(i, chunk)| {
					chunk_n += 1;
					let file_name = format!("gluapack.{}.{}.lua", i + 1, chunk_name);
					let path = gluapack_dir.join(&file_name);
					tokio::fs::write(path, chunk)
				})
			).await?;

			Ok((vec![], chunk_n))
		}
	}

	async fn generate_cache_manifest(&self, hashes_cl: Vec<[u8; 20]>, hashes_sh: Vec<[u8; 20]>) -> Result<(), PackingError> {
		let mut cache_manifest = String::new();
		cache_manifest.push_str("return{");

		if !hashes_sh.is_empty() {
			cache_manifest.push_str("sh={");
			for hash in hashes_sh {
				cache_manifest.push('"');
				cache_manifest.reserve(40);
				for byte in hash.iter() {
					cache_manifest.push_str(&format!("{:02x}", byte));
				}
				cache_manifest.push('"');
				cache_manifest.push(',');
			}
			cache_manifest.pop();
			cache_manifest.push('}');
			cache_manifest.push(',');
		}

		if !hashes_cl.is_empty() {
			cache_manifest.push_str("cl={");
			for hash in hashes_cl {
				cache_manifest.push('"');
				cache_manifest.reserve(40);
				for byte in hash.iter() {
					cache_manifest.push_str(&format!("{:02x}", byte));
				}
				cache_manifest.push('"');
				cache_manifest.push(',');
			}
			cache_manifest.pop();
			cache_manifest.push('}');
		}

		cache_manifest.push('}');
		tokio::fs::write(self.out_dir.join(format!("gluapack/{}/manifest.lua", self.unique_id())), cache_manifest).await?;

		Ok(())
	}

	async fn write_loader(&self, sv_entry_files: Vec<String>, cl_entry_files: Vec<String>, sh_entry_files: Vec<String>) -> Result<(), PackingError> {
		const GLUAPACK_LOADER: &'static str = include_str!("gluapack.lua");

		fn join_entry_files(entry_files: Vec<String>) -> String {
			if entry_files.is_empty() {
				"{}".to_string()
			} else {
				let mut output = "{".to_string();
				output.reserve(entry_files.len() * 255);
				for entry in entry_files {
					output.push('"');
					output.push_str(&entry.replace('\\', "\\\\").replace('"', "\\\""));
					output.push('"');
					output.push(',');
				}
				output.pop();
				output.push('}');
				output
			}
		}

		let (sv_entry_files, cl_entry_files, sh_entry_files) = tokio::try_join!(
			tokio::task::spawn_blocking(move || join_entry_files(sv_entry_files)),
			tokio::task::spawn_blocking(move || join_entry_files(cl_entry_files)),
			tokio::task::spawn_blocking(move || join_entry_files(sh_entry_files)),
		).expect("Failed to join threads");

		let loader = GLUAPACK_LOADER
			.replacen("{ENTRY_FILES_SV}", &sv_entry_files, 1)
			.replacen("{ENTRY_FILES_CL}", &cl_entry_files, 1)
			.replacen("{ENTRY_FILES_SH}", &sh_entry_files, 1);

		tokio::fs::create_dir_all(self.out_dir.join("autorun")).await?;
		tokio::fs::write(self.out_dir.join(format!("autorun/{}_gluapack_{}.lua", self.unique_id(), env!("CARGO_PKG_VERSION"))), loader).await?;

		Ok(())
	}

	async fn delete_unpacked(&self, sv_paths: Vec<String>, cl_paths: Vec<String>, sh_paths: Vec<String>) -> Result<(), PackingError> {
		let mut check_empty = Vec::new();

		future::try_join_all(
			sv_paths.into_iter().chain(cl_paths.into_iter()).chain(sh_paths.into_iter()).map(|path| {
				let path = self.out_dir.join(path);
				for ancestor in path.ancestors().skip(1) {
					if ancestor == self.out_dir {
						break;
					} else {
						let ancestor = ancestor.to_path_buf();
						if let Err(pos) = check_empty.binary_search_by(|probe: &PathBuf| probe.cmp(&ancestor).reverse()) {
							check_empty.insert(pos, ancestor);
						}
					}
				}
				tokio::fs::remove_file(path)
			})
		).await?;

		tokio::task::spawn_blocking(move || {
			for dir in check_empty {
				std::fs::remove_dir(dir).ok();
			}
		}).await.expect("Failed to join thread");

		Ok(())
	}
}

#[derive(Debug, thiserror::Error)]
pub enum PackingError {
	#[error("IO error: {0}")]
	IoError(std::io::Error),

	#[error("gluapack.json error: {0}")]
	ConfigError(serde_json::Error),

	#[error("Realm conflict! This file is included in multiple realms: {0}\nPlease tinker your config and resolve the realm conflicts.")]
	RealmConflict(String),

	#[error("No Lua files were found in your addon using this inclusion configuration")]
	NoLuaFiles,
}
impl From<std::io::Error> for PackingError {
	fn from(error: std::io::Error) -> Self {
		Self::IoError(error)
	}
}
impl From<glob::GlobError> for PackingError {
	fn from(error: glob::GlobError) -> Self {
		Self::IoError(error.into_error())
	}
}
impl From<serde_json::Error> for PackingError {
	fn from(error: serde_json::Error) -> Self {
		Self::ConfigError(error)
	}
}
