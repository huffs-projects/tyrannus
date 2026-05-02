class Tyrannus < Formula
  desc "Terminal word processor"
  homepage "https://github.com/huffs-projects/tyrannus"

  RELEASE_TAG = "0.1.0".freeze # HOMEBREW_BUMP_TAG
  version "0.1.0" # HOMEBREW_BUMP_VERSION

  license "GPL-2.0-only"

  gh_release = "https://github.com/huffs-projects/tyrannus/releases/download/#{RELEASE_TAG}"

  if OS.mac? && Hardware::CPU.arm?
    url "#{gh_release}/tyrannus-#{RELEASE_TAG}-macos-aarch64.tar.gz"
    sha256 "062855c655e262cf95e7a29b4c5032746371ef25aa94d78d980123a674d4cf98" # HOMEBREW_BUMP_MACOS_AARCH64
  elsif OS.linux? && Hardware::CPU.intel?
    url "#{gh_release}/tyrannus-#{RELEASE_TAG}-linux-x86_64.tar.gz"
    sha256 "02a3d97abfc446f8ed61b68312a49033756b1cd743597759424f2692b2475c07" # HOMEBREW_BUMP_LINUX_X86_64
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
