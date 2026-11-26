#!/bin/bash
# Draws the README's pictures into docs/readme/, in English, Russian and Chinese, all dark:
#   hero-<lang>.png             the device window on Windows and on a Mac
#   menubar-<lang>.gif          the Mac menu bar: a keyboard runs down, turns orange, then red, and charges back
#   taskbar-<lang>.gif          the same on the Windows taskbar
#   reminder-<lang>.png         the reminder, its line of the time left at 70%
#   overlay-<lang>.png          the overlay over a game
#   install-windows-<lang>.png, install-macos-<lang>.png   where to click on the first start
#
# The Mac parts are drawn on this Mac: the app is built (macos/build.sh native) and draws its windows with the demo's
# devices (`--snapshot <folder> <lang> dark frames`), so for a minute or two its windows come and go on the screen and
# take the focus. The Windows parts come from a Desktop workflow run (its desktop-windows-2025 artifact: the snapshot
# example's windows at 150% and strip_probe's frames): the newest successful one of the current branch, or of main, or
# the run given as the argument. The scenes around them are drawn by macos/readme-pictures.swift.
#
# Needs what macos/build.sh needs, ffmpeg (brew install ffmpeg) and gh, logged in.
# Usage: macos/readme-pictures.sh [Desktop run id]
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT="$ROOT/docs/readme"
REPO=qu1ncky/BatteryTray
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

for tool in ffmpeg gh swiftc; do
  command -v "$tool" >/dev/null || { echo "error: $tool is needed" >&2; exit 1; }
done

run=${1:-}
if [ -z "$run" ]; then
  for branch in "$(git -C "$ROOT" branch --show-current)" main; do
    run=$(gh run list --repo "$REPO" --workflow Desktop --branch "$branch" --status success --limit 1 \
      --json databaseId --jq '.[0].databaseId // empty')
    [ -n "$run" ] && break
  done
fi
[ -n "$run" ] || { echo "error: no successful Desktop run found; give its id" >&2; exit 1; }
echo "== the Windows pictures: Desktop run $run"
gh run download "$run" --repo "$REPO" --name desktop-windows-2025 --dir "$WORK/windows"
[ -d "$WORK/windows/strip" ] || { echo "error: run $run has no strip pictures (strip_probe)" >&2; exit 1; }

echo "== the Mac app"
"$ROOT/macos/build.sh" native
APP="$ROOT/target/macos/BatteryTray.app/Contents/MacOS/BatteryTray"
swiftc -O "$ROOT/macos/readme-pictures.swift" -o "$WORK/compose"

# One GIF from a folder of frames (f000.png, …): every frame shares one palette, and only what changes is redrawn.
# The menu bar and the taskbar step every 0.45 s, rest at the critical level and charge faster.
gif() {
  local dir=$1 out=$2 kind=$3 list="$WORK/frames.txt"
  local frames=("$dir"/f*.png)
  local n=${#frames[@]} i d
  : > "$list"
  for i in "${!frames[@]}"; do
    case $kind in
      bar) d=0.45; [ "$i" -eq 9 ] && d=1.4; [ "$i" -gt 9 ] && d=0.35; [ "$i" -eq $((n - 1)) ] && d=1.1 ;;
    esac
    printf "file '%s'\nduration %s\n" "${frames[$i]}" "$d" >> "$list"
  done
  printf "file '%s'\n" "${frames[$((n - 1))]}" >> "$list"
  ffmpeg -v error -y -f concat -safe 0 -i "$list" -fps_mode vfr \
    -vf "split[a][b];[a]palettegen=stats_mode=full[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle" \
    -loop 0 "$out"
}

mkdir -p "$OUT" "$WORK/home"
for lang in en ru zh; do
  echo "== $lang"
  # A home of its own: the user's settings and login item stay as they are
  HOME="$WORK/home" "$APP" --snapshot "$WORK/mac-$lang" "$lang" dark frames
  "$WORK/compose" hero "$WORK/windows/windows-$lang-150/readme.png" "$WORK/mac-$lang/main.png" "$OUT/hero-$lang.png"
  "$WORK/compose" menubar "$WORK/mac-$lang" "$lang" "$WORK/menubar-$lang"
  gif "$WORK/menubar-$lang" "$OUT/menubar-$lang.gif" bar
  "$WORK/compose" taskbar "$WORK/windows/strip" "$lang" "$WORK/taskbar-$lang"
  gif "$WORK/taskbar-$lang" "$OUT/taskbar-$lang.gif" bar
  "$WORK/compose" reminder "$WORK/mac-$lang/reminder-70.png" "$lang" "$OUT/reminder-$lang.png"
  "$WORK/compose" overlay "$WORK/windows/windows-$lang-150/overlay.png" "$OUT/overlay-$lang.png"
  "$WORK/compose" install "$lang" "$OUT"
done
echo "== $OUT"
ls -l "$OUT"
