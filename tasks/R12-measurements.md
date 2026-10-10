# R12 measurements (G20, "Raise the published constants")

Method: a throwaway test (deleted) recovered each shape under the default
dialect set (all 12) and the 3-tool test set, with work/scratch/candidates/
evidence limits lifted to read the true cost, then again under plain
`sized(Effort::Standard, output)` at the old constants (32 work, 48 scratch)
to see which resource bound. Release build for the sweep; the final
assertions run in the test profile (`tool_call_scale::one_mib_dense...`,
19 s; `tool_call_cost::dense_markup...`, 1 s). Charges are deterministic.

## 1 MiB (units = calls or array items; per byte)

| Shape | Bytes | Units | Work | Peak scratch | Evidence | Old `sized` stop |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| qwen nested items | 1,048,534 | 16,911 | 59.0 | 268.3 | 0.065 (67.6k) | Work |
| qwen JSON-array items | 1,048,544 | 30,838 | 52.3 | 103.6 | 0 | Work |
| qwen string items | 1,048,552 | 34,950 | 61.4 | 250.5 | 0.067 | Work |
| qwen calls | 1,048,509 | 11,781 | 71.3 | 281.6 | 0.101 (106k) | Work |
| hermes calls | 1,048,506 | 14,169 | 76.8 | 114.1 | 0.014 | Work |
| openai stringified calls | 1,048,500 | 10,485 | 67.7 | 183.0 | 0.070 | Work |
| mistral-v3 (section per call) | 1,048,539 | 12,633 | 66.9 | 160.6 | 0.060 | Work |
| mistral-v11 (`</s>` per call) | 1,048,544 | 16,912 | 56.0 | 203.7 | 0.081 | Work |
| harmony calls | 1,048,506 | 9,446 | 115.4 | 80.1 | 0.009 | Work |
| deepseek-v3.1 calls | 1,048,556 | 11,522 | 70.0 | 86.7 | 0.011 | Work |
| deepseek-v3 calls | 1,048,486 | 9,361 | 79.7 | 291.8 (292.8 in sweep) | 0.045 | ScratchBytes |
| kimi-k2 calls | 1,048,527 | 10,809 | 138.3 | 131.4 | 0.052 | Work |
| llama3 calls | 1,048,550 | 20,971 | 69.1 | 150.1 | 0.020 | Work |
| anthropic calls | 1,048,575 | 13,981 | 51.6 | 170.0 | 0.067 | Work |
| bare-json calls (`{"name":..,"arguments":{..}}`) | 1,048,551 | 21,399 | 70.2 | 153.0 | 0.020 | Work |
| bare-json empty-argument calls | 1,048,560 | 34,952 | 95.1 | 165.6 | 0.033 | Work |
| qwen single 1 MiB string | 1,048,663 | 1 | 17.0 | 11.1 | ~0 | Completed |

Maximum: work 138.3 (kimi), peak scratch 291.8-292.8 (deepseek-v3), evidence
0.101 (qwen calls). Per-byte values at 4 KiB / 64 KiB / 1 MiB are the same to
within 1% for work; peak scratch is higher at 4 KiB (up to 318.5 deepseek-v3,
139.3 work kimi) because of fixed overhead, which the preset's 1M work /
64 MiB scratch covers.

Binding order at the old constants: Work, then EvidenceEntries (preset flat
65,536; exceeded by nested/string items, qwen calls, openai, v11, anthropic),
then ScratchBytes. Candidates never bound (sized term grows with openers;
usage was far below it). Depth is not per byte (preset 128; shapes need <= 4).
Old plain-sized results for the five worst: nested/string/qwen calls hit
Work at ~1,000-1,500 units per MiB.

## Chosen constants

- RECOVER_WORK_PER_BYTE 32 -> 176 (138.3 x 1.273)
- RECOVER_SCRATCH_PER_BYTE 48 -> 368 (292.8 x 1.257)
- RECOVER_EVIDENCE_BYTES_PER_ENTRY (new) 7: sized adds ceil(n/7) evidence
  entries (0.101 -> 0.143 per byte, 41% margin)
- Candidates (opener term) and depth unchanged: not binding.

## Not linear / outside the covered population

- Mistral v11 consecutive calls with only whitespace between them: one
  `next`-chained attempt, segmentation re-run per call, superlinear work AND
  scratch. `[TOOL_CALLS]write[ARGS]{..}\n` x N: 969 B (17 calls) 68.9 work,
  370 scratch/byte; 8,151 B 94.4 / 675; 16 KiB (409 calls, newline form)
  140.9 / 2,009; 262 KiB (6,553 calls) 1,139 / 27,354 (65 s release).
  `</s>` after each call restores 56 / 203 linear (4 KiB to 1 MiB).
  Cause located: `ArgTree` copies at engine.rs `fields()` (17,860 value
  clones for 143 calls): each reported prefix of the chain re-segments the
  whole chain (`segment` -> `part_readings` -> `reading`). Fix needs
  incremental segmentation in the engine; not a constants issue.
- Mistral v3 array with thousands of calls in one `[TOOL_CALLS] [..]`:
  charges linear (81 work, 258 scratch per byte) but wall time quadratic
  (16 KiB 0.18 s, 32 KiB 0.52 s, 64 KiB 1.8 s, release); 1 MiB did not
  finish in 300 s.
- Non-call adversarial text: `{` x 1 MiB = 895 work/byte (linear in n, 7.7 s
  at 256 KiB release), `{"a":` x N = 195 work/byte; both explicit
  BudgetExceeded(Work) under sized.
