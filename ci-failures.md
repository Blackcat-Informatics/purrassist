# PR #169 failed CI logs

## native (ubuntu-24.04)
https://github.com/Blackcat-Informatics/purrassist/actions/runs/41870638014/job/126716413151

/home/paudley/Active/purrassist/.worktrees/16-tool-call-recovery-streaming-recognizer/.stage/tool-call-recovery-streaming-recognizer/ci-logs/github-job-126716413151.log
152: 2026-11-04T16:52:51.4391201Z cargo fmt --all --check
153: 2026-11-04T16:52:54.1520551Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-repository.py
154: 2026-11-04T16:52:54.5123864Z     Updating crates.io index
155: 2026-11-04T16:52:54.5141056Z     Updating git repository `https://github.com/Blackcat-Informatics/purrdf.git`
228: 2026-11-04T16:53:26.4216521Z Repository attribution, frozen schema data, dependency inventory, core and derive boundaries verified.
229: 2026-11-04T16:53:26.4306005Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-repository-audit.py
230: 2026-11-04T16:53:30.5277645Z ....................................................
231: 2026-11-04T16:53:30.5278342Z ----------------------------------------------------------------------
239: 2026-11-04T16:53:30.6488481Z Toolchain pins verified: 8 stable occurrences match rust-version 1.98, 12 nightly occurrences match nightly-2026-09-14.
240: 2026-11-04T16:53:30.6571551Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py
241: 2026-11-04T16:53:31.7216927Z ...................................................................................................................
242: 2026-11-04T16:53:31.7218133Z ----------------------------------------------------------------------
264: 2026-11-04T16:53:31.7248191Z publish.yml verified: tag trigger, crates-io environment, id-token permission, qualification gate, SHA-pinned actions, dependency order, and verify-published step.
265: 2026-11-04T16:53:31.7362412Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py
266: 2026-11-04T16:53:33.4288282Z Documentation facts, links, wording and known limitations match their sources.
267: 2026-11-04T16:53:33.4403880Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-docs.py
268: 2026-11-04T16:55:50.6047208Z ....................................................................................................................................................................................................................................................................
269: 2026-11-04T16:55:50.6048567Z ----------------------------------------------------------------------
272: 2026-11-04T16:55:50.6049456Z OK
273: 2026-11-04T16:55:50.6306253Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py
274: 2026-11-04T16:57:05.8526197Z ............................................................................
275: 2026-11-04T16:57:05.8681853Z ----------------------------------------------------------------------
595: 2026-11-04T17:16:00.6501047Z 
596: 2026-11-04T17:16:00.6504839Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
597: 2026-11-04T17:16:00.6505354Z 
598: 2026-11-04T17:16:00.6626695Z 
604: 2026-11-04T17:16:00.6812541Z 
605: 2026-11-04T17:16:00.6816387Z test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
606: 2026-11-04T17:16:00.6821890Z 
607: 2026-11-04T17:16:00.6824677Z      Running unittests src/main.rs (target/debug/build/purrassist-cli/61eb6b726bedffee/out/purrassist-61eb6b726bedffee)
(69 further error matches; see saved log)

## corpus
https://github.com/Blackcat-Informatics/purrassist/actions/runs/41870638014/job/126716412579

/home/paudley/Active/purrassist/.worktrees/16-tool-call-recovery-streaming-recognizer/.stage/tool-call-recovery-streaming-recognizer/ci-logs/github-job-126716412579.log
170: 2026-11-04T16:54:34.1689683Z 
171: 2026-11-04T16:54:34.1690089Z test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 21.65s
172: 2026-11-04T16:54:34.1690513Z 
173: 2026-11-04T16:54:34.1779367Z      Running tests/corruption_oracle.rs (target/debug/build/purrassist-core/53e853f76407305f/out/corruption_oracle-53e853f76407305f)
177: 2026-11-04T16:54:34.3421650Z 
178: 2026-11-04T16:54:34.3422345Z test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s
179: 2026-11-04T16:54:34.3423026Z 
180: 2026-11-04T16:54:34.3429914Z      Running tests/markdown_corpus.rs (target/debug/build/purrassist-core/83b22452cb911fef/out/markdown_corpus-83b22452cb911fef)
186: 2026-11-04T16:54:34.3639050Z 
187: 2026-11-04T16:54:34.3639487Z test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
188: 2026-11-04T16:54:34.3640135Z 
189: 2026-11-04T16:54:34.3644079Z      Running tests/matching_oracle.rs (target/debug/build/purrassist-core/8319d771219606ba/out/matching_oracle-8319d771219606ba)
196: 2026-11-04T16:54:35.1093911Z 
197: 2026-11-04T16:54:35.1094188Z test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.74s
198: 2026-11-04T16:54:35.1094581Z 
199: 2026-11-04T16:54:35.1098054Z      Running tests/pattern_corpus.rs (target/debug/build/purrassist-core/8ce4f49095c35fb7/out/pattern_corpus-8ce4f49095c35fb7)
206: 2026-11-04T16:56:04.1801594Z 
207: 2026-11-04T16:56:04.1801862Z test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 89.07s
208: 2026-11-04T16:56:04.1802498Z 
209: 2026-11-04T16:56:04.1833673Z      Running tests/schema_conformance.rs (target/debug/build/purrassist-core/c26c0a2f9eaef859/out/schema_conformance-c26c0a2f9eaef859)
217: 2026-11-04T16:56:04.2958027Z 
218: 2026-11-04T16:56:04.2958464Z test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
219: 2026-11-04T16:56:04.2959090Z 
220: 2026-11-04T16:56:04.2962894Z      Running tests/tool_call_scale.rs (target/debug/build/purrassist-core/4e7bce20378b4209/out/tool_call_scale-4e7bce20378b4209)
227: 2026-11-04T16:57:19.2658154Z test work_independent_of_dialect_count_beyond_bound ... ok
228: 2026-11-04T16:57:49.5868550Z test one_mib_argument_fits_published_budget ... FAILED
229: 2026-11-04T16:57:52.0235179Z test one_mib_dense_markup_fits_published_budget ... FAILED
230: 2026-11-04T16:57:59.9717315Z test one_mib_argument_work_bound ... ok
231: 2026-11-04T16:57:59.9719929Z 
(6 further error matches; see saved log)

## release-gate
https://github.com/Blackcat-Informatics/purrassist/actions/runs/41870638014/job/126716412687

/home/paudley/Active/purrassist/.worktrees/16-tool-call-recovery-streaming-recognizer/.stage/tool-call-recovery-streaming-recognizer/ci-logs/github-job-126716412687.log
179: 2026-11-04T16:52:57.2047070Z cargo fmt --all --check
180: 2026-11-04T16:52:59.1135225Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-repository.py
181: 2026-11-04T16:52:59.4370346Z     Updating crates.io index
182: 2026-11-04T16:52:59.4371321Z     Updating git repository `https://github.com/Blackcat-Informatics/purrdf.git`
255: 2026-11-04T16:53:20.6355127Z Repository attribution, frozen schema data, dependency inventory, core and derive boundaries verified.
256: 2026-11-04T16:53:20.6423255Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-repository-audit.py
257: 2026-11-04T16:53:23.4372651Z ....................................................
258: 2026-11-04T16:53:23.4373283Z ----------------------------------------------------------------------
266: 2026-11-04T16:53:23.5248209Z Toolchain pins verified: 8 stable occurrences match rust-version 1.98, 12 nightly occurrences match nightly-2026-09-14.
267: 2026-11-04T16:53:23.5317717Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py
268: 2026-11-04T16:53:24.1128449Z ...................................................................................................................
269: 2026-11-04T16:53:24.1128953Z ----------------------------------------------------------------------
291: 2026-11-04T16:53:24.1144391Z publish.yml verified: tag trigger, crates-io environment, id-token permission, qualification gate, SHA-pinned actions, dependency order, and verify-published step.
292: 2026-11-04T16:53:24.1251570Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py
293: 2026-11-04T16:53:25.1736652Z Documentation facts, links, wording and known limitations match their sources.
294: 2026-11-04T16:53:25.1828880Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-docs.py
295: 2026-11-04T16:54:42.5819744Z ....................................................................................................................................................................................................................................................................
296: 2026-11-04T16:54:42.5820902Z ----------------------------------------------------------------------
299: 2026-11-04T16:54:42.5821641Z OK
300: 2026-11-04T16:54:42.6061681Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py
301: 2026-11-04T16:55:40.6795936Z ............................................................................
302: 2026-11-04T16:55:40.6796669Z ----------------------------------------------------------------------
621: 2026-11-04T17:11:13.1390675Z 
622: 2026-11-04T17:11:13.1391257Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
623: 2026-11-04T17:11:13.1391938Z      Running tests/derive_facade.rs (target/debug/deps/derive_facade-66e4ea22f22939cf)
624: 2026-11-04T17:11:13.1392302Z 
631: 2026-11-04T17:11:13.2179918Z 
632: 2026-11-04T17:11:13.2180139Z test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
633: 2026-11-04T17:11:13.2180393Z 
634: 2026-11-04T17:11:13.2184206Z      Running unittests src/main.rs (target/debug/deps/purrassist-71587bfa651a10cf)
(69 further error matches; see saved log)

## windows-engines
https://github.com/Blackcat-Informatics/purrassist/actions/runs/41870638014/job/126716412261

/home/paudley/Active/purrassist/.worktrees/16-tool-call-recovery-streaming-recognizer/.stage/tool-call-recovery-streaming-recognizer/ci-logs/github-job-126716412261.log
377: 2026-11-04T16:55:12.0848342Z 
378: 2026-11-04T16:55:12.0848817Z test result: ok. 184 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s
379: 2026-11-04T16:55:12.0849767Z 
380: 2026-11-04T16:55:12.0896853Z      Running tests\allocation_oracle.rs (target\debug\deps\allocation_oracle-830b7cbba9f72ae1.exe)
415: 2026-11-04T16:55:12.5291835Z 
416: 2026-11-04T16:55:12.5292752Z test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
417: 2026-11-04T16:55:12.5293162Z 
418: 2026-11-04T16:55:12.5327622Z      Running tests\byte_language.rs (target\debug\deps\byte_language-b3c86431de79a073.exe)
425: 2026-11-04T16:55:12.5456590Z 
426: 2026-11-04T16:55:12.5457046Z test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
427: 2026-11-04T16:55:12.5457604Z 
428: 2026-11-04T16:55:12.5476622Z      Running tests\cancellation.rs (target\debug\deps\cancellation-7d40ce94c75c4deb.exe)
434: 2026-11-04T16:55:12.5611213Z 
435: 2026-11-04T16:55:12.5611627Z test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
436: 2026-11-04T16:55:12.5612191Z 
437: 2026-11-04T16:55:12.5634903Z      Running tests\canonical_print.rs (target\debug\deps\canonical_print-27b248d05103fe72.exe)
442: 2026-11-04T16:55:12.5769755Z 
443: 2026-11-04T16:55:12.5770213Z test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
444: 2026-11-04T16:55:12.5770810Z 
445: 2026-11-04T16:55:12.5790699Z      Running tests\draft07.rs (target\debug\deps\draft07-cf7c78dd24434404.exe)
454: 2026-11-04T16:55:12.5943461Z 
455: 2026-11-04T16:55:12.5943912Z test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
456: 2026-11-04T16:55:12.5944573Z 
457: 2026-11-04T16:55:12.5966496Z      Running tests\edit_ambiguity.rs (target\debug\deps\edit_ambiguity-ff20b125f51f152a.exe)
466: 2026-11-04T16:55:12.6158872Z 
467: 2026-11-04T16:55:12.6159320Z test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
468: 2026-11-04T16:55:12.6159824Z 
469: 2026-11-04T16:55:12.6184426Z      Running tests\edits.rs (target\debug\deps\edits-7cfe45dce02ea80c.exe)
475: 2026-11-04T16:55:12.6311842Z 
476: 2026-11-04T16:55:12.6312241Z test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
477: 2026-11-04T16:55:12.6312820Z 
478: 2026-11-04T16:55:12.6335537Z      Running tests\embeddings.rs (target\debug\deps\embeddings-67184179e96d6658.exe)
(90 further error matches; see saved log)

## native (macos-14)
https://github.com/Blackcat-Informatics/purrassist/actions/runs/41870638014/job/126716413521

/home/paudley/Active/purrassist/.worktrees/16-tool-call-recovery-streaming-recognizer/.stage/tool-call-recovery-streaming-recognizer/ci-logs/github-job-126716413521.log
137: 2026-11-04T16:46:19.5674130Z cargo fmt --all --check
138: 2026-11-04T16:46:21.8460490Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-repository.py
139: 2026-11-04T16:46:22.7673410Z     Updating crates.io index
140: 2026-11-04T16:46:22.7707260Z     Updating git repository `https://github.com/Blackcat-Informatics/purrdf.git`
213: 2026-11-04T16:46:53.9406750Z Repository attribution, frozen schema data, dependency inventory, core and derive boundaries verified.
214: 2026-11-04T16:46:53.9483740Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-repository-audit.py
215: 2026-11-04T16:46:57.9504090Z ....................................................
216: 2026-11-04T16:46:57.9505200Z ----------------------------------------------------------------------
224: 2026-11-04T16:46:58.1471690Z Toolchain pins verified: 8 stable occurrences match rust-version 1.98, 12 nightly occurrences match nightly-2026-09-14.
225: 2026-11-04T16:46:58.1543130Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py
226: 2026-11-04T16:46:59.2357790Z ...................................................................................................................
227: 2026-11-04T16:46:59.2359230Z ----------------------------------------------------------------------
249: 2026-11-04T16:46:59.2490400Z publish.yml verified: tag trigger, crates-io environment, id-token permission, qualification gate, SHA-pinned actions, dependency order, and verify-published step.
250: 2026-11-04T16:46:59.2491130Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py
251: 2026-11-04T16:47:00.8398600Z Documentation facts, links, wording and known limitations match their sources.
252: 2026-11-04T16:47:00.8481230Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-docs.py
253: 2026-11-04T16:49:30.4779290Z ....................................................................................................................................................................................................................................................................
254: 2026-11-04T16:49:30.4875710Z ----------------------------------------------------------------------
257: 2026-11-04T16:49:30.5222910Z OK
258: 2026-11-04T16:49:30.5676740Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py
259: 2026-11-04T16:51:06.4448440Z ............................................................................
260: 2026-11-04T16:51:06.4451810Z ----------------------------------------------------------------------
580: 2026-11-04T17:22:45.6470080Z 
581: 2026-11-04T17:22:45.6480510Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
582: 2026-11-04T17:22:45.6490010Z 
583: 2026-11-04T17:22:45.6743900Z 
590: 2026-11-04T17:22:45.6957780Z 
591: 2026-11-04T17:22:45.6958820Z test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
592: 2026-11-04T17:22:45.6960300Z 
593: 2026-11-04T17:22:45.7228740Z 
(69 further error matches; see saved log)

