#!/usr/bin/env bash
# Linux-аналог fetch-engine.ps1: скачивает движки в src-tauri/resources/engine.
#   z1/    — nfqws (zapret1, bol-van/zapret) + стратегии и фейки Flowseal
#   z2/    — nfqws2 (zapret2) + lua + фейки
#   lists/ — списки доменов и IP
#
# Архитектура: ./fetch-engine.sh [x86_64|arm64], по умолчанию — как у этой машины.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${OUT:-$ROOT/src-tauri/resources/engine}"
case "${1:-$(uname -m)}" in
  x86_64|amd64) ARCH=linux-x86_64 ;;
  aarch64|arm64) ARCH=linux-arm64 ;;
  *) echo "неизвестная архитектура: ${1:-$(uname -m)}" >&2; exit 1 ;;
esac

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

latest() { # repo → JSON последнего релиза
  curl -fsSL -H "User-Agent: prosvet-build" ${GITHUB_TOKEN:+-H "Authorization: Bearer $GITHUB_TOKEN"} \
    "https://api.github.com/repos/$1/releases/latest"
}
field() { python3 -c "import sys,json; d=json.load(sys.stdin); print($1)"; }

rm -rf "$OUT"
mkdir -p "$OUT/z1/strategies" "$OUT/z2/lua" "$OUT/z2/fake" "$OUT/lists"

# --- Flowseal: стратегии, списки, фейки ---
FS="$(latest Flowseal/zapret-discord-youtube)"
FS_TAG="$(field "d['tag_name']" <<<"$FS")"
FS_URL="$(field "[a['browser_download_url'] for a in d['assets'] if a['name'].endswith('.zip')][0]" <<<"$FS")"
echo "Flowseal $FS_TAG"
curl -fsSL "$FS_URL" -o "$TMP/fs.zip"
unzip -q "$TMP/fs.zip" -d "$TMP/fs"
FS_ROOT="$(dirname "$(find "$TMP/fs" -name service.bat | head -n1)")"
cp "$FS_ROOT"/bin/*.bin "$OUT/z1/"
cp "$FS_ROOT"/general*.bat "$OUT/z1/strategies/"
cp "$FS_ROOT"/lists/* "$OUT/lists/"

# --- zapret1: nfqws для стратегий Flowseal ---
Z1="$(latest bol-van/zapret)"
Z1_TAG="$(field "d['tag_name']" <<<"$Z1")"
Z1_URL="$(field "[a['browser_download_url'] for a in d['assets'] if a['name'].startswith('zapret-v') and a['name'].endswith('.zip') and 'openwrt' not in a['name']][0]" <<<"$Z1")"
echo "zapret $Z1_TAG"
curl -fsSL "$Z1_URL" -o "$TMP/z1.zip"
unzip -q "$TMP/z1.zip" -d "$TMP/z1"
cp "$TMP"/z1/*/binaries/$ARCH/nfqws "$OUT/z1/"

# --- zapret2 ---
Z2="$(latest bol-van/zapret2)"
Z2_TAG="$(field "d['tag_name']" <<<"$Z2")"
Z2_URL="$(field "[a['browser_download_url'] for a in d['assets'] if a['name'].startswith('zapret2-v') and a['name'].endswith('.zip')][0]" <<<"$Z2")"
echo "zapret2 $Z2_TAG"
curl -fsSL "$Z2_URL" -o "$TMP/z2.zip"
unzip -q "$TMP/z2.zip" -d "$TMP/z2"
Z2_ROOT="$(find "$TMP/z2" -mindepth 1 -maxdepth 1 -type d | head -n1)"
cp "$Z2_ROOT/binaries/$ARCH/nfqws2" "$OUT/z2/"
cp "$Z2_ROOT"/lua/*.lua "$OUT/z2/lua/"
cp "$Z2_ROOT"/files/fake/* "$OUT/z2/fake/"
# фейки Flowseal тоже пригодятся стратегиям zapret2
cp -f "$OUT"/z1/*.bin "$OUT/z2/fake/"

chmod +x "$OUT/z1/nfqws" "$OUT/z2/nfqws2"
printf '{\n  "flowseal": "%s",\n  "zapret2": "%s",\n  "zapret1": "%s"\n}\n' "$FS_TAG" "$Z2_TAG" "$Z1_TAG" > "$OUT/version.json"
echo "OK -> $OUT ($ARCH)"
