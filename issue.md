# Issue #159: Fix rejected tool-call span boundary found by hosted fuzzing

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

Hosted fuzzing for PR #158 fails in the tool-call probe at execution 425. Current main reproduces the same assertion: a rejected attempt spans bytes 1..179 although its reported failure is byte 39, and the probe finds no valid markup-extension justification.

Diagnose the native attempt span, rejection reason, recovery markup extension and independent probe against the documented F16 contract. Correct the responsible implementation, preserve original input and rejected-span provenance, and add a bounded regression with valid neighboring cases. Do not suppress the assertion or absorb prose to make the probe pass.

Acceptance: the retained 181-byte reproducer passes the tool-call probe, existing affected rejection and content-boundary cases pass, independent source review completes, and the mandatory repository gate passes.


## Comments (0)

