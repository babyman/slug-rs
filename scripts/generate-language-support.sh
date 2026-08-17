#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
input="$root/docs/language-support.tsv"
output=${1:-"$root/docs/generated/language-support.md"}

{
    printf '%s\n' '# Language Support Matrix'
    printf '%s\n' ''
    printf '%s\n' 'Generated from `docs/language-support.tsv`; do not edit directly.'
    printf '%s\n' ''
    printf '%s\n' '| Feature | Status | Evidence |'
    printf '%s\n' '|---|---|---|'
    while IFS='|' read -r feature status evidence; do
        case "$feature" in
            ''|'#'*) continue ;;
        esac
        printf '| %s | %s | %s |\n' "$feature" "$status" "$evidence"
    done < "$input"
} > "$output"
