#!/usr/bin/env bash
# Converts the PNGs written by the zgui site_shots tests into the WebP images
# the site loads. Requires ImageMagick.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
src="$root/target/zgui-shots/site"
dst="$root/site/img/shots"

# Shots that show a detail are cropped to it, keeping the 16:10 frame the site
# displays. Geometry is WIDTHxHEIGHT+X+Y in pixels of the 1600x1000 capture.
declare -A crops=(
    [rating]="1040x650+560+230"
)

mkdir -p "$dst"
for png in "$src"/*.png; do
    name="$(basename "$png" .png)"
    shot="${name%-*}"
    args=()
    if [[ -n "${crops[$shot]:-}" ]]; then
        args=(-crop "${crops[$shot]}" +repage -resize 1600x1000)
    fi
    magick "$png" "${args[@]}" -strip -quality 86 -define webp:method=6 "$dst/$name.webp"
    echo "$dst/$name.webp"
done
