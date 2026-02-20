import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'sidebar-tui',
  description: 'Manage multiple terminal sessions without leaving your terminal.',
  appearance: 'dark',
  cleanUrls: true,

  head: [
    ['link', { rel: 'icon', href: '/favicon.svg', type: 'image/svg+xml' }],
    ['meta', { name: 'theme-color', content: '#875fff' }],
    ['meta', { property: 'og:title', content: 'sidebar-tui' }],
    ['meta', { property: 'og:description', content: 'Manage multiple terminal sessions without leaving your terminal.' }],
  ],

  themeConfig: {
    logo: '/logo.svg',
    siteTitle: 'sidebar-tui',

    nav: [
      { text: 'Guide', link: '/guide/' },
      { text: 'Keybindings', link: '/guide/keybindings' },
      {
        text: 'GitHub',
        link: 'https://github.com/ftl-tools/sidebar-tui',
        target: '_blank',
      },
    ],

    sidebar: {
      '/guide/': [
        {
          text: 'Getting Started',
          items: [
            { text: 'Installation', link: '/guide/installation' },
            { text: 'Quick Start', link: '/guide/' },
          ],
        },
        {
          text: 'Reference',
          items: [
            { text: 'Keybindings', link: '/guide/keybindings' },
            { text: 'Workspaces', link: '/guide/workspaces' },
          ],
        },
      ],
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/ftl-tools/sidebar-tui' },
    ],

    footer: {
      message: 'Released under the MIT License.',
    },

    search: {
      provider: 'local',
    },
  },
})
