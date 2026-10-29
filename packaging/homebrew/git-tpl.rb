# Homebrew formula template.
#
# `@VERSION@` and the `@SHA256_*@` placeholders are substituted by
# .github/workflows/homebrew.yaml from the published release assets, and the
# result is pushed to noirbizarre/homebrew-tap as Formula/git-tpl.rb.
class GitTpl < Formula
  desc "Git-native project templates"
  homepage "https://github.com/noirbizarre/git-tpl"
  version "@VERSION@"
  license "MIT"

  # The release archive, not the raw executable: it carries the man pages and
  # completions the bare binary cannot. Homebrew strips the archive's single
  # top-level directory, so the paths in `install` start at `bin/`.
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
  # distribution Homebrew supports regardless of its glibc.
  on_linux do
    on_intel do
      url "https://github.com/noirbizarre/git-tpl/releases/download/#{version}/git-tpl_#{version}_linux-amd64-musl.tar.gz"
      sha256 "@SHA256_LINUX_AMD64_MUSL@"
    end
  end

  def install
    bin.install "bin/git-tpl"

    # Each is optional: the archive only carries what the project generates.
    man1.install Dir["share/man/man1/*.1"] unless Dir["share/man/man1/*.1"].empty?
    bash_completion.install "share/bash-completion/completions/git-tpl" if File.exist?("share/bash-completion/completions/git-tpl")
    zsh_completion.install "share/zsh/site-functions/_git-tpl" if File.exist?("share/zsh/site-functions/_git-tpl")
    fish_completion.install "share/fish/vendor_completions.d/git-tpl.fish" if File.exist?("share/fish/vendor_completions.d/git-tpl.fish")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/git-tpl --version")
  end
end
