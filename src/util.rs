use std::path::PathBuf;

#[macro_export]
macro_rules! abort {
	() => {
		std::process::exit(2);
	};
}

#[macro_export]
macro_rules! quietln {
	($quiet:expr, $($arg:tt)*) => {
		if !$quiet {
			println!($($arg)*);
		}
	};

	($quiet:expr) => {
		if !$quiet {
			println!();
		}
	};
}

/// Returns an iterator that will find all matches of the given glob pattern.
#[inline(always)]
pub fn glob<S: AsRef<str>>(pattern: S) -> Result<glob::Paths, glob::PatternError> {
	glob::glob_with(pattern.as_ref(), {
		let mut opt = glob::MatchOptions::new();
		opt.require_literal_separator = true;
		opt
	})
}

#[inline(always)]
pub fn canonicalize(path: &PathBuf) -> impl std::fmt::Display {
	#[cfg(target_os = "windows")] {
		let path = path.canonicalize().as_ref().unwrap_or_else(|_| &path).to_string_lossy().into_owned();
		path.clone().strip_prefix(r#"\\?\"#).map(|str| str.to_owned()).unwrap_or(path)
	}

	#[cfg(not(target_os = "windows"))]
	path.canonicalize().as_ref().unwrap_or_else(|_| &path).display()
}

#[inline(always)]
pub async fn prepare_output_dir(quiet: bool, dir: &PathBuf, out_dir: Option<PathBuf>) -> (bool, PathBuf) {
	if let Some(out_dir) = out_dir {
		if out_dir.is_dir() {
			tokio::fs::remove_dir_all(&out_dir).await.expect("Failed to delete existing output directory");
		} else if out_dir.is_file() {
			tokio::fs::remove_file(&out_dir).await.expect("Failed to delete existing output directory");
		}

		let result = tokio::fs::create_dir_all(&out_dir).await;

		quietln!(quiet, "Output Path: {}", canonicalize(&out_dir));

		result.expect("Failed to create output directory");

		(false, out_dir)
	} else {
		quietln!(quiet, "Output Path: In-place");
		(true, dir.clone())
	}
}