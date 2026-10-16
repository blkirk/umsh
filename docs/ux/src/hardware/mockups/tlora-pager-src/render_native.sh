#!/bin/sh
# Rasterize each per-screen SVG at exactly one pixel per panel pixel with
# headless Chrome, then quantize to the panel's RGB565.
set -e
SRC="$1"
OUT="$2"
PROFILE="$3"
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
mkdir -p "$OUT"
for svg in "$SRC"/*.svg; do
  name=$(basename "$svg" .svg)
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars \
    --force-device-scale-factor=1 --window-size=480,222 \
    --user-data-dir="$PROFILE" --screenshot="$OUT/$name.png" \
    "file://$svg" >/dev/null 2>&1
  magick "$OUT/$name.png" \
    -channel R -fx "floor(u*31+0.5)/31" \
    -channel G -fx "floor(u*63+0.5)/63" \
    -channel B -fx "floor(u*31+0.5)/31" +channel \
    -strip "$OUT/$name.png"
done
