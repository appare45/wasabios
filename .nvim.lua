local lspconfig = require("lspconfig")

lspconfig.rust_analyzer.setup({
  server = {
    path = "/home/appare45/.rustup/toolchains/nightly-2024-01-01-x86_64-unknown-linux-gnu/bin/rust-analyzer"
  }
})
