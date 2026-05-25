#!/usr/bin/env bash
# Bootstrap context for the handle-issue skill: print the target issue (full
# body) and the current main HEAD. cadenza is an original contract-first
# runtime, not a port — there is no upstream SPEC mirror to refresh; the frozen
# contract snapshots (abi/expected/, schemas/codex/, tools/versions.toml) are
# the authority.
# Fails loudly (set -euo pipefail) so the agent never proceeds on a bad
# argument or a failed fetch.
#
# Usage: bootstrap.sh <numeric-issue-number>
set -euo pipefail

REPO="bytevane/cadenza"

# Arg first: distinguish a bad/missing argument (exit 2) from later fetch
# failures (which exit non-zero via set -e). Require exactly one numeric arg
# so stray extra args (e.g. "123 456") are rejected, not silently dropped.
if [ "$#" -ne 1 ]; then
	echo "usage: bootstrap.sh <numeric-issue-number>" >&2
	exit 2
fi
issue="$1"
case "$issue" in
'' | *[!0-9]*)
	echo "usage: bootstrap.sh <numeric-issue-number>" >&2
	exit 2
	;;
esac

# 1. Target issue, full body (no truncation), pinned to the repo.
echo "=== issue #$issue ==="
gh issue view "$issue" --repo "$REPO" --json number,title,labels,body \
	--jq '"#\(.number) \(.title)\nlabels: \(.labels | map(.name) | join(", "))\n\n\(.body)"'

# 2. Current canonical main HEAD. Fetch by repo slug, not the `origin` remote
# (which may be a personal fork), so the baseline always matches the target
# repo. A failed fetch aborts here (no pipe masks its status); FETCH_HEAD holds
# the just-fetched canonical main.
echo "=== main HEAD ($REPO) ==="
git fetch "https://github.com/$REPO" main
git --no-pager log --oneline FETCH_HEAD -1
