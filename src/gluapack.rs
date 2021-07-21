// The order of operations should be: sv cl sh

use crate::{PackingError, config::{Config, GlobPattern}};
use std::{collections::HashSet, convert::TryInto, path::PathBuf, sync::Arc, time::{Duration, Instant}};
use futures_util::{FutureExt, future};

use fsize::fsize;
use sha2::Digest;

/// Lua comment
const COMMENT_START: &'static [u8; 2] = b"--";

/// The maximum size of a chunk.
///
/// This should be 64 KiB as Garry's Mod will not network a Lua file larger than this.
const MAX_LUA_SIZE: usize = 65535;

/// Returns an iterator that will find all matches of the given glob pattern.
fn glob<S: AsRef<str>>(pattern: S) -> Result<glob::Paths, glob::PatternError> {
	let mut opt = glob::MatchOptions::new();
	opt.require_literal_separator = true;
	glob::glob_with(pattern.as_ref(), opt)
}

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
	dir: PathBuf,
	config: Config,
	unique_id: Option<String>
}
impl Packer {
	pub async fn pack(config: Config, dir: PathBuf) -> Result<(usize, usize, Duration), PackingError> {
		let mut packer = Packer {
			dir,
			config,
			unique_id: None
		};

		let started = std::time::Instant::now();

		println!("Collecting Lua files...");

		let ((sv, sv_entry_files), (cl, cl_entry_files), (sh, sh_entry_files)) = tokio::try_join!(
			packer.collect_lua_files(&packer.config.include_sv, &packer.config.exclude, &packer.config.entry_sv),
			packer.collect_lua_files(&packer.config.include_cl, &packer.config.exclude, &packer.config.entry_cl),
			packer.collect_lua_files(&packer.config.include_sh, &packer.config.exclude, &packer.config.entry_sh),
		)?;

		{
			println!("Checking realms...");
			let mut all_lua_files = HashSet::new();
			for lua_file in sv.iter().chain(sh.iter()).chain(cl.iter()) {
				if !all_lua_files.insert(lua_file.path.clone()) {
					return Err(PackingError::RealmConflict(lua_file.path.clone()));
				}
			}
		}

		let total_unpacked_files = sv.len() + cl.len() + sh.len();

		println!("Packing...");

		let (sv, cl, sh) = tokio::try_join!(
			tokio::task::spawn_blocking(move || Packer::pack_lua_files(sv, false)),
			tokio::task::spawn_blocking(move || Packer::pack_lua_files(cl, true)),
			tokio::task::spawn_blocking(move || Packer::pack_lua_files(sh, true))
		).expect("Failed to join threads");

		packer.delete_old_gluapack_files().await?;

		packer.unique_id = Some(packer.config.unique_id.as_ref().map(|x| x.to_owned()).unwrap_or_else(|| {
			const HASH_SUBHEX_LENGTH: usize = 16;

			println!("Calculating hash...");

			let mut sha256 = sha2::Sha256::new();
			sha256.update(&sv);
			sha256.update(&sh);
			format!("{:x}", sha256.finalize())[0..HASH_SUBHEX_LENGTH].to_string()
		}));

		tokio::fs::create_dir_all(packer.dir.join(&format!("gluapack/{}", packer.unique_id()))).await.expect("Failed to create gluapack directory");

		if !sv.is_empty() {
			println!("Writing packed serverside files...");
			tokio::fs::write(packer.dir.join(&format!("gluapack/{}/gluapack.sv.lua", packer.unique_id())), sv).await?;
		}

		let total_packed_files = if !cl.is_empty() || !sh.is_empty() {
			println!("Chunking...");

			let ((hashes_cl, chunk_n_cl), (hashes_sh, chunk_n_sh)) = tokio::try_join!(
				packer.write_packed_chunks(cl, "cl"),
				packer.write_packed_chunks(sh, "sh"),
			)?;

			if !hashes_cl.is_empty() || !hashes_sh.is_empty() {
				println!("Generating clientside Lua cache manifest...");
				packer.generate_cache_manifest(hashes_cl, hashes_sh).await?;
			}

			chunk_n_cl + chunk_n_sh
		} else {
			0
		};

		println!("Injecting loader...");
		packer.write_loader(sv_entry_files, cl_entry_files, sh_entry_files).await?;

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

		for pattern in patterns {
			for path in {
				glob(&self.dir.join(pattern.as_str()).to_string_lossy())
					.expect("Failed to construct glob when joining addon directory")
					.filter(|result| {
						match result {
							Ok(path) => 'filter: loop {
								for exclude in excludes {
									if exclude.matches_path(path.strip_prefix(&self.dir).unwrap()) {
										break 'filter false;
									}
								}
								break 'filter true;
							},
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

	async fn delete_old_gluapack_files(&self) -> Result<(), PackingError> {
		let gluapack_dir = glob(&self.dir.join("gluapack/*").to_string_lossy()).unwrap()
			.filter(|result| match result {
				Ok(path) => path.is_dir(),
				Err(_) => true
			})
			.take(1)
			.next();

		let gluapack_loader = glob(&self.dir.join("autorun/*_gluapack_*.lua").to_string_lossy())
			.unwrap()
			.take(1)
			.next();

		if gluapack_dir.is_some() || gluapack_loader.is_some() {
			println!("Deleting old gluapack files...");
			if let Some(gluapack_loader) = gluapack_loader {
				tokio::fs::remove_file(gluapack_loader?).await?;
			}
			if let Some(gluapack_dir) = gluapack_dir {
				tokio::fs::remove_dir_all(gluapack_dir?).await?;
			}
		}
		Ok(())
	}

	fn pack_lua_files(lua_files: HashSet<LuaFile>, is_sent_to_client: bool) -> Vec<u8> {
		const MEM_PREALLOCATE_MAX: usize = 1024 * 1024 * 1024;
		const TERMINATOR_HACK: u8 = '|' as u8;

		use std::io::Write;

		let mut superchunk: Vec<u8> = Vec::with_capacity((lua_files.len() * MAX_LUA_SIZE).min(MEM_PREALLOCATE_MAX));
		for mut lua_file in lua_files.into_iter() {
			superchunk.reserve_exact(lua_file.contents.len() + lua_file.path.len() + 4);

			superchunk.write_all(&mut lua_file.path.as_bytes()).expect("Failed to write script path into superchunk");
			if is_sent_to_client {
				// We can't use NUL to terminate because clientside Lua files will only send up to the NUL byte (fucking C strings)
				// We can just use a newline instead
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
		}

		superchunk
	}

	async fn write_packed_chunks(&self, bytes: Vec<u8>, chunk_name: &'static str) -> Result<(Vec<[u8; 20]>, usize), PackingError> {
		use tokio::io::AsyncWriteExt;

		let gluapack_dir = self.dir.join(format!("gluapack/{}", self.unique_id()));

		let is_sent_to_client = matches!(chunk_name, "sh" | "cl");
		if is_sent_to_client {
			let mut chunk_n = 0;
			let bytes = commentify(bytes);
			let mut writers = Vec::with_capacity((bytes.len() as fsize / MAX_LUA_SIZE as fsize).ceil() as usize);
			for (i, chunk) in bytes.chunks(MAX_LUA_SIZE).enumerate() {
				chunk_n += 1;
				let file_name = format!("gluapack.{}.{}.lua", i + 1, chunk_name);
				let path = gluapack_dir.join(&file_name);
				writers.push(async move {
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
				});
			}
			Ok((future::try_join_all(writers).await?, chunk_n))
		} else {
			let mut chunk_n = 0;
			let mut writers = Vec::with_capacity((bytes.len() as fsize / MAX_LUA_SIZE as fsize).ceil() as usize);
			for (i, chunk) in bytes.chunks(MAX_LUA_SIZE - if is_sent_to_client { 2 } else { 0 }).enumerate() {
				chunk_n += 1;
				let file_name = format!("gluapack.{}.{}.lua", i + 1, chunk_name);
				let path = gluapack_dir.join(&file_name);
				writers.push(tokio::fs::write(path, chunk));
			}

			future::try_join_all(writers).await?;
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
		tokio::fs::write(self.dir.join(format!("gluapack/{}/manifest.lua", self.unique_id())), cache_manifest).await?;

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

		tokio::fs::write(self.dir.join(format!("autorun/{}_gluapack_{}.lua", self.unique_id(), env!("CARGO_PKG_VERSION"))), loader).await?;

		Ok(())
	}
}
