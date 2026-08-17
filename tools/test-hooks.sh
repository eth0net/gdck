#!/usr/bin/env bash
# Exercises the git hooks end to end, under both runners that read them.
#
# `cargo test` cannot reach any of this: the hook definitions, the sample
# configurations, the file selection and the script that fetches the binary all
# live outside the Rust build. A typo in `.pre-commit-hooks.yaml` or a renamed
# installer variable would otherwise surface as a bug report rather than a
# failing build.
#
# Three scenarios, because they fail for different reasons:
#
#   samples  The configurations in hooks/examples/ are the ones users copy, so
#            they are what gets run here — pointed at a local build rather than
#            a release. Catches a sample that drifted from the hook it names.
#
#   ids      Every hook in .pre-commit-hooks.yaml resolves and runs, including
#            the ones the samples leave commented out.
#
#   fetch    The script really downloads a published release, caches it, and
#            runs it. Pinned to the newest tag rather than this checkout's
#            version, which on `main` is routinely one that does not exist yet.
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

# Points a copied sample at the sandbox instead of the published release, so
# the sample's own hook ids and structure are what run.
retarget() {
	local file=$1 hookrepo=$2 rev=$3
	# `[0-9][0-9]*` rather than `[0-9]\+`, which BSD sed does not accept: this
	# has to behave the same on a maintainer's macOS as on a Linux runner.
	sed -i.bak \
		-e "s|https://github.com/eth0net/gdck|$hookrepo|" \
		-e "s|v[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*|$rev|" \
		"$file"
	rm -f "$file.bak"
}

# The runners write the hook's own failure to stdout; a writing hook reporting
# "files were modified" is the expected outcome, not a reason to stop.
run_hooks() {
	local runner=$1 project=$2 log=$3
	(
		cd "$project"
		"${git_as[@]}" add -A
		"${git_as[@]}" commit -qm config
		GDCK_HOOK_BINARY=$binary PRE_COMMIT_HOME=$work/pchome \
			uvx --quiet "$runner" run --all-files >"$log" 2>&1 || true
	)
}

formatted='func f() -> void:'

# --- samples -----------------------------------------------------------------

echo "== samples: the configurations in hooks/examples/, under both runners"
rev=$(make_hook_repo "$work/hookrepo" 0.0.0-test)

for runner in prek pre-commit; do
	project=$work/sample-$runner
	make_project "$project"

	if [ "$runner" = prek ]; then
		sample=prek.toml
	else
		sample=.pre-commit-config.yaml
	fi
	cp "$repo/hooks/examples/$sample" "$project/$sample"
	retarget "$project/$sample" "$work/hookrepo" "$rev"

	run_hooks "$runner" "$project" "$work/$runner.log"

	grep -q "$formatted" "$project/messy.gd" ||
		fail "$runner: the $sample sample did not fix the file
$(cat "$work/$runner.log")"
	echo "   $runner ok ($sample)"
done

# --- ids ---------------------------------------------------------------------

echo "== ids: every hook in .pre-commit-hooks.yaml"
project=$work/ids
make_project "$project"

# Read rather than hardcoded, so adding a hook adds it to this check too.
# A read loop rather than `mapfile`, which macOS's bash 3.2 does not have.
ids=()
while IFS= read -r id; do
	ids+=("$id")
done < <(sed -n 's/^- id: //p' "$repo/.pre-commit-hooks.yaml")
[ ${#ids[@]} -gt 0 ] || fail "no hook ids found in .pre-commit-hooks.yaml"

# Deliberately every hook at once, which the samples advise against for real
# use: the writing hooks tread on each other. Here the only question is whether
# each one resolves and runs at all.
{
	echo '[[repos]]'
	echo "repo = \"$work/hookrepo\""
	echo "rev = \"$rev\""
	echo 'hooks = ['
	for id in "${ids[@]}"; do
		echo "  { id = \"$id\" },"
	done
	echo ']'
} >"$project/prek.toml"

run_hooks prek "$project" "$work/ids.log"

for id in "${ids[@]}"; do
	# The runners print the hook's `name`, which is the id with the dash as a
	# space: `gdck-format` reports as `gdck format`.
	grep -q "${id/-/ }" "$work/ids.log" ||
		fail "$id never ran
$(cat "$work/ids.log")"
	echo "   $id ok"
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
