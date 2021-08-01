use std::{collections::HashSet, path::PathBuf};

use crate::{MAX_LUA_SIZE, config::Config, entities, pack::{LuaFile, Packer, PackingError, PackingStatistics}};

#[derive(Debug, Clone, Copy)]
pub enum Realm {
	Serverside,
	Clientside,
	Shared
}

#[derive(Debug, Clone)]
pub struct ShipmentFile {
	realm: Realm,
	entry: bool,
	path: String,
	contents: Vec<u8>
}
impl ShipmentFile {
	pub fn from_bytes(realm: Realm, entry: bool, path: String, contents: Vec<u8>) -> ShipmentFile {
		ShipmentFile { realm, entry, path, contents }
	}

	pub fn from_reader<R: std::io::Read>(realm: Realm, entry: bool, path: String, reader: &mut R) -> Result<ShipmentFile, std::io::Error> {
		let mut buf = Vec::with_capacity(MAX_LUA_SIZE);
		reader.read_to_end(&mut buf)?;
		buf.shrink_to_fit();

		Ok(ShipmentFile {
			contents: buf,
			realm,
			entry,
			path,
		})
	}
}

/// The shipment builder allows you to programmatically build a gluapacked addon by manually providing
/// serverside, clientside and shared files and entry files.
#[derive(Default, Debug, Clone)]
pub struct ShipmentBuilder {
	sv: Vec<LuaFile>,
	sv_entry_files: HashSet<String>,

	sh: Vec<LuaFile>,
	sh_entry_files: HashSet<String>,

	cl: Vec<LuaFile>,
	cl_entry_files: HashSet<String>,

	entity_dirs: HashSet<String>,
	weapon_dirs: HashSet<String>,
	effect_dirs: HashSet<String>,
}
impl ShipmentBuilder {
	pub fn add(&mut self, file: ShipmentFile) -> &mut Self {
		let (files, entry_files) = match file.realm {
			Realm::Serverside => (&mut self.sv, &mut self.sv_entry_files),
			Realm::Clientside => (&mut self.cl, &mut self.cl_entry_files),
			Realm::Shared => (&mut self.sh, &mut self.sh_entry_files),
		};

		if let Some((ent, ent_type)) = entities::extract_entity(&file.path) {
			match ent_type {
				entities::EntityType::Weapon => &mut self.weapon_dirs,
				entities::EntityType::Entity => &mut self.entity_dirs,
				entities::EntityType::Effect => &mut self.effect_dirs,
			}.insert(ent);
		} else if file.entry {
			entry_files.insert(file.path.clone());
		}

		let lua_file = LuaFile {
			path: file.path,
			contents: file.contents
		};

		if let Err(pos) = files.binary_search(&lua_file) {
			files.insert(pos, lua_file);
		}

		self
	}

	pub fn exists(&self, realm: Realm, path: String) -> bool {
		let files = match realm {
			Realm::Serverside => &self.sv,
			Realm::Clientside => &self.cl,
			Realm::Shared => &self.sh,
		};

		files.binary_search(&LuaFile {
			path,
			contents: vec![]
		}).is_ok()
	}

	pub fn reserve(&mut self, realm: Realm, entry: bool, additional: usize) -> &mut Self {
		match realm {
			Realm::Serverside => self.reserve_sv(entry, additional),
			Realm::Clientside => self.reserve_cl(entry, additional),
			Realm::Shared => self.reserve_sh(entry, additional),
		}
	}

	pub fn reserve_sv(&mut self, entry: bool, additional: usize) -> &mut Self {
		if entry {
			self.sv_entry_files.reserve(additional);
		} else {
			self.sv.reserve(additional);
		}
		self
	}

	pub fn reserve_cl(&mut self, entry: bool, additional: usize) -> &mut Self {
		if entry {
			self.cl_entry_files.reserve(additional);
		} else {
			self.cl.reserve(additional);
		}
		self
	}

	pub fn reserve_sh(&mut self, entry: bool, additional: usize) -> &mut Self {
		if entry {
			self.sh_entry_files.reserve(additional);
		} else {
			self.sh.reserve(additional);
		}
		self
	}

	pub fn has_cl(&self) -> bool {
		!self.cl.is_empty()
	}

	pub fn has_sh(&self) -> bool {
		!self.sh.is_empty()
	}

	pub fn has_sv(&self) -> bool {
		!self.sv.is_empty()
	}

	pub fn has_entities(&self) -> bool {
		!self.entity_dirs.is_empty()
	}

	pub fn has_weapons(&self) -> bool {
		!self.weapon_dirs.is_empty()
	}

	/// Consumes the builder and packs the shipment.
	pub async fn ship(self, out_dir: PathBuf, unique_id: Option<String>) -> Result<PackingStatistics, PackingError> {
		let packer = Packer {
			out_dir,
			config: {
				let mut conf = Config::default();
				conf.unique_id = unique_id;
				conf
			},
			unique_id: None,
			quiet: true,
			in_place: false,
			no_copy: true,
		};

		let total_unpacked_files = self.sv.len() + self.cl.len() + self.sh.len();

		let started = std::time::Instant::now();

		let (total_packed_files, total_packed_size, total_unpacked_size) = packer.process(
			self.sv.into_iter(),
			self.sv_entry_files.into_iter(),

			self.cl.into_iter(),
			self.cl_entry_files.into_iter(),

			self.sh.into_iter(),
			self.sh_entry_files.into_iter(),

			self.entity_dirs.into_iter(),
			self.weapon_dirs.into_iter(),
			self.effect_dirs.into_iter()
		).await?;

		Ok(PackingStatistics {
			total_unpacked_files,
			total_unpacked_size,
			total_packed_files: total_packed_files + 1,
			total_packed_size,
			elapsed: started.elapsed()
		})
	}
}
