# frozen_string_literal: true

# Homebrew formula for br - Agent-first issue tracker
# Repository: https://github.com/quangdang46/br
#
# To install:
#   brew install quangdang46/br/br
#
# Asset names and the checksum values are produced by the release pipeline, not
# by hand. `.github/workflows/release.yml` builds a single-file `br` binary per
# platform and publishes it as a *tagless* archive -- `br-macos-arm64.tar.gz`,
# `br-macos-x64.tar.gz`, `br-linux-x64.tar.gz`, `br-linux-arm64.tar.gz` -- under
# the release tag, so the version never appears in the asset name. That is the
# same contract `install.sh` uses when it builds `br-${platform}.${archive_ext}`.
# The sha256 values below are rewritten at release time; a stale value here makes
# `brew install` fail verification rather than silently install the wrong bytes.

class Br < Formula
  desc "Agent-first issue tracker (SQLite + JSONL)"
  homepage "https://github.com/quangdang46/br"
  license "MIT"
  version "0.1.3"

  on_macos do
    on_arm do
      url "https://github.com/quangdang46/br/releases/download/v#{version}/br-macos-arm64.tar.gz"
      sha256 "REWRITTEN_AT_RELEASE_TIME_br-macos-arm64"
    end
    on_intel do
      url "https://github.com/quangdang46/br/releases/download/v#{version}/br-macos-x64.tar.gz"
      sha256 "REWRITTEN_AT_RELEASE_TIME_br-macos-x64"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/quangdang46/br/releases/download/v#{version}/br-linux-x64.tar.gz"
      sha256 "REWRITTEN_AT_RELEASE_TIME_br-linux-x64"
    end
    on_arm do
      url "https://github.com/quangdang46/br/releases/download/v#{version}/br-linux-arm64.tar.gz"
      sha256 "REWRITTEN_AT_RELEASE_TIME_br-linux-arm64"
    end
  end

  def install
    bin.install "br"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/br --version")

    # Test basic functionality
    system bin/"br", "init"
    assert_predicate testpath/".beads", :directory?
    assert_predicate testpath/".beads/beads.db", :file?
  end
end
