class Tyrannus < Formula
  desc "Terminal word processor"
  homepage "https://github.com/huffs-projects/tyrannus"

  RELEASE_TAG = "0.1.0".freeze # HOMEBREW_BUMP_TAG
  version "0.1.0" # HOMEBREW_BUMP_VERSION

  license "GPL-2.0-only"

  gh_release = "https://github.com/huffs-projects/tyrannus/releases/download/#{RELEASE_TAG}"

  if OS.mac? && Hardware::CPU.arm?
    url "#{gh_release}/tyrannus-#{RELEASE_TAG}-macos-aarch64.tar.gz"
    sha256 "91b3f985f133547aa35cddbc8f501d266699d842a66f03e2c598ceb88a5dc27a" # HOMEBREW_BUMP_MACOS_AARCH64
  elsif OS.linux? && Hardware::CPU.intel?
    url "#{gh_release}/tyrannus-#{RELEASE_TAG}-linux-x86_64.tar.gz"
    sha256 "750bab72b22b0ea87cc45ab3be5a47f8650c65d108b8857357ad74022fb270a8" # HOMEBREW_BUMP_LINUX_X86_64
  else
    odie "tyrannus: unsupported platform (only linux-x86_64 and macOS arm64 binaries are shipped)"
  end

  def install
    bin.install "tyrannus"
  end

  test do
    assert_predicate bin/"tyrannus", :executable?
  end
end
