class Tyrannus < Formula
  desc "Terminal word processor"
  homepage "https://github.com/huffs-projects/tyrannus"

  RELEASE_TAG = "0.1.2.1".freeze # HOMEBREW_BUMP_TAG
  version "0.1.2.1" # HOMEBREW_BUMP_VERSION

  license "GPL-2.0-only"

  gh_release = "https://github.com/huffs-projects/tyrannus/releases/download/#{RELEASE_TAG}"

  if OS.mac? && Hardware::CPU.arm?
    url "#{gh_release}/tyrannus-#{RELEASE_TAG}-macos-aarch64.tar.gz"
    sha256 "8b942e9e83b8130a149fda0eaa9af169f2042a526ccf4ded495db7c1b1034fa0" # HOMEBREW_BUMP_MACOS_AARCH64
  elsif OS.linux? && Hardware::CPU.intel?
    url "#{gh_release}/tyrannus-#{RELEASE_TAG}-linux-x86_64.tar.gz"
    sha256 "f19dcd52e15192639fa4ac7e2d7ce7ce49f8d2c78994d0adbdf45f47383c7110" # HOMEBREW_BUMP_LINUX_X86_64
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
