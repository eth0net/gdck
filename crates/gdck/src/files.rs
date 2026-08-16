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
/// Directories are walked recursively, skipping the configured exclusions and
/// anything a `.gitignore` covers. Explicit file paths are taken as given even
/// if they lack a `.gd` extension, so `gdck parse odd_name.txt` still does what
/// the user asked — and a named path is processed whatever the ignore files
/// say, since naming a file is a stronger signal than a pattern matching it.
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
    let excluded = effective_exclusions(config);
    let mut builder = ignore::WalkBuilder::new(dir);
    builder
        // Every ignore mechanism git itself has, so that what `gdck` skips is
        // what `git status` would leave out: the nearest `.gitignore` and every
        // one above it, `.git/info/exclude`, and the user's global file.
        .git_ignore(config.respect_gitignore)
        .git_exclude(config.respect_gitignore)
        .git_global(config.respect_gitignore)
        .ignore(false)
        .parents(config.respect_gitignore)
        // Without a `.git` directory these files would otherwise be skipped.
        // A checkout without one — an export, a vendored copy — still meant
        // what its `.gitignore` says.
        .require_git(false)
        // `gdck` decides what to skip by name, and a leading dot is not a
        // reason on its own: `.gd` files under a dot-directory are still
        // somebody's code unless the exclusions say otherwise.
        .hidden(false)
        .filter_entry(move |entry| {
            // Directories only: a *file* called `addons` is not the addons
            // directory, and the exclusions are a list of directory names.
            // Depth 0 is the directory the walk was pointed at, which was
            // named on the command line and so is never filtered out.
            !entry.file_type().is_some_and(|kind| kind.is_dir())
                || entry.depth() == 0
                || !excluded.iter().any(|name| name == entry.file_name())
        });

    for entry in builder.build() {
        let entry = entry.with_context(|| format!("walking {}", dir.display()))?;
        let path = entry.path();
        if entry.file_type().is_some_and(|kind| kind.is_file())
            && path.extension().is_some_and(|ext| ext == "gd")
        {
            out.push(path.to_path_buf());
        }
    }
    Ok(())
}

/// The directory names in force, resolved from the configuration.
fn effective_exclusions(config: &Config) -> Vec<std::ffi::OsString> {
    let mut names = Vec::new();
    for candidate in gdck_config::DEFAULT_EXCLUDED_DIRS {
        if config.is_excluded_dir(candidate) {
            names.push(std::ffi::OsString::from(candidate));
        }
    }
    for name in &config.excluded_dirs {
        let owned = std::ffi::OsString::from(name);
        if !names.contains(&owned) {
            names.push(owned);
        }
    }
    names
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

    /// A throwaway tree, removed when the test ends.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("gdck-files-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("should create the tree");
            Self(root)
        }

        fn write(&self, path: &str, text: &str) {
            let full = self.0.join(path);
            std::fs::create_dir_all(full.parent().expect("has a parent")).expect("should create");
            std::fs::write(&full, text).expect("should write");
        }

        fn collect_names(&self, config: &Config) -> Vec<String> {
            let mut names: Vec<String> = collect(std::slice::from_ref(&self.0), config)
                .expect("collect should succeed")
                .iter()
                .map(|path| {
                    path.strip_prefix(&self.0)
                        .expect("under the root")
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_gitignored_file_is_skipped_and_the_setting_brings_it_back() {
        // A `.gd` file git has been told to ignore is nearly always generated
        // or vendored, so reporting on it is noise about code nobody edits.
        let tree = Tree::new("gitignored");
        tree.write(".gitignore", "build/\ngenerated_*.gd\n");
        tree.write("build/out.gd", "extends Node\n");
        tree.write("src/generated_thing.gd", "extends Node\n");
        tree.write("src/real.gd", "extends Node\n");

        let mut config = Config::default();
        assert_eq!(tree.collect_names(&config), ["src/real.gd"]);

        config.respect_gitignore = false;
        assert_eq!(
            tree.collect_names(&config),
            ["build/out.gd", "src/generated_thing.gd", "src/real.gd"]
        );
    }

    #[test]
    fn a_named_file_is_processed_even_when_gitignored() {
        // Naming a file is a stronger signal than a pattern matching it, and
        // it is the same rule the exclusions already follow.
        let tree = Tree::new("named");
        tree.write(".gitignore", "build/\n");
        tree.write("build/out.gd", "extends Node\n");

        let named = tree.0.join("build/out.gd");
        let collected =
            collect(std::slice::from_ref(&named), &Config::default()).expect("should collect");
        assert_eq!(collected, vec![named]);
    }

    #[test]
    fn a_gitignore_negation_is_honoured() {
        // The reason this is not hand-rolled. `!` re-includes, and getting it
        // wrong means silently skipping a file somebody asked to keep.
        let tree = Tree::new("negation");
        tree.write(".gitignore", "generated_*.gd\n!src/generated_keep.gd\n");
        tree.write("src/generated_thing.gd", "extends Node\n");
        tree.write("src/generated_keep.gd", "extends Node\n");

        assert_eq!(
            tree.collect_names(&Config::default()),
            ["src/generated_keep.gd"]
        );
    }

    #[test]
    fn the_exclusions_and_the_gitignore_both_apply() {
        // Two mechanisms, and a file needs to pass both. `addons` is committed
        // and so is never gitignored; `build/` is ignored and not a name in the
        // exclusion list. Neither would catch the other's case.
        let tree = Tree::new("both");
        tree.write(".gitignore", "build/\n");
        tree.write("build/out.gd", "extends Node\n");
        tree.write("addons/a.gd", "extends Node\n");
        tree.write("src/real.gd", "extends Node\n");

        assert_eq!(tree.collect_names(&Config::default()), ["src/real.gd"]);
    }
}
