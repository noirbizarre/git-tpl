# Changelog

All notable changes to this project will be documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.12.1](https://github.com/noirbizarre/git-tpl/compare/0.12.0..0.12.1) - 2026-08-30

### 🐛 Bug Fixes

- **theme** Honour FORCE_COLOR, not just CLICOLOR_FORCE ([#149](https://github.com/noirbizarre/git-tpl/issues/149)) - ([4e35e68](https://github.com/noirbizarre/git-tpl/commit/4e35e682aafcd9cd51c53f7c66c709cea75d9ca9))

## [0.12.0](https://github.com/noirbizarre/git-tpl/compare/0.11.0..0.12.0) - 2026-08-30

### 🐛 Bug Fixes

- **jj** Diagnose non-colocated Jujutsu workspaces, document compatibility ([#145](https://github.com/noirbizarre/git-tpl/issues/145)) - ([59e2c8f](https://github.com/noirbizarre/git-tpl/commit/59e2c8ffb53d50f1adb32620016e627194b7c422))
- **status**  🚨 **breaking** Rename JSON fields to match the reference/description naming convention - ([0c88996](https://github.com/noirbizarre/git-tpl/commit/0c889964b9a33bba5e9e440b1373a74bc9f03e3a))
- **testing** Stop `git tpl test`'s progress line from printing twice ([#148](https://github.com/noirbizarre/git-tpl/issues/148)) - ([675a6c2](https://github.com/noirbizarre/git-tpl/commit/675a6c28c36e286b80b1c868e8d45b1211074221))
- **testing**  🚨 **breaking** --write only records a snapshot, not a case ([#146](https://github.com/noirbizarre/git-tpl/issues/146)) - ([b54554b](https://github.com/noirbizarre/git-tpl/commit/b54554b1d2846439cf64d3961891cd707eb0bda8))
- **testing** Colour +/- lines in git tpl test's snapshot diff ([#142](https://github.com/noirbizarre/git-tpl/issues/142)) - ([2088e25](https://github.com/noirbizarre/git-tpl/commit/2088e2505e152c5c43b1618165b7508366d79f91))

### 📚 Documentation

- **config** Document tpl.testCommands - ([17644bc](https://github.com/noirbizarre/git-tpl/commit/17644bc91d89e0598670ec29c6146a69badb61da))
- **skill** Document merge --abort's aborted result - ([0d5f8c4](https://github.com/noirbizarre/git-tpl/commit/0d5f8c45f695bebdfd4e742ddb89861d588b3daa))
- **skill** Refresh the agent skill and guard against future drift ([#143](https://github.com/noirbizarre/git-tpl/issues/143)) - ([cf83e04](https://github.com/noirbizarre/git-tpl/commit/cf83e047400b4a5b76269f1de96ccac1a381fc09))

### 🧪 Tests

- **cli** Drop redundant test_ prefix from test names - ([bffc605](https://github.com/noirbizarre/git-tpl/commit/bffc605bd9ff17ddead6cf839e0d6473e425a38e))

## [0.11.0](https://github.com/noirbizarre/git-tpl/compare/0.10.0..0.11.0) - 2026-08-30

### 💫 Features

- **status** Add availableCommit alongside availableRevision - ([b8dab25](https://github.com/noirbizarre/git-tpl/commit/b8dab2518746970fd44bd0f99ca0b36aa649446e))
- **testing**  🚨 **breaking** Show live progress and colour throughout `git tpl test` ([#138](https://github.com/noirbizarre/git-tpl/issues/138)) - ([0de4b7e](https://github.com/noirbizarre/git-tpl/commit/0de4b7e3be211b38d2efcb4a7ee3d93873c1e5b5))
- **testing** Expose the resolved template's root to a case's commands ([#140](https://github.com/noirbizarre/git-tpl/issues/140)) - ([59dc1dc](https://github.com/noirbizarre/git-tpl/commit/59dc1dceb2158e3d046be1cefe12eaebc7a35a7d))
- **testing**  🚨 **breaking** Test the working tree by default, refuse a remote source ([#137](https://github.com/noirbizarre/git-tpl/issues/137)) - ([0c9416b](https://github.com/noirbizarre/git-tpl/commit/0c9416b799122ef14437d196ffe2efadb656da1b))
- **testing** Scope environment variables to a case's commands ([#133](https://github.com/noirbizarre/git-tpl/issues/133)) - ([6d6f026](https://github.com/noirbizarre/git-tpl/commit/6d6f026a0d89a78b4dc494cd74676ed495f1dadf))

### 🐛 Bug Fixes

- **data** Describe the revision, not just the commit, in "no such file" errors - ([b6a6781](https://github.com/noirbizarre/git-tpl/commit/b6a6781303bd78426fe7fa6d7752700955478cd2))
- **dist** Pass the completion shell as a positional argument ([#128](https://github.com/noirbizarre/git-tpl/issues/128)) - ([3b31c75](https://github.com/noirbizarre/git-tpl/commit/3b31c7537f505341f938715e82dbc393bcda153b))
- **report**  🚨 **breaking** Use a consistent object shape for the JSON revision field - ([36eb506](https://github.com/noirbizarre/git-tpl/commit/36eb506db95867dc582b73f90c731ada1749ece2))
- **testing**  🚨 **breaking** Validate a test case's [answers] strictly, unconditionally ([#136](https://github.com/noirbizarre/git-tpl/issues/136)) - ([aa15446](https://github.com/noirbizarre/git-tpl/commit/aa154466ffaf09aa96f725c7998328a2fe27fed1))

### 🔨 Refactor

- **testing** Rename TestError to TestingError - ([09f2649](https://github.com/noirbizarre/git-tpl/commit/09f26499a60191d2f8c5200e4ea3792dc7ea254c))

### 📚 Documentation

- **refs** Explain why TemplateIdError breaks the file-name convention - ([28ccd0f](https://github.com/noirbizarre/git-tpl/commit/28ccd0f64022601a10389de47763add0f06aa690))
- **setup** Match the update invariant's wording to AGENTS.md - ([fe2948f](https://github.com/noirbizarre/git-tpl/commit/fe2948ffb4ed5b317b264349a34912316d39eb94))
- **template** Document yaml as a supported data source format - ([6aa19d1](https://github.com/noirbizarre/git-tpl/commit/6aa19d160e943d3f6892a737807aa5a1613d7255))
- Document tpl::data::needs_project for lint and context - ([b9fe17f](https://github.com/noirbizarre/git-tpl/commit/b9fe17ffcff3d0a8c30633b1175706eff17124a5))
- Mark the recorded-answers precedence step update-only - ([56b8566](https://github.com/noirbizarre/git-tpl/commit/56b8566a263dbae20361e79b1969527bf7678d44))
- Decline `git tpl detach`, document the manual path ([#139](https://github.com/noirbizarre/git-tpl/issues/139)) - ([33ff72f](https://github.com/noirbizarre/git-tpl/commit/33ff72fb15d6c36fa6cd3678ad516e7c170ef461))

## [0.10.0](https://github.com/noirbizarre/git-tpl/compare/0.9.0..0.10.0) - 2026-08-26

### 💫 Features

- **testing**  🚨 **breaking** Replace git tpl test --trust with a per-case trust attribute ([#127](https://github.com/noirbizarre/git-tpl/issues/127)) - ([ce1e747](https://github.com/noirbizarre/git-tpl/commit/ce1e7470e98282319b542b0fad7335bf94961b9d))
- **testing** Let a test case run commands and opt explicitly into a snapshot - ([40515ec](https://github.com/noirbizarre/git-tpl/commit/40515ec08231cbad9f3ae6dee98bed9aa6028681))

### 🐛 Bug Fixes

- **cli** Refuse --ref together with --dirty instead of silently discarding --ref - ([83fd89c](https://github.com/noirbizarre/git-tpl/commit/83fd89c55cf3f1bbd7767b4ea26f527aa3542cd9))

## [0.9.0](https://github.com/noirbizarre/git-tpl/compare/0.8.1..0.9.0) - 2026-08-25

### 💫 Features

- **questions** Let a when-gated question keep its default when skipped ([#120](https://github.com/noirbizarre/git-tpl/issues/120)) - ([4e18adc](https://github.com/noirbizarre/git-tpl/commit/4e18adc7d38d8706c12c97ea29b637fe45077d93))
- **render** Let a path piece be `.` or fan out across `/` ([#118](https://github.com/noirbizarre/git-tpl/issues/118)) - ([f48ffaf](https://github.com/noirbizarre/git-tpl/commit/f48ffaf6baf209c84b227b6c4fa3944878710cd4))

### 🐛 Bug Fixes

- **lint** Report a question or computed value named after a MiniJinja builtin ([#122](https://github.com/noirbizarre/git-tpl/issues/122)) - ([dbd0aae](https://github.com/noirbizarre/git-tpl/commit/dbd0aae9b6e19da196ab43aba9a035d1c839c007))
- **testing** Stop letting .gitignore hide a --dirty snapshot's own files ([#119](https://github.com/noirbizarre/git-tpl/issues/119)) - ([55b29bf](https://github.com/noirbizarre/git-tpl/commit/55b29bf3dbe3a6fe130de8e5a849cd326abc33be))

### ⚡ Performance

- **test** Cut git subprocess spawns in the repository fixture harness - ([6a57d85](https://github.com/noirbizarre/git-tpl/commit/6a57d855604702de82766106e56a9a8c11b0ba15))

### 🔨 Refactor

- **ops** Extract merge_answers to de-duplicate the answers merge - ([d527c41](https://github.com/noirbizarre/git-tpl/commit/d527c41a265cea5d8e0340c9e9091b2c3b54bc0f))
- **template** Derive Error for ChoiceError instead of hand-rolled Display - ([729282a](https://github.com/noirbizarre/git-tpl/commit/729282aae368a3e5df0664f76a9f96d00acec025))

### 📚 Documentation

- **configuration** Document HOME's SSH-key-discovery use - ([007f6be](https://github.com/noirbizarre/git-tpl/commit/007f6be53d1dadeaf3e6082950dfecd644fb8878))
- **setup** Complete the invariants list and fix layout-tree wording drift - ([248189d](https://github.com/noirbizarre/git-tpl/commit/248189d3f31ea1c1a4a9957eb8d6ec8bf13b35c5))
- **skill** Drop version pinning from install instructions - ([7cc131d](https://github.com/noirbizarre/git-tpl/commit/7cc131df31ff733075a6262f0ba54e3bbfe3dd5c))
- **src** Stop calling a reference field a revision in doc comments - ([4fa3626](https://github.com/noirbizarre/git-tpl/commit/4fa3626c168f6bf83c57ad394bb1152cbdf836c6))
- Clarify AGENTS.md invariant 5's network-access scope - ([2344148](https://github.com/noirbizarre/git-tpl/commit/2344148ea5f26ab5e482e224b03d008e7854cac6))
- Replace the stale 0.7.0 example version with a version-agnostic placeholder - ([9576272](https://github.com/noirbizarre/git-tpl/commit/9576272be475e63a13b4b9a69c8199e94c09308f))

### 🔧 CI

- Disable windows defender real-time scanning for the test job - ([37b0951](https://github.com/noirbizarre/git-tpl/commit/37b09510c0696ba36208d0e78ef354e83d99a405))

## ❤️ New Contributors

* @ made their first contribution
## [0.8.1](https://github.com/noirbizarre/git-tpl/compare/0.8.0..0.8.1) - 2026-08-24

### 🐛 Bug Fixes

- **value** Keep `+`-concatenated sequences as arrays ([#112](https://github.com/noirbizarre/git-tpl/issues/112)) - ([1e4e1cb](https://github.com/noirbizarre/git-tpl/commit/1e4e1cba392afaf143c66537f2c39d43115b1f3c))

## [0.8.0](https://github.com/noirbizarre/git-tpl/compare/0.7.0..0.8.0) - 2026-08-24

### 💫 Features

- **lint** Warn on a when-gated question read outside its guard ([#106](https://github.com/noirbizarre/git-tpl/issues/106)) - ([3eb266e](https://github.com/noirbizarre/git-tpl/commit/3eb266ed17191457bf3e5c10284c4a3048be934e))
- **skill** Add an agent skill for driving git-tpl ([#104](https://github.com/noirbizarre/git-tpl/issues/104)) - ([e7f0801](https://github.com/noirbizarre/git-tpl/commit/e7f08015ba51d787711a21801d33c49de5aaffc9))
- **testing** Add `expect.lacks`, the negative partner to `expect.contains` ([#105](https://github.com/noirbizarre/git-tpl/issues/105)) - ([c125b3b](https://github.com/noirbizarre/git-tpl/commit/c125b3b19a63dc4d8ac4ae7925378d112e6d9de4))
- **update** Discover and apply template migrations ([#108](https://github.com/noirbizarre/git-tpl/issues/108)) - ([1803298](https://github.com/noirbizarre/git-tpl/commit/1803298385f93868f54afa8706a27e503c21528b))

### 🐛 Bug Fixes

- **answers** Enforce --strict-answers on init, update, context, diff and show - ([7746fbb](https://github.com/noirbizarre/git-tpl/commit/7746fbb87963e3a43ee3d90e5bd8b2bfdec2bf5c))
- **update** Stop claiming a new question caused every answers-file change - ([765454d](https://github.com/noirbizarre/git-tpl/commit/765454dc00a83c60be3fbb7800d77dff0320e1a4))

### 🔨 Refactor

- **backport** Rename report()/payload() to print_text()/json() - ([5984de3](https://github.com/noirbizarre/git-tpl/commit/5984de3ccb411586d318de6f586b0d4998f34c50))
- **lint** Rename report() to print_text() for naming consistency - ([93ff33d](https://github.com/noirbizarre/git-tpl/commit/93ff33d5268042057449af5572f9f43b11d3dc86))
- **provenance**  🚨 **breaking** Rename commit field to revision - ([650b5be](https://github.com/noirbizarre/git-tpl/commit/650b5beb45928700ad0f9c634fd3ed760ce98e50))

### 📚 Documentation

- **data** Pin down why the remote agent has no retry setting - ([d4a44b0](https://github.com/noirbizarre/git-tpl/commit/d4a44b031728f976d518d8170e3529977107e966))
- **skill** Simplify agent skill installation to two global paths ([#107](https://github.com/noirbizarre/git-tpl/issues/107)) - ([6d1b4a3](https://github.com/noirbizarre/git-tpl/commit/6d1b4a38ceeb94fbc741b6d02bd07af8f7627831))
- **templates** Document the migrations/ directory in the layout tree - ([f38f7f0](https://github.com/noirbizarre/git-tpl/commit/f38f7f0ba86561d729df86457d8b300cb99d1424))
- Clarify when an ops/ mechanism needs its own error type - ([06d8318](https://github.com/noirbizarre/git-tpl/commit/06d8318edb542460d9f3c6a4866c6517234ef4ab))
- Correct environment variable count from five to six - ([623ec18](https://github.com/noirbizarre/git-tpl/commit/623ec18ceaf0a243792d1f24f67f24ebfe3aa16f))
- Bump stale 0.6.0 examples to the current 0.7.0 release - ([3adb0d7](https://github.com/noirbizarre/git-tpl/commit/3adb0d78e22d667f8237f70d6659572a609206f2))
- Add migration.rs to the src/ layout trees - ([48ed046](https://github.com/noirbizarre/git-tpl/commit/48ed046324f57581dad969626850ce2cdf681e7e))
- Rewrap to semantic line breaks capped at 120 characters ([#109](https://github.com/noirbizarre/git-tpl/issues/109)) - ([f29e810](https://github.com/noirbizarre/git-tpl/commit/f29e81075cef195edbe633c32d3fb5d44db8e6ef))

### 🧪 Tests

- Adopt std::assert_matches! for structural assertions - ([16866ea](https://github.com/noirbizarre/git-tpl/commit/16866ea7dc757891cc339cc67d46026d1b22d7b4))

### 🏗️ Build

- **deps** Replace serde_norway with noyalib for YAML parsing ([#103](https://github.com/noirbizarre/git-tpl/issues/103)) - ([3981d93](https://github.com/noirbizarre/git-tpl/commit/3981d93fc092fc7d9c4bd41db2f9a688008f6bb2))

### 🧹 Chores

- Raise the MSRV to Rust 1.96 - ([80dde48](https://github.com/noirbizarre/git-tpl/commit/80dde482ab3f4d6d98d7d89c1be526560d1a9d4b))

## [0.7.0](https://github.com/noirbizarre/git-tpl/compare/0.6.0..0.7.0) - 2026-08-19

### 💫 Features

- **backport** `git tpl backport -p` — interactive hunk selection ([#78](https://github.com/noirbizarre/git-tpl/issues/78)) - ([f3e5620](https://github.com/noirbizarre/git-tpl/commit/f3e56205fea8192fbd52190d2dc9fe138096db52))
- **backport** Un-substitute changed regions - ([4b0ada0](https://github.com/noirbizarre/git-tpl/commit/4b0ada07f6a481571f8f90d9779e82cfdf0f8962))
- **init** Accept an optional destination directory ([#97](https://github.com/noirbizarre/git-tpl/issues/97)) - ([c35e59c](https://github.com/noirbizarre/git-tpl/commit/c35e59c67b7c0dda9fa56452bc3b7356b03cca7b))
- **init** Commit the attachment in the merge commit - ([8038d48](https://github.com/noirbizarre/git-tpl/commit/8038d48c5661eafbe4715ffc3d1e745f606fd7c4))
- **template** Accept a literal value in [computed] ([#92](https://github.com/noirbizarre/git-tpl/issues/92)) - ([601adf8](https://github.com/noirbizarre/git-tpl/commit/601adf887289656d9418d13b08cc8545b4bc6c53))
- **update** Warn when no rendered ref exists locally - ([ec9a4b6](https://github.com/noirbizarre/git-tpl/commit/ec9a4b68aa1e06ef521552c0a15e25b688660c04))

### 🐛 Bug Fixes

- **backport** Describe the revision through describe_revision - ([8017fd9](https://github.com/noirbizarre/git-tpl/commit/8017fd9c12cdb6916d72ce0ac1e3e4a52f29803d))
- **lint** Recognise raw blocks with trailing whitespace control ([#98](https://github.com/noirbizarre/git-tpl/issues/98)) - ([d14ffdd](https://github.com/noirbizarre/git-tpl/commit/d14ffddbd866b1808f75ed7c347ede672538d2a5))
- **lint** Report a binding that shadows a question or computed name ([#94](https://github.com/noirbizarre/git-tpl/issues/94)) - ([725b819](https://github.com/noirbizarre/git-tpl/commit/725b819e119ec5c3682d31d20b42ae69606e928f))
- **lint** Warn on a top-level manifest key absorbed by a table ([#93](https://github.com/noirbizarre/git-tpl/issues/93)) - ([d2fb940](https://github.com/noirbizarre/git-tpl/commit/d2fb9409c4131541e284d4686e412dbcfa994c10))
- **render** Report output write failures as tpl::ops::write_failed - ([38fdd6c](https://github.com/noirbizarre/git-tpl/commit/38fdd6c7a06bdcc29a7a579e79975c4da6c47356))
- **resolve** Only report ignored paths a render reads ([#91](https://github.com/noirbizarre/git-tpl/issues/91)) - ([e309963](https://github.com/noirbizarre/git-tpl/commit/e3099631887f7ff78d76fffd6b548dce8e78797b))
- **show** Route the stdout write failure through Reporter - ([a485d77](https://github.com/noirbizarre/git-tpl/commit/a485d773d626dc39ab60cbc3add1f9c81a1c8ba1))

### 🔨 Refactor

- **backport** Map changes through diff ops rather than a change stream - ([67a1c7e](https://github.com/noirbizarre/git-tpl/commit/67a1c7e3452649a8a54f032ecd43718b5bfd970b))
- **cli**  🚨 **breaking** Remove the deprecated status --format ([#90](https://github.com/noirbizarre/git-tpl/issues/90)) - ([c92ae56](https://github.com/noirbizarre/git-tpl/commit/c92ae567ada93923e79702080a1bc645fa7d8155))
- **ops** Materialise a rendered tree in one place - ([537a8dc](https://github.com/noirbizarre/git-tpl/commit/537a8dc50d85e95e110f8e263560defca7e1b9fd))

### 📚 Documentation

- **adr** Use one status format - ([a6a5513](https://github.com/noirbizarre/git-tpl/commit/a6a551339006d919bb87ce898a1588b993cc79b2))
- **answers** Move the --strict-answers paragraph to its warning - ([d32413e](https://github.com/noirbizarre/git-tpl/commit/d32413e6244747dc2c957128a53a77c82b425725))
- **data** Count the pinned items correctly - ([b00043c](https://github.com/noirbizarre/git-tpl/commit/b00043c098c7ad0c170ee3612d59c4ac3c46ab3c))
- **determinism** Qualify the no-runtime-context claim - ([d992164](https://github.com/noirbizarre/git-tpl/commit/d9921648c3a91c38130a720fcf76e0aa04264ea3))
- **json** Document the missing payload keys and dry-run shapes - ([40f4c6e](https://github.com/noirbizarre/git-tpl/commit/40f4c6e4df3f9c4725faacd24c4b030b8540363e))
- **json** Correct the universality and flat claims - ([26b72b4](https://github.com/noirbizarre/git-tpl/commit/26b72b47d398a5544506f7185bc93da76230aa9a))
- **quickstart** Match the transcript to the example template - ([3b22e1d](https://github.com/noirbizarre/git-tpl/commit/3b22e1d1ab6175e00776298fa42ad69734059b97))
- **render** Say that a dirty render honours .gitignore - ([4eca750](https://github.com/noirbizarre/git-tpl/commit/4eca750815af98f4cccf0d5766be5e750440dcc9))
- **setup** Restore the missing modules in the layout - ([75a71ae](https://github.com/noirbizarre/git-tpl/commit/75a71aead580b58032d796eac68bc1fbf4f0ab91))
- **templates** Add the missing as binding to the import example ([#95](https://github.com/noirbizarre/git-tpl/issues/95)) - ([0ba15ab](https://github.com/noirbizarre/git-tpl/commit/0ba15ab6911504a7b2e1a6b936975ec69730394f))
- Fix anchors pointing into usage/lint.md ([#96](https://github.com/noirbizarre/git-tpl/issues/96)) - ([6d3c1f8](https://github.com/noirbizarre/git-tpl/commit/6d3c1f877abdde1d30403d47e1e26769d814c613))
- Qualify the --json universality claim - ([21e692e](https://github.com/noirbizarre/git-tpl/commit/21e692e112ffcc934475bdb003d9441a1590ff7d))
- Document the XDG fallbacks and the colour environment - ([1b16596](https://github.com/noirbizarre/git-tpl/commit/1b165963f06ae2a7c7931c9b6a1cf1e45e81fa51))
- Document --strict-answers on init and update - ([bbde130](https://github.com/noirbizarre/git-tpl/commit/bbde130456d72f255d599e2a9a4767aae34d5c48))
- Point the front doors at all five install methods - ([7bec72c](https://github.com/noirbizarre/git-tpl/commit/7bec72c3ecd4def148ccda602a490d843a99e38d))
- Give --format json a single removal deadline - ([6f1407d](https://github.com/noirbizarre/git-tpl/commit/6f1407dc729d4d783a92bc6064d36142ebb7de07))
- Refresh the versions in examples to 0.6.0 - ([f59cf3a](https://github.com/noirbizarre/git-tpl/commit/f59cf3a65889ca1cb235ada05490ff77b9c9c2b9))
- Describe src/ops as the directory it is - ([9160308](https://github.com/noirbizarre/git-tpl/commit/91603084a29943989cd4834b2dc47cd2664bd0c4))

### 🧪 Tests

- **backport** Cover the un-substitution paths a terminal was hiding - ([44f73a6](https://github.com/noirbizarre/git-tpl/commit/44f73a65830ad04cc88cc647d97ba53b88f6bc01))

## [0.6.0](https://github.com/noirbizarre/git-tpl/compare/0.5.1..0.6.0) - 2026-08-17

### 💫 Features

- **cli** `git tpl backport` — emit a patch for the upstream template - ([e47aa75](https://github.com/noirbizarre/git-tpl/commit/e47aa75805bdd689c54d547a22466f06057a7307))
- **commands** Report gitignore-skipped paths from every dirty command - ([6381f2c](https://github.com/noirbizarre/git-tpl/commit/6381f2cd18e4a0d99cd15ca681e4ed4e909124bb))
- **data** Git-hosted data sources (source + ref + path) ([#59](https://github.com/noirbizarre/git-tpl/issues/59)) - ([32cdf38](https://github.com/noirbizarre/git-tpl/commit/32cdf3812ea6e25540c0c5ae80625b13d0b8314e))
- **questions** Derive a prompt seed from the repository ([#70](https://github.com/noirbizarre/git-tpl/issues/70)) - ([500c96b](https://github.com/noirbizarre/git-tpl/commit/500c96bbabc3c51a639295e3d1659777f33178a4))
- **render** Carry the template source path on a rendered file - ([239ce32](https://github.com/noirbizarre/git-tpl/commit/239ce32826c020d75adddc805bfb696e95313272))
- **template** A template may address the user and declare git remotes - ([41ab169](https://github.com/noirbizarre/git-tpl/commit/41ab1694fd7b27e4250e8365fc800b82037b4127))

### 🐛 Bug Fixes

- **cli** A mode-only difference is not a change to backport - ([52002dc](https://github.com/noirbizarre/git-tpl/commit/52002dca0dacdf8aa70c908ca13c59aab0528381))
- **cli** Spell the backport apply hint with forward slashes on Windows - ([a7ab3b0](https://github.com/noirbizarre/git-tpl/commit/a7ab3b000553c6a2807dd5b4167e4a7b8367d7f0))

### 🔨 Refactor

- **answers** Declare unknown_key in its own module - ([e3638ff](https://github.com/noirbizarre/git-tpl/commit/e3638fff522475c29d8724c3df146235445f5962))
- **commands** Session delegates output to Reporter - ([843b81b](https://github.com/noirbizarre/git-tpl/commit/843b81bc4d2d82954b22265ae7b94247aa7dfa3d))
- **config** One XDG config-home rule - ([566c5d2](https://github.com/noirbizarre/git-tpl/commit/566c5d2157de8aac2ccac87a5ab956b9a6d898a8))
- **git** Resolve_revision takes a reference, not a revision - ([d2508c5](https://github.com/noirbizarre/git-tpl/commit/d2508c56b6959958343fc279984da454d547ca69))
- **ops** Add ops::lint and ops::questions - ([6f845ad](https://github.com/noirbizarre/git-tpl/commit/6f845adcd7df97dc9608686598149b6f44efbfd7))
- **ops** Name describe_revision output revision_description - ([5190066](https://github.com/noirbizarre/git-tpl/commit/5190066521a8c272672e2a12e48f3b126ac944f9))
- **theme** Add transition() for the A → B line - ([a107bdf](https://github.com/noirbizarre/git-tpl/commit/a107bdf54da4a65626711904ac642ad0c719812d))

### 📚 Documentation

- **adr** ADR-020 — backport emits a patch, and proves it by re-rendering - ([25f4bf7](https://github.com/noirbizarre/git-tpl/commit/25f4bf79cff852976a031f7952af240263077959))
- **adr** Decline post-render tasks, and state what replaces them - ([40f9ddb](https://github.com/noirbizarre/git-tpl/commit/40f9ddb9bf6a29407955af4763b6dd7e26624457))
- **cli** Document `git tpl backport` and the loop it closes - ([6abe181](https://github.com/noirbizarre/git-tpl/commit/6abe181c5fed77628ecaa2bafa0a55c461e9dc01))
- **data** Link the git source's trust note to the page that exists ([#69](https://github.com/noirbizarre/git-tpl/issues/69)) - ([5e698b9](https://github.com/noirbizarre/git-tpl/commit/5e698b972f61d46a56930ab115fb22238cd096d7))
- **diagnostics** Template.toml does not reject unknown keys - ([ec3d23b](https://github.com/noirbizarre/git-tpl/commit/ec3d23b7c6bf391e9c537c2ffc5fc4a6a7329c1d))
- **init** Document --force - ([822eacb](https://github.com/noirbizarre/git-tpl/commit/822eacba8d71a4215709003f708d2b2ec1220267))
- **lint** Add an options table - ([bb736ed](https://github.com/noirbizarre/git-tpl/commit/bb736edb68cdfc5e544cee039c257375c11e75fd))
- **readme** Fix the underlined spaces around the header badges ([#72](https://github.com/noirbizarre/git-tpl/issues/72)) - ([24262a8](https://github.com/noirbizarre/git-tpl/commit/24262a82f0c16866d330fe9f448cb5d0880de645))
- **show** Document --dirty and the answer flags - ([40958eb](https://github.com/noirbizarre/git-tpl/commit/40958eba810091d510d348097680b8786523aa98))
- **status** Lead with --json, and document --dirty - ([dc4e2e3](https://github.com/noirbizarre/git-tpl/commit/dc4e2e351ae8be295d3eccd0003be6b9a27a0e22))
- **templates** Correct the manifest schema - ([3e33a57](https://github.com/noirbizarre/git-tpl/commit/3e33a5788a30381ee28830d78dd80d9c4cf7d347))
- **test** Drop the nonexistent context --partials flag - ([9fc9e64](https://github.com/noirbizarre/git-tpl/commit/9fc9e6447506f93f0e74d58afb95e6a012ea91fd))
- **trust** Trust gates a git source's clone too - ([a428df0](https://github.com/noirbizarre/git-tpl/commit/a428df006034f75e80ea6fe4044c3f2d2e02401d))
- Fix stale keys, versions, counts and missing options - ([3b30275](https://github.com/noirbizarre/git-tpl/commit/3b302756c5d707649843c9b8cfb218e29df512e2))

### 🧪 Tests

- **cli** Redact the version from the backport snapshots ([#71](https://github.com/noirbizarre/git-tpl/issues/71)) - ([40ed77d](https://github.com/noirbizarre/git-tpl/commit/40ed77d6e29f844403c349c577538aca674205dc))
- **cli** Pin line endings for the backport clone before it checks out - ([d9345d8](https://github.com/noirbizarre/git-tpl/commit/d9345d8f89d6c67180a26d99894e88633e6a92cd))
- **cli** Pin the backport transcripts, and redact the mailbox date - ([3fdf701](https://github.com/noirbizarre/git-tpl/commit/3fdf7017ba621765f21b94c27b109368e28df885))
- **cli** Snapshot the documented CLI output ([#62](https://github.com/noirbizarre/git-tpl/issues/62)) - ([b777c49](https://github.com/noirbizarre/git-tpl/commit/b777c494751edc797a35a5dfa3b6276461126fbb))
- **render** Pin the executable-bit behaviour on every platform ([#61](https://github.com/noirbizarre/git-tpl/issues/61)) - ([313d6cd](https://github.com/noirbizarre/git-tpl/commit/313d6cd19f6308609e6ed6b1e96684fd8e9217f9))

### 🎨 Style

- Apply rustfmt after the Reporter and ops refactors - ([6af94d5](https://github.com/noirbizarre/git-tpl/commit/6af94d5c1bedead788829d68d0cb87f53ac821e8))

## [0.5.1](https://github.com/noirbizarre/git-tpl/compare/0.5.0..0.5.1) - 2026-08-16

### 🐛 Bug Fixes

- **cli** A --json payload for every command that had none ([#57](https://github.com/noirbizarre/git-tpl/issues/57)) - ([2b015e2](https://github.com/noirbizarre/git-tpl/commit/2b015e294dbe8f4388910787b1220820118ecc67))
- **git** Tell a full disk from an unreachable remote ([#54](https://github.com/noirbizarre/git-tpl/issues/54)) - ([ddf2f33](https://github.com/noirbizarre/git-tpl/commit/ddf2f33dd69d77883992a31579e9842cb4b758a9))
- **git** Honour .gitignore negations over global ignore rules in --dirty ([#55](https://github.com/noirbizarre/git-tpl/issues/55)) - ([3dc18fd](https://github.com/noirbizarre/git-tpl/commit/3dc18fdec4e244ce3bffb057fb92288d29367633))

### 🔧 CI

- **release** A build-only dispatch, so this workflow can be tested - ([2883af1](https://github.com/noirbizarre/git-tpl/commit/2883af1f8df8c95259e5b46f81c7b9c8f2e46d7e))
- **release** Generate the man pages and completions in each build leg - ([c975efe](https://github.com/noirbizarre/git-tpl/commit/c975efe053312bb95135a18796958a6f051a077e))
- **ship** Tidy up the Ship PR description ([#58](https://github.com/noirbizarre/git-tpl/issues/58)) - ([3f0eec4](https://github.com/noirbizarre/git-tpl/commit/3f0eec46ac24b568744d67eb302db1298eb313bb))

## [0.5.0](https://github.com/noirbizarre/git-tpl/compare/0.4.0..0.5.0) - 2026-08-16

### 💫 Features

- **artwork** A wordmark logo, so the social preview renders the same twice ([#50](https://github.com/noirbizarre/git-tpl/issues/50)) - ([6595b0f](https://github.com/noirbizarre/git-tpl/commit/6595b0f2ea409fecac59c85783355d0d5f65d4dd))
- **cli** `git tpl test` — a test runner for templates - ([f707200](https://github.com/noirbizarre/git-tpl/commit/f7072003bdd10069999e978bc4244d4af0629c89))
- **cli** Make git tpl --help work, with a man page and shell completions ([#44](https://github.com/noirbizarre/git-tpl/issues/44)) - ([31597bb](https://github.com/noirbizarre/git-tpl/commit/31597bb331fdb0c620eebc6e536cc0edf5f8a326))
- **lint** --deny and --allow, by code or by severity ([#48](https://github.com/noirbizarre/git-tpl/issues/48)) - ([d6c6458](https://github.com/noirbizarre/git-tpl/commit/d6c64585eaac4f3d9acb63f2380b04506c1ea2cd))

### 🐛 Bug Fixes

- **git** Restore the https and ssh transports, so a remote can be reached ([#47](https://github.com/noirbizarre/git-tpl/issues/47)) - ([1f32914](https://github.com/noirbizarre/git-tpl/commit/1f329141464522518fbe2f490187dfbdac54c47d))

### 🔨 Refactor

- **ops** Resolve a template once for many renderings - ([353e5dd](https://github.com/noirbizarre/git-tpl/commit/353e5dd15e7ff35f0108c9a0fa8db46138abf1fc))

### 🧪 Tests

- **testing** Cover the paths a broken case or a corrupted snapshot takes - ([3bc5f22](https://github.com/noirbizarre/git-tpl/commit/3bc5f2279f7d35db2e3e1a0670070ab8b1246b74))

## [0.4.0](https://github.com/noirbizarre/git-tpl/compare/0.3.0..0.4.0) - 2026-08-16

### 💫 Features

- **cli** Add --strict-answers, --exit-code and init --force - ([b6d0149](https://github.com/noirbizarre/git-tpl/commit/b6d0149c3ffa85da73e2344fffb0ce6238f43f76))
- **cli** Preview an uncommitted template with --dirty on diff, show and status - ([dbff20e](https://github.com/noirbizarre/git-tpl/commit/dbff20e3e7c106637a846c22e908b04ed1311afe))
- **cli**  🚨 **breaking** Emit machine-readable JSON, including on failure - ([1283e54](https://github.com/noirbizarre/git-tpl/commit/1283e545422eda3c7ecbd7fd3ba32c606c464a67))
- **commands** Add git tpl context to show what a template sees - ([09f2fd8](https://github.com/noirbizarre/git-tpl/commit/09f2fd8a627c79c5ec0ce504f283a02b1581ec1b))
- **commands** Add git tpl questions for the answer schema - ([2569dd0](https://github.com/noirbizarre/git-tpl/commit/2569dd08c7a6f2b83a22ad6ea8ab492e4fa722e3))
- **commands** Add git tpl lint for the checks rendering cannot make - ([807ad8f](https://github.com/noirbizarre/git-tpl/commit/807ad8fe82062260dcf203298b83906a5eeed3a1))
- **commands** Add git tpl render for a project-free rendering - ([1ff2e30](https://github.com/noirbizarre/git-tpl/commit/1ff2e302dbb881c88b8144592272b1caa56a7113))
- **commands** Add git tpl show for reading one path from the rendered ref ([#10](https://github.com/noirbizarre/git-tpl/issues/10)) - ([9e7d66d](https://github.com/noirbizarre/git-tpl/commit/9e7d66d96a0f44be95efc42b40cf1de888ff43b5))
- **diff** Count lines in --stat ([#12](https://github.com/noirbizarre/git-tpl/issues/12)) - ([8ba290b](https://github.com/noirbizarre/git-tpl/commit/8ba290b77e9c2fd0b2ba0b939bf64413cd8e8f7e))
- **dist** AUR packages `git-tpl-bin` and `git-tpl` ([#43](https://github.com/noirbizarre/git-tpl/issues/43)) - ([0b83dc6](https://github.com/noirbizarre/git-tpl/commit/0b83dc6f11b8616dfb7233db7e9d5ae7a165109c))
- **dist** Homebrew tap noirbizarre/homebrew-tap ([#38](https://github.com/noirbizarre/git-tpl/issues/38)) - ([d04aa1c](https://github.com/noirbizarre/git-tpl/commit/d04aa1c053844bf8b43a7e7c8d7d4d0060d872cf))
- **eval** Close the undefined-name asymmetry between manifests and files - ([61ae117](https://github.com/noirbizarre/git-tpl/commit/61ae1176e5cb571e032d3310e6bdd6251577b0a8))
- **ops** Render to bytes without a project - ([94dd5cf](https://github.com/noirbizarre/git-tpl/commit/94dd5cf4169e7bfb5ca8faee3724cebdce2f66f4))

### 🐛 Bug Fixes

- **diff** Preview the merge instead of diffing the trees - ([febbc37](https://github.com/noirbizarre/git-tpl/commit/febbc37504b6daadbd20ed0c0e62044873d76660))
- **questions** The text listing reports a value type where the schema means the declared kind ([#16](https://github.com/noirbizarre/git-tpl/issues/16)) - ([9be9e74](https://github.com/noirbizarre/git-tpl/commit/9be9e743f067b22950b9cca2946b51f6e87d7768))
- **render** Explain what .gitignore removed, and make --dry-run mean one thing - ([9ddd798](https://github.com/noirbizarre/git-tpl/commit/9ddd798f8e70b9d16fca37ba722bb2044f97642a))

### 📚 Documentation

- **diff** Document --exit-code and --dirty ([#42](https://github.com/noirbizarre/git-tpl/issues/42)) - ([4d16624](https://github.com/noirbizarre/git-tpl/commit/4d1662412a9673e9368bca8400b72129beafadef))
- Replace PLAN.md with the issue tracker and a roadmap page ([#36](https://github.com/noirbizarre/git-tpl/issues/36)) - ([de94982](https://github.com/noirbizarre/git-tpl/commit/de94982a619799b2a3480140aedbba63f116efb4))
- Document the git-tpl GitHub topic for template discovery - ([5b64bf8](https://github.com/noirbizarre/git-tpl/commit/5b64bf8c6aa11509860b0263b79a31a12451b627))

### 🧪 Tests

- **eval** A computed sequence and a computed table keep their type through evaluate() ([#39](https://github.com/noirbizarre/git-tpl/issues/39)) - ([07dbe5e](https://github.com/noirbizarre/git-tpl/commit/07dbe5e4b9fa3199cb8ab45188e411220b374c6c))
- Detach the harness from the ambient Git environment ([#17](https://github.com/noirbizarre/git-tpl/issues/17)) - ([c77dd2a](https://github.com/noirbizarre/git-tpl/commit/c77dd2a5435b82f98934b8da9088eb1942e87e77))

### 🏗️ Build

- **release**  🚨 **breaking** Publish .tar.gz archives containing a plain git-tpl ([#37](https://github.com/noirbizarre/git-tpl/issues/37)) - ([05315b6](https://github.com/noirbizarre/git-tpl/commit/05315b60b742e4a10c5c7f6d064b0f8a09cb0edd))

## [0.3.0](https://github.com/noirbizarre/git-tpl/compare/0.2.0..0.3.0) - 2026-08-15

### 💫 Features

- **questions** Validate string answers with a pattern - ([ce13a49](https://github.com/noirbizarre/git-tpl/commit/ce13a4916f8310d63df610d4697a55758a15c1ae))
- **render** Share macros with {% import %} and {% include %} - ([e04ad3c](https://github.com/noirbizarre/git-tpl/commit/e04ad3c042d2de07a72db830b2cfdb75bb8c24a2))
- **userconfig** Authorise templates from a [trust] list - ([3b1fbcb](https://github.com/noirbizarre/git-tpl/commit/3b1fbcbcfc7996c7d8989b3665cf2237f224ed76))
- **userconfig** Expand [shortcuts] in a template URL - ([e609588](https://github.com/noirbizarre/git-tpl/commit/e609588ab3b173741419a2b799cc6cc1e0ab6462))
- **userconfig** Seed prompts from [defaults] - ([72dbae4](https://github.com/noirbizarre/git-tpl/commit/72dbae4c3af0049ab513fd80bf38be35fb1f78ca))
- **userconfig**  🚨 **breaking** Read ~/.config/git-tpl/config.toml - ([b868a09](https://github.com/noirbizarre/git-tpl/commit/b868a0965453ca926a9e9bf4c24f408f2de51720))

### 🐛 Bug Fixes

- **cli** Describe every revision the same way - ([f1bf503](https://github.com/noirbizarre/git-tpl/commit/f1bf5034424114cb7d98619d0e7ac653e0cb3414))
- **graph** Name the unknown reference in the help - ([a04ece7](https://github.com/noirbizarre/git-tpl/commit/a04ece7a3a45113dc621c2ae6e6c100a5092d7b6))

### 🔨 Refactor

- **commands** Remove the small divergences in the command layer - ([3f59b5a](https://github.com/noirbizarre/git-tpl/commit/3f59b5ae3d7f387f2293f0c884784ca4ec727ac1))
- **commands** Rename Context to Session - ([89424f2](https://github.com/noirbizarre/git-tpl/commit/89424f240a8b7f9dd33ed8ef80622fdc87ce056b))
- **data** Carry SourceKind through the error path - ([5a7505e](https://github.com/noirbizarre/git-tpl/commit/5a7505e0d2bdd77931ed232ef8ba09351ba8d1b3))
- **errors** Drop the duplicate variants for two conditions - ([c8fd61c](https://github.com/noirbizarre/git-tpl/commit/c8fd61cb7f4c6bf776e81afc0bf25c855745a535))
- **git** Keep the GitBackend boundary above src/git/ - ([135b0f3](https://github.com/noirbizarre/git-tpl/commit/135b0f3946aa61a05006413a67b1eda3fb4cf3ef))
- **gitconfig** One shape for preference overrides - ([a2a72bd](https://github.com/noirbizarre/git-tpl/commit/a2a72bda09eced466772a6af89d11b9bea21c384))
- **update** Use the shared answering and trust helpers in the dry run - ([0e54fd8](https://github.com/noirbizarre/git-tpl/commit/0e54fd86cd6955debe504d4491210219b3886c34))
- Reconcile the documentation and the code ([#7](https://github.com/noirbizarre/git-tpl/issues/7)) - ([290e05f](https://github.com/noirbizarre/git-tpl/commit/290e05fa1cb404bd955f429b9c6ff7ec666c7aa0))
- Never name a String `revision` - ([8fc79de](https://github.com/noirbizarre/git-tpl/commit/8fc79dea60c20404ab15e2f2865a8ff1c5a8bc25))

### 📚 Documentation

- **data** Remote sources take YAML too - ([84ab02d](https://github.com/noirbizarre/git-tpl/commit/84ab02de9aff65c8c10fe52553d24390048edafe))
- **determinism** Checksum pinning exists - ([713df97](https://github.com/noirbizarre/git-tpl/commit/713df9707de452470ea7e42a8e9c1df2a62e7c48))
- **diff** Say that the diff covers the whole tree - ([328f15f](https://github.com/noirbizarre/git-tpl/commit/328f15f1a57fc6b4d295d4291c0ff7cbcb11e5f3))
- **quickstart** Make the conditional path actually conditional - ([1a96d62](https://github.com/noirbizarre/git-tpl/commit/1a96d62210361707faeb488eb7abe7894ccafa15))
- **releasing** Match what cliff.toml actually sets - ([3d757a2](https://github.com/noirbizarre/git-tpl/commit/3d757a213a45262da84acf474df76a8073fff21b))
- Correct the ADR index URL - ([db3aa47](https://github.com/noirbizarre/git-tpl/commit/db3aa47c3f7d281559070b31ce15c0c53304793c))
- Regenerate the transcripts from real runs - ([5ab40f4](https://github.com/noirbizarre/git-tpl/commit/5ab40f4c24b4e882e4bc0dd8b88b6519a26e4890))
- One accurate description of the src/ tree - ([5935cb2](https://github.com/noirbizarre/git-tpl/commit/5935cb2deba00e65c88ec97dd8d01bb81dc12ca7))
- Drop the flags that do not exist, add the ones that do - ([7a45ac2](https://github.com/noirbizarre/git-tpl/commit/7a45ac23fa8be996d002aa93bf31624aae10762a))
- Use one derived template id in every example - ([0d4be94](https://github.com/noirbizarre/git-tpl/commit/0d4be9424a7b818b7e3cf8d7cdd19d77252289ee))

### 🧪 Tests

- Pin test repositories to LF line endings - ([3384d04](https://github.com/noirbizarre/git-tpl/commit/3384d04940668cfeb4db4a6ae5ba9af53e2458e9))
- Name tests as sentences, and share the one duplicated helper - ([72186d9](https://github.com/noirbizarre/git-tpl/commit/72186d90bf8197c1628c9f740239127d35bc5813))

### 🔧 CI

- Assert on `git tpl status` output without its exit status - ([8b2f648](https://github.com/noirbizarre/git-tpl/commit/8b2f648de81521e165becfa89d0df3ebfd49c9c2))
- Fail the job when a Windows step fails - ([9e027ac](https://github.com/noirbizarre/git-tpl/commit/9e027accf20cf03596edfddffee8d92a75789aa1))

## [0.2.0](https://github.com/noirbizarre/git-tpl/compare/0.1.0..0.2.0) - 2026-08-14

### 💫 Features

- **cli** Read answers from a file - ([eb39096](https://github.com/noirbizarre/git-tpl/commit/eb3909636502b43d3c4eb469dc0ee88e4663cd8d))
- **data** Fetch remote data sources - ([6068e8b](https://github.com/noirbizarre/git-tpl/commit/6068e8b85f55c8fc58ddd51175543490b310c2b7))
- **data** Accept YAML data sources - ([d3052df](https://github.com/noirbizarre/git-tpl/commit/d3052df6249eedf90150e5d0eef1616044e39974))
- **questions** Seed a prompt default from Git configuration - ([d038e02](https://github.com/noirbizarre/git-tpl/commit/d038e028a6367326d87d7478f274c2f3724e82c7))
- **questions**  🚨 **breaking** Give choices a label, a help line and a filter - ([f34dc7d](https://github.com/noirbizarre/git-tpl/commit/f34dc7d6effe2d3cc78f8a1dd483fb0a2a6287b6))
- **templates** Add the slugify filter - ([5c3e415](https://github.com/noirbizarre/git-tpl/commit/5c3e415d1c50d21674e75f3b1665e06e0031c29c))

### 🐛 Bug Fixes

- **tests** Clear the inherited non-blocking flag on accepted sockets - ([b9497da](https://github.com/noirbizarre/git-tpl/commit/b9497da3b6572ed398b5b10b254f6f42996af6e5))

### 📚 Documentation

- **init** Document attaching a template to an existing project - ([5ff87b5](https://github.com/noirbizarre/git-tpl/commit/5ff87b500fbbaaa4c7f02e78fb15c2ccb60cce7f))
- **install** Document installing with mise - ([abb23ae](https://github.com/noirbizarre/git-tpl/commit/abb23ae8e9f41b14281045f5aca29573f0590ca7))
- **plan** Packaging, user configuration, template testing and inheritance - ([f128c27](https://github.com/noirbizarre/git-tpl/commit/f128c27b6efb930a8a9494dfa7861109c03a29bb))
- **plan** Record what migrating a real template needs - ([3ae1910](https://github.com/noirbizarre/git-tpl/commit/3ae1910892da1ba3daa6d1b9eed11f1cf65ef69a))

### 🏗️ Build

- **deps** Bump git2 from 0.20.4 to 0.21.0 ([#3](https://github.com/noirbizarre/git-tpl/issues/3)) - ([b9006b5](https://github.com/noirbizarre/git-tpl/commit/b9006b587e415a53ac421c163a0628709ea3f869))
- **deps** Bump sha2 from 0.10.9 to 0.11.0 ([#2](https://github.com/noirbizarre/git-tpl/issues/2)) - ([d5b0614](https://github.com/noirbizarre/git-tpl/commit/d5b061478d2e1f5d01b309ba1fa783ba8423b053))
- **deps** Bump toml from 0.9.12+spec-1.1.0 to 1.1.4+spec-1.1.0 ([#1](https://github.com/noirbizarre/git-tpl/issues/1)) - ([9ab2091](https://github.com/noirbizarre/git-tpl/commit/9ab20910025110c3fa4fbf9adf649222f3c8a7e0))

### 🔧 CI

- **git-cliff** Bump minor on breaking change until 1.0 - ([d00c4fb](https://github.com/noirbizarre/git-tpl/commit/d00c4fbe035ebbe24c740cefa229b9afc54a47cb))
- Publish to crates.io with Trusted Publishing - ([bc5ca59](https://github.com/noirbizarre/git-tpl/commit/bc5ca59b44d7e44fb927b7040c98a2b5e6782791))

## ❤️ New Contributors

* @dependabot[bot] made their first contribution in [#3](https://github.com/noirbizarre/git-tpl/pull/3)
## 0.1.0 - 2026-08-14

### 💫 Features

- Render templates into Git refs - ([2e69ca9](https://github.com/noirbizarre/git-tpl/commit/2e69ca929186ddd0106ff58162e9b1fd50ae9049))

### 🐛 Bug Fixes

- Track mise.toml, which a global gitignore had silently dropped - ([47b9404](https://github.com/noirbizarre/git-tpl/commit/47b94046e558046b0939590e33c89eea024a9e86))
- Point every repository reference at noirbizarre/git-tpl - ([b277284](https://github.com/noirbizarre/git-tpl/commit/b2772845ebfa9d38c3c0d2b30064d69160931b8b))

### 📚 Documentation

- **logo** Generate icons and social preview - ([2824849](https://github.com/noirbizarre/git-tpl/commit/2824849ba8c5bf0a8d7c49ef6dfbb37afbdb88f7))
- Add the coverage, crates.io and release badges to the README - ([46b4304](https://github.com/noirbizarre/git-tpl/commit/46b430424771966a7c0109e310ed85e97c064cf7))
- Document the model, the commands and the decisions - ([a0792dd](https://github.com/noirbizarre/git-tpl/commit/a0792dd97d6504f60513eca307b49b3753da63d2))

### 🧪 Tests

- Cover the lifecycle against real Git repositories - ([e3709d7](https://github.com/noirbizarre/git-tpl/commit/e3709d7227758eb67d4989747e7f0952ca12d2cf))

### 🏗️ Build

- **mise** Update the lock file - ([298250b](https://github.com/noirbizarre/git-tpl/commit/298250b469f66d04926a368a07a2735a54a5b075))

### 🔧 CI

- Authenticate git-cliff's GitHub API calls - ([a134de6](https://github.com/noirbizarre/git-tpl/commit/a134de6680b06e4aeadd55917a394f064b32b69c))

### 🧹 Chores

- Set up the project tooling - ([6bfb252](https://github.com/noirbizarre/git-tpl/commit/6bfb25208b374cc83033dbd7eb1e5292e88f01be))

## ❤️ New Contributors

* @noirbizbot[bot] made their first contribution in [#4](https://github.com/noirbizarre/git-tpl/pull/4)
* @noirbizarre made their first contribution
