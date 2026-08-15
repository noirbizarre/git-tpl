# Homebrew formula template.
#
# `@VERSION@` and the `@SHA256_*@` placeholders are substituted by
# .github/workflows/homebrew.yaml from the published release assets, and the
# result is pushed to noirbizarre/homebrew-tap as Formula/git-tpl.rb.
#
# The formula is named after the binary (`git-tpl`), which is also the crate
# name, because that is what `brew install noirbizarre/tap/git-tpl` spells.
class GitTpl < Formula
  desc "Git-native project templates"
  homepage "https://noirbizarre.github.io/git-tpl/"
  version "@VERSION@"
  license "MIT"

  # Prebuilt binaries from the GitHub release rather than a source build:
  # installing takes no Rust toolchain, and libgit2 is already vendored into
  # each binary so there is nothing to link against.
  #
  # This project tags without a `v` prefix, so the tag is `#{version}` as-is.
  on_macos do
    on_arm do
      url "https://github.com/noirbizarre/git-tpl/releases/download/#{version}/git-tpl_#{version}_darwin-arm64.tar.gz"
      sha256 "@SHA256_DARWIN_ARM64@"
    end
    on_intel do
      url "https://github.com/noirbizarre/git-tpl/releases/download/#{version}/git-tpl_#{version}_darwin-amd64.tar.gz"
      sha256 "@SHA256_DARWIN_AMD64@"
    end
  end

  # musl rather than gnu: the binary is statically linked, so it runs on any
  # distribution Homebrew supports regardless of its glibc. There is no
  # aarch64-musl leg in the release matrix, so Linux arm64 is left unsupported
  # rather than served a glibc-pinned binary that would fail at load time.
  on_linux do
    on_intel do
      url "https://github.com/noirbizarre/git-tpl/releases/download/#{version}/git-tpl_#{version}_linux-amd64-musl.tar.gz"
      sha256 "@SHA256_LINUX_AMD64_MUSL@"
    end
  end

  # Not a build dependency — a usage one. Git is what resolves `git tpl` to
  # this executable, and every workflow in the documentation is a `git` one.
  depends_on "git"

  def install
    # The archive holds a plain `git-tpl` at its root, so there is nothing to
    # rename: Git only resolves `git tpl` through an executable called exactly
    # `git-tpl`.
    bin.install "git-tpl"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/git-tpl --version")
    assert_match "template", shell_output("#{bin}/git-tpl --help")
  end
end
