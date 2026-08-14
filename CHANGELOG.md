# Changelog

All notable changes to this project will be documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
