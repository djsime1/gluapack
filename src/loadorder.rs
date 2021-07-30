//! https://wiki.facepunch.com/gmod/Lua_Loading_Order

use crate::config::GlobPattern;

fn cmp_index(path: &str) -> usize {
	lazy_static! {
		static ref ORDERINGS: [GlobPattern; 9] = [
			GlobPattern::new("includes/init.lua"),
			GlobPattern::new("derma/init.lua"),
			GlobPattern::new("autorun/*.lua"),
			GlobPattern::new("autorun/server/*.lua"),
			GlobPattern::new("autorun/client/*.lua"),
			GlobPattern::new("postprocess/*.lua"),
			GlobPattern::new("vgui/*.lua"),
			GlobPattern::new("matproxy/*.lua"),
			GlobPattern::new("skins/default.lua"),
		];
	}

	for (i, glob) in ORDERINGS.iter().enumerate() {
		if glob.matches(path) {
			return i;
		}
	}

	ORDERINGS.len()
}

pub fn cmp<S: AsRef<str>>(x: S, y: S) -> std::cmp::Ordering {
	let x = x.as_ref();
	let y = y.as_ref();
	match cmp_index(x).cmp(&cmp_index(y)) {
		std::cmp::Ordering::Equal => x.cmp(&y),
		cmp @ _ => cmp
	}
}

pub fn sort<S: AsRef<str>>(items: &mut Vec<S>) {
	items.sort_unstable_by(|x, y| {
		cmp(x, y)
	});
}
