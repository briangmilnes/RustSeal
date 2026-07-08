#!/bin/bash
# verus-lab "build" == verify. Delegates to validate.sh (verus has no separate
# build step). Logs via validate.sh to logs/.
set -uo pipefail
CRATE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec "$CRATE_ROOT/scripts/validate.sh" "$@"
