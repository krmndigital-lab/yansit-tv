#!/bin/bash
# Fails if a built AAB/APK carries build-machine details (home paths, user names) in its native
# libraries or dex. Usage: tools/privacy_check.sh path/to/app.aab|.apk  [extra-pattern ...]
set -euo pipefail
pkg="$1"; shift
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
unzip -q -o "$pkg" '*.so' '*.dex' -d "$tmp" 2>/dev/null || true
pat='/home/[a-z_][a-z0-9_-]*/|/Users/[A-Za-z]|/root/\.cargo|C:\\Users\\'
for extra in "$@"; do pat="$pat|$extra"; done
hits=0
while IFS= read -r -d '' f; do
  n=$(strings -n 6 "$f" | grep -c -E "$pat" || true)
  if [ "$n" != 0 ]; then
    echo "LEAK: $n string(s) in ${f#"$tmp"/}:"; strings -n 6 "$f" | grep -o -E ".{0,40}($pat).{0,40}" | head -3
    hits=$((hits + n))
  fi
done < <(find "$tmp" -type f \( -name '*.so' -o -name '*.dex' \) -print0)
[ "$hits" = 0 ] && echo "privacy check OK: no build-machine paths in $(basename "$pkg")" || exit 1
