#!/usr/bin/env bash
# Exercises the git hooks end to end, under both runners that read them.
#
# `cargo test` cannot reach any of this: the hook definitions, the entry
# strings, the file selection and the script that fetches the binary all live
# outside the Rust build. A typo in `.pre-commit-hooks.yaml` or a renamed
# installer variable would otherwise surface as a bug report rather than a
# failing build.
#
# Two scenarios, because they fail for different reasons:
#
#   wiring  Hook ids resolve, `types: [gdscript]` selects the right files, the
#           arguments reach the binary and the writing hooks write. Runs
#           against a locally built `gdck`, so it works on a pull request whose
#           version has never been released.
#
#   fetch   The script really downloads a published release, caches it, and
#           runs it. Pinned to the newest tag rather than this checkout's
#           version, which on `main` is routinely one that does not exist yet.
#
# Usage: tools/test-hooks.sh [path-to-gdck-binary]
set -euo pipefail

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
binary=${1:-$repo/target/debug/gdck}

if [ ! -x "$binary" ]; then
	echo "no gdck binary at $binary; run: cargo build -p gdck" >&2
	exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git_as=(git -c user.email=hooks@test -c user.name=hooks -c commit.gpgsign=false -c init.defaultBranch=main)

fail() {
	echo "FAIL: $*" >&2
	exit 1
}

# A hooks repository holding the definitions and script under test, at the
# version passed in. Both runners want a real git repo with a real revision.
make_hook_repo() {
	local dir=$1 version=$2
	mkdir -p "$dir/hooks"
	cp "$repo/.pre-commit-hooks.yaml" "$dir/"
	cp "$repo/hooks/gdck" "$dir/hooks/"
	printf '[workspace.package]\nversion = "%s"\n' "$version" >"$dir/Cargo.toml"
	(cd "$dir" && "${git_as[@]}" init -q && "${git_as[@]}" add -A &&
		"${git_as[@]}" commit -qm hooks && git rev-parse HEAD)
}

# A project needing exactly one thing from each hook: the file is misformatted
# but otherwise clean, so a writing hook must change it and a reading hook must
# report it.
make_project() {
	local dir=$1
	mkdir -p "$dir"
	printf 'extends Node\n\n\nfunc  f( ) ->void:\n\tprint( 1 )\n' >"$dir/messy.gd"
	(cd "$dir" && "${git_as[@]}" init -q && "${git_as[@]}" add -A &&
		"${git_as[@]}" commit -qm init)
}

formatted='func f() -> void:'

# --- wiring ------------------------------------------------------------------

echo "== wiring: prek and pre-commit against a local build"
rev=$(make_hook_repo "$work/hookrepo" 0.0.0-test)

for runner in prek pre-commit; do
	project=$work/wiring-$runner
	make_project "$project"

	# Each runner gets the config format it owns, so both parsers are covered.
	if [ "$runner" = prek ]; then
		cat >"$project/prek.toml" <<-EOF
			[[repos]]
			repo = "$work/hookrepo"
			rev = "$rev"
			hooks = [{ id = "gdck-format" }, { id = "gdck-lint" }]
		EOF
	else
		cat >"$project/.pre-commit-config.yaml" <<-EOF
			repos:
			  - repo: $work/hookrepo
			    rev: $rev
			    hooks:
			      - id: gdck-format
			      - id: gdck-lint
		EOF
	fi

	(
		cd "$project"
		"${git_as[@]}" add -A
		"${git_as[@]}" commit -qm config
		# The formatting hook rewrites the file and so reports failure. That is
		# the expected outcome here, not a reason to stop.
		GDCK_HOOK_BINARY=$binary PRE_COMMIT_HOME=$work/pchome \
			uvx --quiet "$runner" run --all-files >"$work/$runner.log" 2>&1 || true
	)

	grep -q "$formatted" "$project/messy.gd" ||
		fail "$runner: gdck-format did not format the file
$(cat "$work/$runner.log")"
	grep -qi 'gdck lint' "$work/$runner.log" ||
		fail "$runner: gdck-lint never ran
$(cat "$work/$runner.log")"
	echo "   $runner ok"
done

# --- fetch -------------------------------------------------------------------

echo "== fetch: downloading a published release"
released=$(
	curl --proto '=https' --tlsv1.2 -fsSL \
		https://api.github.com/repos/eth0net/gdck/releases/latest |
		sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p' | head -n 1
)
[ -n "$released" ] || fail "could not determine the latest released version"
echo "   latest release is $released"

rev=$(make_hook_repo "$work/hookrepo-released" "$released")
project=$work/fetch
make_project "$project"
cat >"$project/prek.toml" <<-EOF
	[[repos]]
	repo = "$work/hookrepo-released"
	rev = "$rev"
	hooks = [{ id = "gdck-format" }]
EOF

(
	cd "$project"
	"${git_as[@]}" add -A
	"${git_as[@]}" commit -qm config
	GDCK_HOOK_CACHE=$work/cache uvx --quiet prek run --all-files \
		>"$work/fetch.log" 2>&1 || true
)

grep -q "$formatted" "$project/messy.gd" ||
	fail "fetched binary did not format the file
$(cat "$work/fetch.log")"
[ -x "$work/cache/$released/gdck" ] ||
	fail "no cached binary at $work/cache/$released/gdck"

# The scratch directory the download is staged in must never survive: it is
# created inside the cache, and a leftover would accumulate on every version.
leftovers=$(find "$work/cache" -maxdepth 1 -name '.staging.*' | wc -l | tr -d ' ')
[ "$leftovers" = 0 ] || fail "install left $leftovers staging directories behind"

echo "   fetch ok, cached $released"
echo "all hook checks passed"
