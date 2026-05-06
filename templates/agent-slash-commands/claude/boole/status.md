---
description: Check Boole node/mining wrapper status from Claude Code.
allowed-tools: Bash(__BOOLE_ROOT__/scripts/boole-agent-mine.sh:*), Bash(test:*), Bash(command:*)
---

Check whether the shared Boole agent mining wrapper is available.

Run:

```bash
test -x __BOOLE_ROOT__/scripts/boole-agent-mine.sh && __BOOLE_ROOT__/scripts/boole-agent-mine.sh --help
```

Summarize:

- wrapper path
- whether the wrapper is executable
- supported runtimes shown in help
- note that `/boole:mine` should use the shared wrapper and that consensus acceptance remains verifier/canonicalizer/node replay based
