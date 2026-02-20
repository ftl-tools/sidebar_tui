import { defineConfig } from "vitepress";

export default defineConfig({
  title: "Sidebar TUI",
  description:
    "Manage multiple terminal sessions without leaving your terminal.",
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
          "Manage multiple terminal sessions without leaving your terminal.",
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

    sidebar: {
      "/guide/": [
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
            { text: "Workspaces", link: "/guide/workspaces" },
          ],
        },
      ],
    },

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
