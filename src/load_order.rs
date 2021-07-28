//! https://wiki.facepunch.com/gmod/Lua_Loading_Order

use crate::config::GlobPattern;

fn cmp_index(path: &str) -> usize {
	match path {
		"includes/init.lua" => 0,
		"derma/init.lua" => 1,
		"base/gamemode/cl_init.lua" => 2,
		_ => {
			lazy_static! {
				static ref ORDERINGS: [GlobPattern; 17] = [
					GlobPattern::new("autorun/*.lua"),
					GlobPattern::new("autorun/server/*.lua"),
					GlobPattern::new("autorun/client/*.lua"),
					GlobPattern::new("postprocess/*.lua"),
					GlobPattern::new("vgui/*.lua"),
					GlobPattern::new("matproxy/*.lua"),
					GlobPattern::new("skins/*.lua"),
					GlobPattern::new("*/gamemode/cl_init.lua"),
					GlobPattern::new("weapons/*.lua"),
					GlobPattern::new("weapons/**/cl_init.lua"),
					GlobPattern::new("weapons/**/init.lua"),
					GlobPattern::new("weapons/**/shared.lua"),
					GlobPattern::new("entities/*.lua"),
					GlobPattern::new("entities/**/cl_init.lua"),
					GlobPattern::new("entities/**/init.lua"),
					GlobPattern::new("entities/**/shared.lua"),
					GlobPattern::new("effects/*.lua"),
				];
			}

			for (i, glob) in ORDERINGS.iter().enumerate() {
				if glob.matches(path) {
					return i + 3;
				}
			}

			ORDERINGS.len() + 3 + 1
		}
	}
}

pub fn sort<S: AsRef<str>>(items: &mut Vec<S>) {
	items.sort_unstable_by(|x, y| {
		let x = x.as_ref();
		let y = y.as_ref();
		match cmp_index(x).cmp(&cmp_index(y)) {
			std::cmp::Ordering::Equal => x.cmp(&y),
			cmp @ _ => cmp
		}
	});
}
