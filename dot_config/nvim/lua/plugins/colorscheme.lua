return {
  {
    "juliarozanova/dopamine-light",
    dir = "~/Dashboard/Code/dopamine-light",
    lazy = false,
    priority = 1000,
    config = function()
      -- Colours come from dopamine-dotfiles' .chezmoidata/palette.toml, written
      -- out by `chezmoi apply` as lua/dopamine_palette.lua. Fall back to the
      -- plugin's own built-in palette if that file isn't there yet (fresh
      -- machine, or nvim config used without chezmoi).
      local ok, palette = pcall(require, "dopamine_palette")

      require("dopamine").setup({
        -- WezTerm only blends cells with no explicit background, so the
        -- colorscheme has to drop its own bg for window_background_opacity to
        -- reach nvim. See the transparency note in wezterm.lua.
        transparent = true,
        palette = ok and palette or nil,
      })
    end,
  },

  {
    "LazyVim/LazyVim",
    opts = {
      colorscheme = "dopamine-dark",
    },
  },
}
