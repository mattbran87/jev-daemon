#!/bin/sh
# The PNGs are rendered once and committed; rerun this script after any SVG
# change, then commit the PNGs again.
# Renders the tray PNGs from on.svg and off.svg. The build does not run it.
# It needs the resvg command line tool, version 0.48.1 exactly:
#   cargo install resvg --version 0.48.1 --locked
# Pass the tool's path as the first argument when it is not on PATH.
set -eu
resvg=${1:-resvg}
version=$("$resvg" --version 2>/dev/null | tr -d '\r') || version=
if [ "$version" != "0.48.1" ]; then
  echo "need resvg 0.48.1, found: ${version:-none}" >&2
  exit 1
fi
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
for state in on off; do
  for color in black white; do
    case $color in
      black) hex='#000000' ;;
      white) hex='#ffffff' ;;
    esac
    sed "s/currentColor/$hex/g" "$state.svg" > "$tmp/$state-$color.svg"
    for size in 16 20 24 32 48; do
      "$resvg" -w "$size" -h "$size" "$tmp/$state-$color.svg" "$state-$color-$size.png"
    done
  done
done
