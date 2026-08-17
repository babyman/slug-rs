#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
language_index="$root/docs/language/README.md"
generated="$root/docs/generated/language-support.md"
temporary=$(mktemp "${TMPDIR:-/tmp}/slug-language-support.XXXXXX")
trap 'rm -f "$temporary"' EXIT HUP INT TERM

for file in \
    "$root/docs/README.md" \
    "$root/docs/architecture.md" \
    "$root/docs/compatibility.md" \
    "$root/docs/development.md" \
    "$root/docs/testing.md" \
    "$root/docs/language/language-specification.md" \
    "$root/docs/language/runtime-requirements.md" \
    "$root/docs/language/slug.ebnf" \
    "$root/docs/language-support.tsv" \
    "$generated"; do
    test -f "$file"
done

for file in "$root"/docs/language/*; do
    name=$(basename "$file")
    test "$name" = 'README.md' || grep -F "\`$name\`" "$language_index" >/dev/null
done

if grep -R -n -E '[[:blank:]]+$' "$root/docs"; then
    printf '%s\n' 'documentation contains trailing whitespace' >&2
    exit 1
fi

sh "$root/scripts/generate-language-support.sh" "$temporary"
diff -u "$generated" "$temporary"
