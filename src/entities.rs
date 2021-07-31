pub fn extract_entity(path: &str) -> Option<String> {
	lazy_static! {
		static ref RE_ENTITY: regex::Regex = regex::Regex::new(r#"^(?:gamemodes/([^/]+/entities/)|addons/[^/]+/lua/)?((?:entities|weapons|effects)/[^/]+(?:\.lua|/(?:cl_init|init|shared)\.lua))$"#).unwrap();
	}
	RE_ENTITY.captures(path).map(|caps| {
		format!("{}{}", caps.get(1).map(|x| x.as_str()).unwrap_or(""), &caps[2])
	})
}
