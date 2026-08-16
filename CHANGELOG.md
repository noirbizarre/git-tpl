# Changelog

All notable changes to this project will be documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
