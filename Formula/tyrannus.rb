class Tyrannus < Formula
  desc "Terminal word processor"
  homepage "https://github.com/huffs-projects/tyrannus"

  RELEASE_TAG = "0.1.2".freeze # HOMEBREW_BUMP_TAG
  version "0.1.2" # HOMEBREW_BUMP_VERSION

  license "GPL-2.0-only"

  gh_release = "https://github.com/huffs-projects/tyrannus/releases/download/#{RELEASE_TAG}"

  if OS.mac? && Hardware::CPU.arm?
    url "#{gh_release}/tyrannus-#{RELEASE_TAG}-macos-aarch64.tar.gz"
    sha256 "a44636e1662b619a3f4c120e605bcbc4333a84ce41ee787460ca5ba3782aaf41" # HOMEBREW_BUMP_MACOS_AARCH64
  elsif OS.linux? && Hardware::CPU.intel?
    url "#{gh_release}/tyrannus-#{RELEASE_TAG}-linux-x86_64.tar.gz"
    sha256 "57d7ffae5e26fe3dc2a32034d59c1e2aaaf8c1be578d39c6b9bb5cdbed0bf421" # HOMEBREW_BUMP_LINUX_X86_64
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
