local lspconfig = require("lspconfig")

lspconfig.rust_analyzer.setup({
  cmd = "$(rustup which rust-analyzer)"
})
