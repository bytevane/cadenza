#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <slug>" >&2
  exit 1
fi

slug="$1"
next=$(find decisions -maxdepth 1 -name '[0-9][0-9][0-9][0-9]-*.md' | sed 's#.*/##; s#-.*##' | sort | tail -n1)
if [[ -z "${next:-}" ]]; then
  num="0001"
else
  num=$(printf '%04d' $((10#$next + 1)))
fi

file="decisions/${num}-${slug}.md"
cat > "$file" <<EOF
# ADR ${num}: ${slug}

## Status

Proposed.

## Context

## Decision

## Consequences

EOF

echo "$file"
