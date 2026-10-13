# Linux acceptance-gate controls

The `acceptance-gate-linux` workflow runs on `ubuntu-24.04` and builds Landrun
`811cfff51ceaf3d9843708aa6d22e9b84ccac8b4` with digest-pinned Go 1.25.7. Its
output must reproduce the tracked Linux amd64 binary digest. Public Git fetches
use no user configuration, credential helper or persisted checkout credentials.

These five fast controls require no Lean or Mathlib installation. They exercise
an actual production command, inside/outside filesystem writes, an own-loopback
TCP connection, missing/protected boundary rejection, and the attribute allowlist
and code-defining-command policy. They are not full proof or export checks and
do not certify all network protocols or descendant lifetime containment.

The pinned Landrun requests Landlock ABI **9**. A runner exposing only the older
Ubuntu kernel ABI fails closed with named `ERROR sandbox-unavailable`; the job
does not skip the control, enable best-effort, reboot a shared runner or turn an
availability error into a successful outcome. Therefore this workflow can be
red on the stock hosted image until the required kernel support is available.
Successful local validation on a qualifying Linux kernel does not qualify the
stock hosted image. This CI job is not an overall production security approval.

The branch starts from `origin/main`. Before acceptance gate v2 merges, the job
uses public commit `2468d705d53e6ebf8bf0983b9039f7ee2bb80ace` as its explicit
initial dependency. After `sandbox.py` is tracked in the checked-out revision,
the controls test that revision, not the historical dependency. No CI run is
started from the local Task Z experiment; the operator handles publication.
