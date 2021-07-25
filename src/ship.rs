use std::path::PathBuf;

use crate::{util, config::Config, pack::PackingError};

#[derive(Default)]
pub struct ShipBuilder {
	lua_folders: Vec<PathBuf>,
	config: Option<Config>
}
impl ShipBuilder {
	/// Creates a default ShipBuilder.
	pub fn new() -> Self {
		Self::default()
	}

	/// Sets the override config to be used for any addon included in this shipment.
	///
	/// This will override any addon's config specified in the `gluapack.json` file.
	pub fn override_config(&mut self, override_config: Option<Config>) -> &mut Self {
		self.config = override_config;
		self
	}

	/// Includes a Lua-containing folder to be shipped.
	pub fn include(&mut self, path: PathBuf) -> &mut Self {
		self.lua_folders.push(path);
		self
	}

	/// Includes a slice of Lua-containing folders to be shipped.
	pub fn includes(&mut self, paths: &[PathBuf]) -> &mut Self {
		self.lua_folders.extend_from_slice(paths);
		self
	}

	/// Includes a glob pattern of Lua-containing folders to be shipped.
	pub fn include_glob<S: AsRef<str>>(&mut self, glob: S) -> Result<&mut Self, glob::PatternError> {
		self.includes(&util::glob(glob.as_ref())?.filter_map(|x| x.ok()).collect::<Vec<PathBuf>>());
		Ok(self)
	}

	/// Consumes the builder and packs the ship.
	pub fn ship(mut self, out_dir: PathBuf) -> Result<(), PackingError> {
		todo!()
	}
}