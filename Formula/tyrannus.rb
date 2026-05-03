class Tyrannus < Formula
  desc "Terminal word processor"
  homepage "https://github.com/huffs-projects/tyrannus"

  RELEASE_TAG = "0.1.2.2".freeze # HOMEBREW_BUMP_TAG
  version "0.1.2.2" # HOMEBREW_BUMP_VERSION

  license "GPL-2.0-only"

  gh_release = "https://github.com/huffs-projects/tyrannus/releases/download/#{RELEASE_TAG}"

  if OS.mac? && Hardware::CPU.arm?
    url "#{gh_release}/tyrannus-#{RELEASE_TAG}-macos-aarch64.tar.gz"
    sha256 "5f58f50c75d24ff63cde92bfb32cdac7b21e4923d26d420bd3b6765189df1e61" # HOMEBREW_BUMP_MACOS_AARCH64
  elsif OS.linux? && Hardware::CPU.intel?
    url "#{gh_release}/tyrannus-#{RELEASE_TAG}-linux-x86_64.tar.gz"
    sha256 "a118a43459cb894229dca6fdb160a5550372ff0ced6ca0e8507321d83567e419" # HOMEBREW_BUMP_LINUX_X86_64
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
