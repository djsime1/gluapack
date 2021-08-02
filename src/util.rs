use std::path::PathBuf;

macro_rules! abort {
	() => {
		std::process::exit(2);
	};
}

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

macro_rules! impl_error {
	($from:ty, $to:ident::$err:ident) => {
		impl From<$from> for $to {
			fn from(error: $from) -> Self {
				Self::$err {
					error,
					#[cfg(all(debug_assertions, feature = "nightly"))]
					backtrace: std::backtrace::Backtrace::force_capture()
				}
			}
		}
	}
}

macro_rules! error {
	($enum:ident::$variant:ident($error:expr)) => {
		$enum::$variant {
			error: $error,
			#[cfg(all(debug_assertions, feature = "nightly"))]
			backtrace: std::backtrace::Backtrace::force_capture()
		}
	};

	($enum:ident::$variant:ident) => {
		$enum::$variant {
			#[cfg(all(debug_assertions, feature = "nightly"))]
			backtrace: std::backtrace::Backtrace::force_capture()
		}
	}
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
pub fn canonicalize(path: &PathBuf) -> PathBuf {
	dunce::canonicalize(path).as_ref().unwrap_or(path).to_owned()
}

#[inline(always)]
pub(crate) async fn prepare_output_dir(quiet: bool, out_dir: &PathBuf) {
	if out_dir.is_dir() {
		quietln!(quiet, "Deleting old output directory...");
		tokio::fs::remove_dir_all(&out_dir).await.expect("Failed to delete existing output directory");
	} else if out_dir.is_file() {
		quietln!(quiet, "Deleting old output directory...");
		tokio::fs::remove_file(&out_dir).await.expect("Failed to delete existing output directory");
	}

	let result = tokio::fs::create_dir_all(&out_dir).await;

	quietln!(quiet, "Output Path: {}", canonicalize(&out_dir).display());

	result.expect("Failed to create output directory");
}

pub fn file_size(bytes: usize) -> String {
	if bytes > 1000 * 1000 {
		format!("{:.2} MB", bytes as f32 / 1000. / 1000.)
	}else if bytes > 1000 {
		format!("{:.2} KB", bytes as f32 / 1000.)
	} else {
		format!("{} bytes", bytes)
	}
}