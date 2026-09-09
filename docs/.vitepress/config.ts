import { defineConfig } from "vitepress";

export default defineConfig({
  title: "Sidebar TUI",
  description:
    "Manage multiple terminal windows without leaving your terminal.",
  // Required for GitHub Pages project sites (served at /sidebar_tui/, not /)
  base: "/sidebar_tui/",
  // 'dark' allows toggling; 'force-dark' disables the toggle and locks to dark mode
  appearance: "force-dark",
  cleanUrls: true,

  head: [
    ["link", { rel: "icon", href: "/favicon.svg", type: "image/svg+xml" }],
    ["meta", { name: "theme-color", content: "#875fff" }],
    ["meta", { property: "og:title", content: "Sidebar TUI" }],
    [
      "meta",
      {
        property: "og:description",
        content:
          "Manage multiple terminal windows without leaving your terminal.",
      },
    ],
  ],

  themeConfig: {
    logo: "/logo.svg",
    siteTitle: "Sidebar TUI",

    nav: [
      { text: "Quickstart", link: "/guide/" },
      { text: "Keybindings", link: "/guide/keybindings" },
      {
        text: "GitHub",
        link: "https://github.com/ftl-tools/sidebar_tui",
        target: "_blank",
      },
    ],

    // The route-scoped sidebar hid navigation outside the guide; one shared list
    // keeps the migration plan and current reference pages discoverable together.
    sidebar: [
      {
        text: "Getting Started",
        items: [
          { text: "Installation", link: "/guide/installation" },
          { text: "Quickstart", link: "/guide/" },
        ],
      },
      {
        text: "Reference",
        items: [
          { text: "Keybindings", link: "/guide/keybindings" },
          { text: "Sessions", link: "/guide/sessions" },
          { text: "Terminology & compatibility", link: "/guide/terminology" },
          { text: "tmux chooser & inspection", link: "/guide/tmux_preview" },
          { text: "Native sidebar demo (approval pending)", link: "/guide/tmux_sidebar_demo" },
          { text: "tmux migration progress", link: "/guide/tmux_migration_progress" },
          {
            text: "tmux migration plan",
            link: "/guide/tmux_migration_plan",
          },
        ],
      },
    ],

    // socialLinks: [
    //   { icon: "github", link: "https://github.com/ftl-tools/sidebar_tui" },
    // ],

    footer: {
      message: "Released under the MIT License.",
    },

    // search: {
    //   provider: "local",
    // },
  },
});
