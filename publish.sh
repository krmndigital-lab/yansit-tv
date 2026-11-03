#!/usr/bin/env bash
# Local one-command publish (CI does the same on a vX.Y.Z tag, see .github/workflows/release.yml).
#   ./publish.sh check                       what Play has now
#   ./publish.sh all --track alpha           build if needed, then listing + products + release
#   ./publish.sh release --track production  just the AAB
#   ./publish.sh promote --from alpha --to production
# All arguments go to tools/play/publish.py (see its --help). Key: ~/.config/krmn/play-service-account.json
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AAB="$ROOT/android/build/gradle/outputs/bundle/release/Yansit-release.aab"
case "${1:-}" in
  release|all)
    if [ ! -f "$AAB" ] || [ -n "$(find "$ROOT/src" "$ROOT/android/java" "$ROOT/android/res" "$ROOT/Cargo.toml" -newer "$AAB" -print -quit)" ]; then
      echo "== building signed AAB"
      (cd "$ROOT/android" && ./gradlew --no-daemon -q bundleRelease)
    fi
    bash "$ROOT/tools/privacy_check.sh" "$AAB" ;;
esac
exec python3 "$ROOT/tools/play/publish.py" "$@"
