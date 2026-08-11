//! Turning the paths on the command line into a list of files to work on.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use gdck_config::Config;

/// The path spelling that means "read standard input".
pub(crate) const STDIN: &str = "-";

/// A file to process, with its contents already read.
#[derive(Debug, Clone)]
pub(crate) struct SourceFile {
    /// Display name. `STDIN` when the source came from standard input.
    pub(crate) name: String,
    pub(crate) text: String,
}

/// Expand command-line paths into `.gd` files.
///
/// Directories are walked recursively, skipping the configured exclusions.
/// Explicit file paths are taken as given even if they lack a `.gd` extension,
/// so `gdck parse odd_name.txt` still does what the user asked.
pub(crate) fn collect(paths: &[PathBuf], config: &Config) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_dir() {
            walk(path, config, &mut out).with_context(|| format!("walking {}", path.display()))?;
        } else {
            out.push(path.clone());
        }
    }
    // A stable order keeps output diffable between runs.
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, config: &Config, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if config.is_excluded_dir(&name) {
                continue;
            }
            walk(&path, config, out)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "gd") {
            out.push(path);
        }
    }
    Ok(())
}

/// Read one file, or standard input when `path` is `-`.
pub(crate) fn read(path: &Path) -> Result<SourceFile> {
    if path.as_os_str() == STDIN {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .context("reading standard input")?;
        return Ok(SourceFile {
            name: STDIN.to_string(),
            text,
        });
    }

    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(SourceFile {
        name: path.display().to_string(),
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_files_bypass_the_extension_filter() {
        let config = Config::default();
        let paths = vec![PathBuf::from("weird.txt")];
        let collected = collect(&paths, &config).expect("collect should succeed");
        assert_eq!(collected, vec![PathBuf::from("weird.txt")]);
    }

    #[test]
    fn stdin_is_passed_through_as_a_path() {
        let config = Config::default();
        let paths = vec![PathBuf::from(STDIN)];
        let collected = collect(&paths, &config).expect("collect should succeed");
        assert_eq!(collected, vec![PathBuf::from(STDIN)]);
    }
}
