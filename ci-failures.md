# PR #171 failed CI logs

## release-gate
https://github.com/Blackcat-Informatics/purrassist/actions/runs/41851753761/job/126649603660

/home/paudley/Active/purrassist/.worktrees/170-destroy-scratch-buffers-before/.stage/destroy-scratch-buffers-before/ci-logs/github-job-126649603660.log
165: 2026-11-04T15:02:38.0983235Z cargo fmt --all --check
166: 2026-11-04T15:02:40.6160739Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-repository.py
167: 2026-11-04T15:02:41.0493575Z     Updating crates.io index
168: 2026-11-04T15:02:41.0495985Z     Updating git repository `https://github.com/Blackcat-Informatics/purrdf.git`
239: 2026-11-04T15:03:09.7991683Z Repository attribution, frozen schema data, dependency inventory, core and derive boundaries verified.
240: 2026-11-04T15:03:09.8065564Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-repository-audit.py
241: 2026-11-04T15:03:14.7080288Z .....................................................
242: 2026-11-04T15:03:14.7082212Z ----------------------------------------------------------------------
250: 2026-11-04T15:03:14.8226288Z Toolchain pins verified: 8 stable occurrences match rust-version 1.98, 12 nightly occurrences match nightly-2026-09-14.
251: 2026-11-04T15:03:14.8314010Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py
252: 2026-11-04T15:03:17.1263789Z ...................................................................................................................
253: 2026-11-04T15:03:17.1265019Z ----------------------------------------------------------------------
275: 2026-11-04T15:03:17.1288357Z publish.yml verified: tag trigger, crates-io environment, id-token permission, qualification gate, SHA-pinned actions, dependency order, and verify-published step.
276: 2026-11-04T15:03:17.1413603Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py
277: 2026-11-04T15:03:18.5540024Z Documentation facts, links, wording and known limitations match their sources.
278: 2026-11-04T15:03:18.5651775Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-docs.py
279: 2026-11-04T15:04:57.9683128Z ....................................................................................................................................................................................................................................................................
280: 2026-11-04T15:04:57.9684656Z ----------------------------------------------------------------------
283: 2026-11-04T15:04:57.9686092Z OK
284: 2026-11-04T15:04:57.9951204Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py
285: 2026-11-04T15:06:13.4616654Z ............................................................................
286: 2026-11-04T15:06:13.4617616Z ----------------------------------------------------------------------
604: 2026-11-04T15:27:31.2919301Z 
605: 2026-11-04T15:27:31.2919747Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
606: 2026-11-04T15:27:31.2920111Z 
607: 2026-11-04T15:27:31.4575472Z 
613: 2026-11-04T15:27:31.5358571Z 
614: 2026-11-04T15:27:31.5358964Z test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
615: 2026-11-04T15:27:31.5359294Z 
616: 2026-11-04T15:27:31.5367860Z      Running unittests src/main.rs (target/debug/deps/purrassist-ce71a453a0256c74)
(55 further error matches; see saved log)

## native (macos-14)
https://github.com/Blackcat-Informatics/purrassist/actions/runs/41851753761/job/126649603463

/home/paudley/Active/purrassist/.worktrees/170-destroy-scratch-buffers-before/.stage/destroy-scratch-buffers-before/ci-logs/github-job-126649603463.log
137: 2026-11-04T14:55:51.3731890Z cargo fmt --all --check
138: 2026-11-04T14:55:53.1229760Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-repository.py
139: 2026-11-04T14:55:53.7361990Z     Updating crates.io index
140: 2026-11-04T14:55:53.7376030Z     Updating git repository `https://github.com/Blackcat-Informatics/purrdf.git`
211: 2026-11-04T14:56:19.0477160Z Repository attribution, frozen schema data, dependency inventory, core and derive boundaries verified.
212: 2026-11-04T14:56:19.0521350Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-repository-audit.py
213: 2026-11-04T14:56:21.9384410Z .....................................................
214: 2026-11-04T14:56:21.9385160Z ----------------------------------------------------------------------
222: 2026-11-04T14:56:22.0499890Z Toolchain pins verified: 8 stable occurrences match rust-version 1.98, 12 nightly occurrences match nightly-2026-09-14.
223: 2026-11-04T14:56:22.0554080Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py
224: 2026-11-04T14:56:22.8496020Z ...................................................................................................................
225: 2026-11-04T14:56:22.8496810Z Release gate identity verified: release qualification and the Verify release-gate job both run `make release-gate` on the same toolchains and environment.
247: 2026-11-04T14:56:22.8512840Z publish.yml verified: tag trigger, crates-io environment, id-token permission, qualification gate, SHA-pinned actions, dependency order, and verify-published step.
248: 2026-11-04T14:56:22.8575790Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py
249: 2026-11-04T14:56:23.9189200Z Documentation facts, links, wording and known limitations match their sources.
250: 2026-11-04T14:56:23.9292120Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-docs.py
251: 2026-11-04T14:58:16.2809580Z ....................................................................................................................................................................................................................................................................
252: 2026-11-04T14:58:16.2810280Z ----------------------------------------------------------------------
255: 2026-11-04T14:58:16.2810800Z OK
256: 2026-11-04T14:58:16.2981390Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py
257: 2026-11-04T14:59:14.6152920Z ............................................................................
258: 2026-11-04T14:59:14.6153350Z ----------------------------------------------------------------------
575: 2026-11-04T15:13:51.3608590Z 
576: 2026-11-04T15:13:51.3609330Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
577: 2026-11-04T15:13:51.3609710Z 
578: 2026-11-04T15:13:51.3611180Z      Running tests/derive_facade.rs (target/debug/build/purrassist/27353494c236c084/out/derive_facade-27353494c236c084)
585: 2026-11-04T15:13:51.3866990Z 
586: 2026-11-04T15:13:51.3867530Z test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
587: 2026-11-04T15:13:51.3868380Z      Running unittests src/main.rs (target/debug/build/purrassist-cli/e756812f811df9d8/out/purrassist-e756812f811df9d8)
588: 2026-11-04T15:13:51.3868860Z 
(55 further error matches; see saved log)

## native (ubuntu-24.04)
https://github.com/Blackcat-Informatics/purrassist/actions/runs/41851753761/job/126649603067

/home/paudley/Active/purrassist/.worktrees/170-destroy-scratch-buffers-before/.stage/destroy-scratch-buffers-before/ci-logs/github-job-126649603067.log
152: 2026-11-04T14:55:42.3300788Z cargo fmt --all --check
153: 2026-11-04T14:55:44.9116965Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-repository.py
154: 2026-11-04T14:55:45.2931900Z     Updating crates.io index
155: 2026-11-04T14:55:45.2935613Z     Updating git repository `https://github.com/Blackcat-Informatics/purrdf.git`
226: 2026-11-04T14:56:16.6552660Z Repository attribution, frozen schema data, dependency inventory, core and derive boundaries verified.
227: 2026-11-04T14:56:16.6644043Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-repository-audit.py
228: 2026-11-04T14:56:20.6111469Z .....................................................
229: 2026-11-04T14:56:20.6112465Z ----------------------------------------------------------------------
237: 2026-11-04T14:56:20.7398112Z Toolchain pins verified: 8 stable occurrences match rust-version 1.98, 12 nightly occurrences match nightly-2026-09-14.
238: 2026-11-04T14:56:20.7503614Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py
239: 2026-11-04T14:56:21.7348448Z ...................................................................................................................
240: 2026-11-04T14:56:21.7349432Z ----------------------------------------------------------------------
262: 2026-11-04T14:56:21.7367688Z publish.yml verified: tag trigger, crates-io environment, id-token permission, qualification gate, SHA-pinned actions, dependency order, and verify-published step.
263: 2026-11-04T14:56:21.7490239Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py
264: 2026-11-04T14:56:23.4435274Z Documentation facts, links, wording and known limitations match their sources.
265: 2026-11-04T14:56:23.4597585Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-docs.py
266: 2026-11-04T14:58:33.1457368Z ....................................................................................................................................................................................................................................................................
267: 2026-11-04T14:58:33.1458318Z ----------------------------------------------------------------------
270: 2026-11-04T14:58:33.1459138Z OK
271: 2026-11-04T14:58:33.1803420Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py
272: 2026-11-04T14:59:49.0647807Z ............................................................................
273: 2026-11-04T14:59:49.0648292Z ----------------------------------------------------------------------
591: 2026-11-04T15:19:54.2347811Z 
592: 2026-11-04T15:19:54.2352580Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
593: 2026-11-04T15:19:54.2354185Z 
594: 2026-11-04T15:19:54.2431355Z 
600: 2026-11-04T15:19:54.2536980Z 
601: 2026-11-04T15:19:54.2542590Z test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
602: 2026-11-04T15:19:54.2548072Z 
603: 2026-11-04T15:19:54.2552710Z      Running unittests src/main.rs (target/debug/build/purrassist-cli/0000e384d8c8b783/out/purrassist-0000e384d8c8b783)
(55 further error matches; see saved log)

## windows-engines
https://github.com/Blackcat-Informatics/purrassist/actions/runs/41851753761/job/126649603086

/home/paudley/Active/purrassist/.worktrees/170-destroy-scratch-buffers-before/.stage/destroy-scratch-buffers-before/ci-logs/github-job-126649603086.log
376: 2026-11-04T15:02:39.8003980Z 
377: 2026-11-04T15:02:39.8004393Z test result: ok. 183 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s
378: 2026-11-04T15:02:39.8004883Z 
379: 2026-11-04T15:02:39.8037422Z      Running tests\allocation_oracle.rs (target\debug\deps\allocation_oracle-830b7cbba9f72ae1.exe)
414: 2026-11-04T15:02:40.1945567Z 
415: 2026-11-04T15:02:40.1945986Z test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.38s
416: 2026-11-04T15:02:40.1946546Z 
417: 2026-11-04T15:02:40.1978840Z      Running tests\byte_language.rs (target\debug\deps\byte_language-b3c86431de79a073.exe)
424: 2026-11-04T15:02:40.2086669Z 
425: 2026-11-04T15:02:40.2087032Z test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
426: 2026-11-04T15:02:40.2087506Z 
427: 2026-11-04T15:02:40.2104323Z      Running tests\cancellation.rs (target\debug\deps\cancellation-7d40ce94c75c4deb.exe)
433: 2026-11-04T15:02:40.2202700Z 
434: 2026-11-04T15:02:40.2203070Z test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
435: 2026-11-04T15:02:40.2203529Z 
436: 2026-11-04T15:02:40.2217614Z      Running tests\canonical_print.rs (target\debug\deps\canonical_print-27b248d05103fe72.exe)
441: 2026-11-04T15:02:40.2335747Z 
442: 2026-11-04T15:02:40.2336023Z test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
443: 2026-11-04T15:02:40.2336287Z 
444: 2026-11-04T15:02:40.2354248Z      Running tests\draft07.rs (target\debug\deps\draft07-cf7c78dd24434404.exe)
453: 2026-11-04T15:02:40.2472274Z 
454: 2026-11-04T15:02:40.2472669Z test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
455: 2026-11-04T15:02:40.2473224Z 
456: 2026-11-04T15:02:40.2490757Z      Running tests\edit_ambiguity.rs (target\debug\deps\edit_ambiguity-ff20b125f51f152a.exe)
465: 2026-11-04T15:02:40.2678468Z 
466: 2026-11-04T15:02:40.2678921Z test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
467: 2026-11-04T15:02:40.2679491Z 
468: 2026-11-04T15:02:40.2699736Z      Running tests\edits.rs (target\debug\deps\edits-7cfe45dce02ea80c.exe)
474: 2026-11-04T15:02:40.2795773Z 
475: 2026-11-04T15:02:40.2796195Z test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
476: 2026-11-04T15:02:40.2796746Z 
477: 2026-11-04T15:02:40.2809881Z      Running tests\embeddings.rs (target\debug\deps\embeddings-67184179e96d6658.exe)
(112 further error matches; see saved log)

## corpus
https://github.com/Blackcat-Informatics/purrassist/actions/runs/41851753761/job/126649602931

/home/paudley/Active/purrassist/.worktrees/170-destroy-scratch-buffers-before/.stage/destroy-scratch-buffers-before/ci-logs/github-job-126649602931.log
170: 2026-11-04T14:56:05.2327267Z 
171: 2026-11-04T14:56:05.2327726Z test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 25.42s
172: 2026-11-04T14:56:05.2328344Z 
173: 2026-11-04T14:56:05.2409030Z      Running tests/corruption_oracle.rs (target/debug/build/purrassist-core/53e853f76407305f/out/corruption_oracle-53e853f76407305f)
177: 2026-11-04T14:56:05.4103616Z 
178: 2026-11-04T14:56:05.4104064Z test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s
179: 2026-11-04T14:56:05.4104649Z 
180: 2026-11-04T14:56:05.4109437Z      Running tests/markdown_corpus.rs (target/debug/build/purrassist-core/83b22452cb911fef/out/markdown_corpus-83b22452cb911fef)
186: 2026-11-04T14:56:05.4256504Z 
187: 2026-11-04T14:56:05.4256939Z test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
188: 2026-11-04T14:56:05.4257526Z 
189: 2026-11-04T14:56:05.4260365Z      Running tests/matching_oracle.rs (target/debug/build/purrassist-core/8319d771219606ba/out/matching_oracle-8319d771219606ba)
196: 2026-11-04T14:56:06.1519553Z 
197: 2026-11-04T14:56:06.1520287Z test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.72s
198: 2026-11-04T14:56:06.1521183Z 
199: 2026-11-04T14:56:06.1523585Z      Running tests/pattern_corpus.rs (target/debug/build/purrassist-core/8ce4f49095c35fb7/out/pattern_corpus-8ce4f49095c35fb7)
206: 2026-11-04T14:57:38.3303213Z 
207: 2026-11-04T14:57:38.3304052Z test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 92.18s
208: 2026-11-04T14:57:38.3304708Z 
209: 2026-11-04T14:57:38.3338125Z      Running tests/schema_conformance.rs (target/debug/build/purrassist-core/c26c0a2f9eaef859/out/schema_conformance-c26c0a2f9eaef859)
217: 2026-11-04T14:57:38.4605702Z 
218: 2026-11-04T14:57:38.4606338Z test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
219: 2026-11-04T14:57:38.4606999Z 
220: 2026-11-04T14:57:38.4611193Z      Running tests/tool_call_scale.rs (target/debug/build/purrassist-core/4e7bce20378b4209/out/tool_call_scale-4e7bce20378b4209)
222: 2026-11-04T14:57:38.4638905Z running 4 tests
223: 2026-11-04T14:57:58.6087036Z test one_mib_argument_fits_published_budget ... FAILED
224: 2026-11-04T14:58:32.3307031Z test work_independent_of_dialect_count_beyond_bound ... ok
225: 2026-11-04T14:58:38.4657072Z test one_mib_argument_work_bound has been running for over 60 seconds
226: 2026-11-04T14:58:38.4732295Z test one_mib_dense_markup_fits_published_budget has been running for over 60 seconds
227: 2026-11-04T14:59:08.7048946Z test one_mib_dense_markup_fits_published_budget ... FAILED
228: 2026-11-04T14:59:18.1605075Z test one_mib_argument_work_bound ... ok
229: 2026-11-04T14:59:18.1605450Z 
(10 further error matches; see saved log)

## fuzz
https://github.com/Blackcat-Informatics/purrassist/actions/runs/41851753761/job/126649602560

/home/paudley/Active/purrassist/.worktrees/170-destroy-scratch-buffers-before/.stage/destroy-scratch-buffers-before/ci-logs/github-job-126649602560.log
157: 2026-11-04T14:54:10.1148889Z ##[endgroup]
158: 2026-11-04T14:54:10.1205302Z python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py
159: 2026-11-04T14:55:23.3976571Z ............................................................................
160: 2026-11-04T14:55:23.3977082Z ----------------------------------------------------------------------
296: 2026-11-04T15:32:34.6609737Z 
297: 2026-11-04T15:32:34.6609979Z thread 'main' (79937) panicked at crates/core/examples/fuzz_tool_call.rs:1242:9:
298: 2026-11-04T15:32:34.6610511Z rejected attempt 22..77 runs past where it failed (53) without its closer: "a\"), list_dir(path=\".\")]"
299: 2026-11-04T15:32:34.6611016Z note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
300: 2026-11-04T15:32:34.6611704Z Failure retained in /home/runner/work/purrassist/purrassist/.cache/fuzz-tool-call-k3kjobzk/current.bin
310: 2026-11-04T15:32:34.7684400Z     raise CalledProcessError(self.returncode, self.args, self.stdout,
311: 2026-11-04T15:32:34.7685152Z subprocess.CalledProcessError: Command '['/home/runner/work/purrassist/purrassist/.cache/fuzz-tool-call-k3kjobzk/probe']' returned non-zero exit status 101.
312: 2026-11-04T15:32:34.7974721Z make: *** [Makefile:92: fuzz-hosted] Error 1
313: 2026-11-04T15:32:34.7988870Z ##[error]Process completed with exit code 2.
314: 2026-11-04T15:32:34.8076166Z Post job cleanup.
315: 2026-11-04T15:32:34.8667259Z [command]/usr/bin/git version

