---
name: debug
description: Systematic debugging: reproduction, hypothesis generation, root cause isolation, and regression verification.
---

# Debug Skill

## Scientific Debugging Workflow
1. **Reproduce First:** Formulate a minimal reproduction command or test case showing the failure before editing any source code.
2. **Isolate Root Cause:** Trace the bug to its source; do not merely patch symptoms or suppress error messages.
3. **Smallest Correct Patch:** Make the minimal targeted edit that resolves the issue across all valid input domains.
4. **Run Verification:** Re-run reproduction and the full package test suite to confirm zero regressions.
