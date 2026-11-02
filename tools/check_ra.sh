#!/usr/bin/env bash
# rust-analyzer diagnostics gate (editor parity, not clippy parity).
#
# The CLI analyses the NON-test cfg, so every `mod tests` block reports
# `inactive_code: code is inactive due to #[cfg] directives: test is disabled`.
# That is a property of the CLI, not of the code (the editor loads all targets, so a
# human never sees it), so those lines are filtered. Anything else is a real finding.
#
# Also note: the write-time lint hook on agent file writes is a syntax check, not
# rust-analyzer - a hard type error written through it reports "lint: ok". This script
# is what actually catches that class locally.
set -uo pipefail
cd "$(dirname "$0")/.."
out=$(rust-analyzer diagnostics . 2>/dev/null)
printf '%s\n' "$out" | grep -v "inactive_code" | grep -vE "^\s*$" | grep -v "diagnostic scan complete" > /tmp/ra-findings.txt || true
n=$(wc -l < /tmp/ra-findings.txt)
if [ "$n" -ne 0 ]; then
    echo "rust-analyzer findings ($n):"
    cat /tmp/ra-findings.txt
    exit 1
fi
echo "rust-analyzer: clean ($(printf '%s\n' "$out" | grep -c inactive_code) inactive_code artifacts filtered)"
