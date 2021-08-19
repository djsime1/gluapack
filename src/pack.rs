// The order of operations should be: sv cl sh

use crate::{MAX_LUA_SIZE, MEM_PREALLOCATE_MAX, TERMINATOR_HACK, config::{Config, GlobPattern}, loadorder, util};
use std::{collections::HashSet, path::PathBuf, time::Duration};
use futures_util::{FutureExt, future};
use sha2::Digest;

#[derive(Debug, Clone)]
pub struct LuaFile {
	pub path: String,
	pub contents: Vec<u8>
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
impl PartialOrd for LuaFile {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(loadorder::cmp(&self.path, &other.path))
    }
}
impl Ord for LuaFile {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        loadorder::cmp(&self.path, &other.path)
    }
}

pub struct PackingStatistics {
	pub total_unpacked_files: usize,
	pub total_unpacked_size: usize,
	pub total_packed_files: usize,
	pub total_packed_size: usize,
	pub elapsed: Duration,
}
impl PackingStatistics {
	pub fn files(&self) -> String {
		let pct_change = (((self.total_unpacked_files as f64) - (self.total_packed_files as f64)) / (self.total_unpacked_files as f64)) * 100.;
		let sign = if pct_change == 0. { "" } else if pct_change > 0. { "-" } else { "+" };

		format!("{} files -> {} file(s) ({}{:.2}%)", self.total_unpacked_files, self.total_packed_files, sign, pct_change.abs())
	}

	pub fn size(&self) -> String {
		let pct_change = (((self.total_unpacked_size as f64) - (self.total_packed_size as f64)) / (self.total_unpacked_size as f64)) * 100.;
		let sign = if pct_change == 0. { "" } else if pct_change > 0. { "-" } else { "+" };

		format!("{} -> {} ({}{:.2}%)", util::file_size(self.total_unpacked_size), util::file_size(self.total_packed_size), sign, pct_change.abs())
	}

	pub fn elapsed(&self) -> String {
		format!("{:?}", self.elapsed)
	}
}

pub struct Packer {
	pub out_dir: PathBuf,
	pub config: Config,
	pub unique_id: Option<String>,
	pub quiet: bool,
	pub in_place: bool,
	pub no_copy: bool
}
impl Packer {
	pub async fn pack(mut dir: PathBuf, out_dir: Option<PathBuf>, no_copy: bool, quiet: bool, config: Option<Config>) -> Result<PackingStatistics, PackingError> {
		let mut config = match config {
			Some(config) => config,
			None => {
				let config_path = dir.join("gluapack.json");
				if config_path.is_file() {
					Config::read(config_path).await?
				} else {
					quietln!(quiet, "WARNING: Couldn't find gluapack.json in your addon. Using the default config.");
					Config::default()
				}
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

		if quiet && config.entry_cl.is_empty() && config.entry_sh.is_empty() && config.entry_sv.is_empty() {
			println!("WARNING: You have not specified any entry file patterns in your config. gluapack will do nothing after unpacking your addon.");
		}

		quietln!(quiet);

		// Make sure we exclude any previous gluapack files
		config.exclude.push(GlobPattern::new("gluapack/*/*"));
		config.exclude.push(GlobPattern::new("autorun/*_gluapack_*.lua"));

		// Start packing
		let mut packer = Packer {
			out_dir,
			config,
			unique_id: None,
			quiet,
			in_place,
			no_copy
		};

		let started = std::time::Instant::now();

		quietln!(quiet, "Collecting Lua files...");

		packer.out_dir.push("lua");
		dir.push("lua");

		let ((sv, sv_entry_files), (cl, cl_entry_files), (sh, sh_entry_files)) = tokio::try_join!(
			packer.collect_lua_files(&dir, &packer.config.include_sv, &packer.config.entry_sv),
			packer.collect_lua_files(&dir, &packer.config.include_cl, &packer.config.entry_cl),
			packer.collect_lua_files(&dir, &packer.config.include_sh, &packer.config.entry_sh),
		)?;

		quietln!(quiet, "Collecting entity files...");

		let entity_dirs = {
			// TODO
			//lazy_static! {
			//	static ref ENTITY_FILE_GLOBS: [GlobPattern; 3] = [
			//		GlobPattern::new("gamemodes/*/entities/[entities|weapons|effects]/*"),
			//		GlobPattern::new("[entities|weapons|effects]/*"),
			//	];
			//}
			let mut entity_dirs = HashSet::new();
			/*for file in sv.iter().chain(sh.iter()).chain(cl.iter()) {
				entity_dirs.insert(file.path.to_owned());
			}*/
			entity_dirs
		};

		quietln!(quiet, "Collecting weapon files...");

		let weapon_dirs = {
			// TODO
			//lazy_static! {
			//	static ref ENTITY_FILE_GLOBS: [GlobPattern; 3] = [
			//		GlobPattern::new("gamemodes/*/entities/[entities|weapons|effects]/*"),
			//		GlobPattern::new("[entities|weapons|effects]/*"),
			//	];
			//}
			let mut weapon_dirs = HashSet::new();
			/*for file in sv.iter().chain(sh.iter()).chain(cl.iter()) {
				entity_dirs.insert(file.path.to_owned());
			}*/
			weapon_dirs
		};

		quietln!(quiet, "Collecting effect files...");

		let effect_dirs = {
			// TODO
			//lazy_static! {
			//	static ref ENTITY_FILE_GLOBS: [GlobPattern; 3] = [
			//		GlobPattern::new("gamemodes/*/entities/[entities|weapons|effects]/*"),
			//		GlobPattern::new("[entities|weapons|effects]/*"),
			//	];
			//}
			let mut effect_dirs = HashSet::new();
			/*for file in sv.iter().chain(sh.iter()).chain(cl.iter()) {
				entity_dirs.insert(file.path.to_owned());
			}*/
			effect_dirs
		};

		{
			quietln!(quiet, "Checking realms...");
			let mut all_lua_files = HashSet::new();
			for lua_file in sv.iter().chain(sh.iter()).chain(cl.iter()) {
				if !all_lua_files.insert(lua_file.path.clone()) {
					return Err(error!(PackingError::RealmConflict(lua_file.path.clone())));
				}
			}
		}

		let total_unpacked_files = sv.len() + cl.len() + sh.len();
		if total_unpacked_files == 0 {
			return Err(error!(PackingError::NoLuaFiles));
		}

		if !in_place {
			if !no_copy {
				quietln!(quiet, "Copying addon to output directory...");
				packer.copy_addon(&dir).await?;
			}
		} else {
			quietln!(quiet, "Deleting old gluapack files...");
			packer.delete_old_gluapack_files().await?;
		}

		let (total_packed_files, total_packed_size, total_unpacked_size) = packer.process(
			sv.into_iter(),
			sv_entry_files.into_iter(),

			cl.into_iter(),
			cl_entry_files.into_iter(),

			sh.into_iter(),
			sh_entry_files.into_iter(),

			entity_dirs.into_iter(),
			weapon_dirs.into_iter(),
			effect_dirs.into_iter(),

			true
		).await?;

		Ok(PackingStatistics {
			total_unpacked_files,
			total_unpacked_size,
			total_packed_files: total_packed_files + 1,
			total_packed_size,
			elapsed: started.elapsed()
		})
	}

	fn unique_id(&self) -> &String {
		debug_assert!(self.unique_id.is_some());
		self.unique_id.as_ref().unwrap()
	}

	async fn collect_lua_files(&self, dir: &PathBuf, patterns: &[GlobPattern], entries: &[GlobPattern]) -> Result<(Vec<LuaFile>, Vec<String>), PackingError> {
		let mut lua_files = vec![];
		let mut entry_files = vec![];
		let mut abort_handles = vec![];

		let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Result<(Vec<u8>, String), std::io::Error>>();

		for pattern in patterns.iter().chain(entries.iter()) {
			for path in {
				util::glob(&dir.join(pattern.as_str()).to_string_lossy())
					.expect("Failed to construct glob when joining addon directory")
					.filter(|result| {
						match result {
							Ok(path) => self.config.exclude.iter().find(|exclude| exclude.matches_path(path.strip_prefix(&dir).unwrap())).is_none(),
							Err(_) => true,
						}
					})
			} {
				let fs_path = path?;
				let path = fs_path.strip_prefix(&dir).unwrap().to_string_lossy().into_owned().replace('\\', "/");
				let tx = tx.clone();

				if lua_files.binary_search(&LuaFile {
					path: path.to_owned(),
					contents: vec![]
				}).is_ok() {
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
					return Err(error!(PackingError::IoError(error)));
				}
			};

			for entry in entries {
				if entry.matches(&path) {
					entry_files.push(path.to_owned());
				}
			}

			let lua_file = LuaFile {
				path,
				contents
			};

			match lua_files.binary_search(&lua_file) {
				Err(pos) => lua_files.insert(pos, lua_file),
				Ok(_) => {
					// We've already included this file, skip it.
					continue
				},
			}
		}

		Ok((lua_files, entry_files))
	}

	async fn copy_addon(&self, dir: &PathBuf) -> Result<(), std::io::Error> {
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

		let from = dir.parent().unwrap().to_path_buf();
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

	async fn pack_lua_files<L>(collect_paths: bool, lua_files: L, is_sent_to_client: bool) -> (Vec<String>, Vec<u8>, usize)
	where
		L: Iterator<Item = LuaFile> + ExactSizeIterator
	{
		use tokio::io::AsyncWriteExt;

		let mut total_size = 0;

		let mut file_list = Vec::with_capacity(lua_files.len());

		let mut superchunk: Vec<u8> = Vec::with_capacity((lua_files.len() * MAX_LUA_SIZE).min(MEM_PREALLOCATE_MAX));
		for mut lua_file in lua_files.into_iter() {
			superchunk.reserve_exact(lua_file.contents.len() + lua_file.path.len() + 4);

			superchunk.write_all(&mut lua_file.path.as_bytes()).await.expect("Failed to write script path into superchunk");
			if is_sent_to_client {
				// We can't use NUL to terminate because clientside Lua files will only send up to the NUL byte (fucking C strings)
				// We can just use a | instead
				superchunk.push(TERMINATOR_HACK);

				// Write the length of the file as a hex string since we can't use NUL to terminate
				superchunk.write_all(format!("{:x}", lua_file.contents.len()).as_bytes()).await.expect("Failed to write Lua file length into superchunk");
				superchunk.push(TERMINATOR_HACK);
			} else {
				superchunk.push(0);

				debug_assert_eq!((lua_file.contents.len() as u32).to_le_bytes().len(), 4);
				for byte in (lua_file.contents.len() as u32).to_le_bytes().iter() {
					superchunk.push(*byte);
				}
			}

			superchunk.write_all(&mut lua_file.contents).await.expect("Failed to write Lua file into superchunk");

			if collect_paths {
				file_list.push(lua_file.path);
			}

			total_size += lua_file.contents.len();
		}

		(file_list, superchunk, total_size)
	}

	async fn write_packed_chunks(&self, bytes: Vec<u8>, chunk_name: &'static str) -> Result<(usize, usize), PackingError> {
		use tokio::io::AsyncWriteExt;

		const NEWLINE_BYTE: u8 = '\n' as u8;
		const QUOTE_BYTE: u8 = '"' as u8;
		const BACKSLASH_BYTE: u8 = '\\' as u8;
		const N_BYTE: u8 = 'n' as u8;
		const E_LOWER_BYTE: u8 = 'e' as u8;
		const E_UPPER_BYTE: u8 = 'E' as u8;
		const X_LOWER_BYTE: u8 = 'x' as u8;
		const X_UPPER_BYTE: u8 = 'X' as u8;
		const DOT_BYTE: u8 = '.' as u8;
		const DASH_BYTE: u8 = '-' as u8;

		if bytes.is_empty() {
			return Ok((0, 0));
		}

		let gluapack_dir = self.out_dir.join(format!("gluapack/{}", self.unique_id()));

		let mut f = Vec::with_capacity(MAX_LUA_SIZE);
		f.write_all(b"return\"").await.unwrap();

		let mut chunk_n = 1;
		let mut written = b"return\"".len();
		let mut total_written = written;

		macro_rules! written {
			(++$n:literal) => {
				written += $n;
				total_written += $n;
			};
			($n:literal) => {
				written = $n;
				total_written += $n;
			};
			(++$n:expr) => {
				let n = $n;
				written += n;
				total_written += n;
			};
			($n:expr) => {
				let n = $n;
				written = n;
				total_written += n;
			}
		}

		macro_rules! next_chunk {
			(@write) => {
				f.push(QUOTE_BYTE);
				std::fs::write(gluapack_dir.join(format!("gluapack.{}.{}.lua", chunk_n, chunk_name)), f)?;
			};

			() => {
				if f.len() > 0 {
					next_chunk!(@write);

					chunk_n += 1;
					written!(b"return\"".len());

					f = Vec::with_capacity(MAX_LUA_SIZE);
					f.write_all(b"return\"").await.unwrap();
				}
			}
		}

		let mut iter = bytes.into_iter();
		while let Some(byte) = iter.next() {
			match (byte.is_ascii_whitespace() || byte.is_ascii_control() || byte.is_ascii_digit(), byte) {
				(_, NEWLINE_BYTE) => {
					if written + 2 > MAX_LUA_SIZE {
						next_chunk!();
					}
					written!(++2);

					f.push(BACKSLASH_BYTE);
					f.push(N_BYTE);
				},
				(true, _) | (_, 0) | (_, E_LOWER_BYTE) | (_, E_UPPER_BYTE) | (_, X_LOWER_BYTE) | (_, X_UPPER_BYTE) | (_, DOT_BYTE) | (_, DASH_BYTE) => {
					let byte = byte.to_string();

					if written + byte.len() + 1 > MAX_LUA_SIZE {
						next_chunk!();
					}
					written!(++byte.len() + 1);

					f.push(BACKSLASH_BYTE);
					f.write_all(byte.as_bytes()).await.unwrap();
				},
				(_, BACKSLASH_BYTE) | (_, QUOTE_BYTE) => {
					if written + 2 > MAX_LUA_SIZE {
						next_chunk!();
					}
					written!(++2);

					f.push(BACKSLASH_BYTE);
					f.push(byte);
				},
				_ => {
					if written + 1 > MAX_LUA_SIZE {
						next_chunk!();
					}
					written!(++1);

					f.push(byte);
				}
			}
		}

		if f.len() > 0 {
			next_chunk!(@write);
		}

		Ok((chunk_n, total_written))
	}

	pub async fn build_loader<S, E>(&self, sv_entry_files: S, cl_entry_files: S, sh_entry_files: S, entity_dirs: E, weapon_dirs: E, effect_dirs: E) -> Result<String, PackingError>
	where
		S: Iterator<Item = String> + ExactSizeIterator,
		E: Iterator<Item = String> + ExactSizeIterator
	{
		const GLUAPACK_LOADER: &'static str = include_str!("gluapack.lua");

		async fn join_entry_files<S: Iterator<Item = String> + ExactSizeIterator>(entry_files: S) -> String {
			if entry_files.len() == 0 {
				"{}".to_string()
			} else {
				let mut output = "{".to_string();
				output.reserve(entry_files.len() * 255);
				for entry in {
					let mut entry_files: Vec<String> = entry_files.collect();
					loadorder::sort(&mut entry_files);
					entry_files
				} {
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

		async fn join_entity_dirs<S: Iterator<Item = String> + ExactSizeIterator>(entry_files: S) -> String {
			if entry_files.len() == 0 {
				"{}".to_string()
			} else {
				let mut output = "{".to_string();
				output.reserve(entry_files.len() * 255);
				for ent in entry_files {
					output.push('"');
					output.push_str(&ent.replace('\\', "\\\\").replace('"', "\\\""));
					output.push('"');
					output.push(',');
				}
				output.pop();
				output.push('}');
				output
			}
		}

		let (sv_entry_files, cl_entry_files, sh_entry_files, entity_dirs, weapon_dirs, effect_dirs) = tokio::join!(
			join_entry_files(sv_entry_files),
			join_entry_files(cl_entry_files),
			join_entry_files(sh_entry_files),
			join_entity_dirs(entity_dirs),
			join_entity_dirs(weapon_dirs),
			join_entity_dirs(effect_dirs),
		);

		let loader = GLUAPACK_LOADER
			.replacen("{ENTRY_FILES_SV}", &sv_entry_files, 1)
			.replacen("{ENTRY_FILES_CL}", &cl_entry_files, 1)
			.replacen("{ENTRY_FILES_SH}", &sh_entry_files, 1)
			.replacen("{ENTRY_ENTITIES}", &entity_dirs, 1)
			.replacen("{ENTRY_WEAPONS}", &weapon_dirs, 1)
			.replacen("{ENTRY_EFFECTS}", &effect_dirs, 1);

		Ok(loader)
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

	pub async fn squash_packed_files<L>(&self, sv: L, cl: L, sh: L, compress: bool) -> Result<((Vec<u8>, Vec<u8>, Vec<u8>), (Vec<String>, Vec<String>, Vec<String>), usize), PackingError>
	where
		L: Iterator<Item = LuaFile> + ExactSizeIterator + Send
	{
		quietln!(self.quiet, "Packing...");

		let collect_paths = !self.in_place && !self.no_copy;
		let ((sv_paths, sv, sv_len), (cl_paths, cl, cl_len), (sh_paths, sh, sh_len)) = tokio::join!(
			Packer::pack_lua_files(collect_paths, sv, false),
			Packer::pack_lua_files(collect_paths, cl, true),
			Packer::pack_lua_files(collect_paths, sh, true)
		);

		let total_unpacked_size = sv_len + cl_len + sh_len;

		let (cl, sh) = if compress {
			quietln!(self.quiet, "Compressing...");

			async fn compress(data: Vec<u8>) -> Result<Vec<u8>, gmod_lzma::SZ> {
				if data.is_empty() {
					Ok(data)
				} else {
					tokio::task::spawn_blocking(move || gmod_lzma::compress(&data, 9)).await.expect("Failed to join thread")
				}
			}
			future::try_join(
				compress(cl),
				compress(sh)
			).await.map_err(|_| error!(PackingError::CompressionError))?
		} else {
			(cl, sh)
		};

		Ok((
			(sv, cl, sh),
			(sv_paths, cl_paths, sh_paths),
			total_unpacked_size
		))
	}

	pub fn compute_unique_id(&mut self, sv: &[u8], sh: &[u8], cl: &[u8]) {
		self.unique_id = Some(self.config.unique_id.as_ref().map(|x| x.to_owned()).unwrap_or_else(|| {
			const HASH_SUBHEX_LENGTH: usize = 16;

			quietln!(self.quiet, "Calculating hash...");

			let mut sha256 = sha2::Sha256::new();
			sha256.update(sv);
			sha256.update(sh);
			sha256.update(cl);
			format!("{:x}", sha256.finalize())[0..HASH_SUBHEX_LENGTH].to_string()
		}));
	}

	pub async fn process<L, S, E>(mut self, sv: L, sv_entry_files: S, cl: L, cl_entry_files: S, sh: L, sh_entry_files: S, entity_dirs: E, weapon_dirs: E, effect_dirs: E, compress: bool) -> Result<(usize, usize, usize), PackingError>
	where
		L: Iterator<Item = LuaFile> + ExactSizeIterator + Send,
		S: Iterator<Item = String> + ExactSizeIterator + Send,
		E: Iterator<Item = String> + ExactSizeIterator + Send
	{
		let ((sv, cl, sh), (sv_paths, cl_paths, sh_paths), total_unpacked_size) = self.squash_packed_files(sv, cl, sh, compress).await?;

		self.compute_unique_id(&sv, &cl, &sh);

		tokio::fs::create_dir_all(self.out_dir.join(&format!("gluapack/{}", self.unique_id()))).await.expect("Failed to create gluapack directory");

		if !sv.is_empty() {
			quietln!(self.quiet, "Writing packed serverside files...");
			tokio::fs::write(self.out_dir.join(&format!("gluapack/{}/gluapack.sv.lua", self.unique_id())), &sv).await?;
		}

		let (mut total_packed_files, mut total_packed_size) = if !cl.is_empty() || !sh.is_empty() {
			quietln!(self.quiet, "Chunking...");

			let ((chunk_n_cl, chunk_size_cl), (chunk_n_sh, chunk_size_sh)) = tokio::try_join!(
				self.write_packed_chunks(cl, "cl"),
				self.write_packed_chunks(sh, "sh"),
			)?;

			(chunk_n_cl + chunk_n_sh, chunk_size_cl + chunk_size_sh)
		} else {
			(0, 0)
		};

		quietln!(self.quiet, "Injecting loader...");

		let loader = self.build_loader(sv_entry_files, cl_entry_files, sh_entry_files, entity_dirs, weapon_dirs, effect_dirs).await?;
		tokio::fs::create_dir_all(self.out_dir.join("autorun")).await?;
		tokio::fs::write(self.out_dir.join(format!("autorun/{}_gluapack_{}.lua", self.unique_id(), env!("CARGO_PKG_VERSION"))), loader).await?;

		if !self.in_place && !self.no_copy {
			quietln!(self.quiet, "Deleting unpacked files...");
			self.delete_unpacked(sv_paths, cl_paths, sh_paths).await?;
		}

		if !sv.is_empty() {
			total_packed_files += 1;
			total_packed_size += sv.len();
		}

		Ok((total_packed_files, total_packed_size, total_unpacked_size))
	}
}

#[derive(Debug, thiserror::Error)]
pub enum PackingError {
	#[error("IO error: {error}")]
	IoError {
		error: std::io::Error,
		#[cfg(all(debug_assertions, feature = "nightly"))]
		backtrace: std::backtrace::Backtrace
	},

	#[error("gluapack.json error: {error}")]
	ConfigError {
		error: serde_json::Error,
		#[cfg(all(debug_assertions, feature = "nightly"))]
		backtrace: std::backtrace::Backtrace
	},

	#[error("Realm conflict! This file is included in multiple realms: {error}\nPlease tinker your config and resolve the realm conflicts.")]
	RealmConflict {
		error: String,
		#[cfg(all(debug_assertions, feature = "nightly"))]
		backtrace: std::backtrace::Backtrace
	},

	#[error("No Lua files were found in your addon using this inclusion configuration")]
	NoLuaFiles {
		#[cfg(all(debug_assertions, feature = "nightly"))]
		backtrace: std::backtrace::Backtrace
	},

	#[error("Compression error")]
	CompressionError {
		#[cfg(all(debug_assertions, feature = "nightly"))]
		backtrace: std::backtrace::Backtrace
	},
}
impl_error!(std::io::Error, PackingError::IoError);
impl_error!(serde_json::Error, PackingError::ConfigError);
impl From<glob::GlobError> for PackingError {
	fn from(error: glob::GlobError) -> Self {
		Self::IoError {
			error: error.into_error(),
			#[cfg(all(debug_assertions, feature = "nightly"))]
			backtrace: std::backtrace::Backtrace::force_capture()
		}
	}
}
