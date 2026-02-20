class Sb < Formula
  desc "TUI for managing terminal sessions in a sidebar"
  homepage "https://github.com/ftl-tools/sidebar_tui"
  version "0.1.9"
  license "MIT"

  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/ftl-tools/sidebar_tui/releases/download/v0.1.9/sb-v0.1.9-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_MACOS_ARM64_SHA256"
    end
    if Hardware::CPU.intel?
      url "https://github.com/ftl-tools/sidebar_tui/releases/download/v0.1.9/sb-v0.1.9-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_MACOS_X64_SHA256"
    end
  end
  if OS.linux?
    if Hardware::CPU.arm?
      url "https://github.com/ftl-tools/sidebar_tui/releases/download/v0.1.9/sb-v0.1.9-aarch64-unknown-linux-musl.tar.gz"
      sha256 "PLACEHOLDER_LINUX_ARM64_SHA256"
    end
    if Hardware::CPU.intel?
      url "https://github.com/ftl-tools/sidebar_tui/releases/download/v0.1.9/sb-v0.1.9-x86_64-unknown-linux-musl.tar.gz"
      sha256 "PLACEHOLDER_LINUX_X64_SHA256"
    end
  end

  def install
    bin.install "sb"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/sb --version")
  end
end
