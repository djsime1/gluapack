use std::{fs::File, path::Path};

use serde::de::{Unexpected, Visitor};

use crate::pack::PackingError;

macro_rules! impl_default {
	{ Config { $($field:ident: $ty:ty = $default:expr),* } } => {
		impl Default for Config {
			fn default() -> Self {
				Config {
					$($field: $default),*
				}
			}
		}

		$(
			#[inline(always)]
			#[allow(unused)]
			fn $field() -> $ty {
				$default
			}
		)*
	};
}

#[derive(derive_more::Deref, derive_more::DerefMut, Clone)]
pub struct GlobPattern(glob::Pattern);
impl GlobPattern {
	pub fn new(pattern: &'static str) -> Self {
		Self(glob::Pattern::new(pattern).unwrap())
	}

	pub fn matches<S: AsRef<str>>(&self, str: S) -> bool {
		let mut opt = glob::MatchOptions::new();
		opt.require_literal_separator = true;
		self.matches_with(str.as_ref(), opt)
	}

	pub fn matches_path<P: AsRef<std::path::Path>>(&self, path: P) -> bool {
		let mut opt = glob::MatchOptions::new();
		opt.require_literal_separator = true;
		self.matches_path_with(path.as_ref(), opt)
	}
}
impl From<glob::Pattern> for GlobPattern {
	fn from(pattern: glob::Pattern) -> Self {
		GlobPattern(pattern)
	}
}
impl std::fmt::Debug for GlobPattern {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0)
	}
}

struct GlobPatternVisitor;
impl<'de> Visitor<'de> for GlobPatternVisitor {
	type Value = GlobPattern;

	fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
		formatter.write_str("invalid glob pattern")
	}

	fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
	where
		E: serde::de::Error
	{
		glob::Pattern::new(v)
		.map(|pattern| pattern.into())
		.map_err(|error| {
			serde::de::Error::invalid_value(Unexpected::Other(&error.to_string()), &self)
		})
	}
}

impl<'de> serde::Deserialize<'de> for GlobPattern {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>
	{
		deserializer.deserialize_str(GlobPatternVisitor)
	}
}

impl serde::Serialize for GlobPattern {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer
	{
		serializer.serialize_str(self.0.as_str())
    }
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct Config {
	#[serde(default = "include_sh")]
	pub include_sh: Vec<GlobPattern>,

	#[serde(default = "include_cl")]
	pub include_cl: Vec<GlobPattern>,

	#[serde(default = "include_sv")]
	pub include_sv: Vec<GlobPattern>,

	#[serde(default = "exclude")]
	pub exclude: Vec<GlobPattern>,

	#[serde(default = "entry_cl")]
	pub entry_cl: Vec<GlobPattern>,

	#[serde(default = "entry_sh")]
	pub entry_sh: Vec<GlobPattern>,

	#[serde(default = "entry_sv")]
	pub entry_sv: Vec<GlobPattern>,

	#[serde(default)]
	pub unique_id: Option<String>,
}
impl Config {
	pub fn read<P: AsRef<Path>>(path: P) -> Result<Config, PackingError> {
		let mut f = File::open(path)?;
		Ok(serde_json::from_reader(&mut f)?)
	}

	pub fn dump_json(&self) {
		println!("{}", serde_json::to_string_pretty(&self).unwrap());
	}
}
impl_default! {
	Config {
		include_sh: Vec<GlobPattern> = vec![GlobPattern::new("**/sh_*.lua"), GlobPattern::new("**/*.sh.lua")],
		include_cl: Vec<GlobPattern> = vec![GlobPattern::new("**/cl_*.lua"), GlobPattern::new("**/*.cl.lua"), GlobPattern::new("vgui/*.lua")],
		include_sv: Vec<GlobPattern> = vec![GlobPattern::new("**/sv_*.lua"), GlobPattern::new("**/*.sv.lua")],
		exclude: Vec<GlobPattern> = vec![],

		entry_cl: Vec<GlobPattern> = vec![GlobPattern::new("autorun/client/*.lua"), GlobPattern::new("vgui/*.lua")], // TODO add more to this - i think there's more "autorun" stuff (postprocess/?)
		entry_sh: Vec<GlobPattern> = vec![GlobPattern::new("autorun/*.lua")],
		entry_sv: Vec<GlobPattern> = vec![GlobPattern::new("autorun/server/*.lua")],

		unique_id: Option<String> = None
	}
}
