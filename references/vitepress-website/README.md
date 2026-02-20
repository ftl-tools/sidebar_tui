# VitePress Research: sidebar-tui Marketing Site

Research gathered for building the sidebar-tui marketing/docs website with VitePress.

## Contents of This Folder

```
vitepress-website/
├── README.md             (this file)
├── vitepress/            (full VitePress source repo, shallow clone)
│   ├── src/client/theme-default/    (default theme components + CSS vars)
│   ├── docs/.vitepress/             (VitePress's own site config - real example)
│   └── template/                   (starter template files)
├── starship/             (Starship prompt site - uses VitePress with video embed)
│   └── docs/
│       ├── README.md               (home page with video embed pattern)
│       └── .vitepress/theme/       (custom CSS + theme)
└── mise/                 (mise-en-place site - uses VitePress with custom hero Vue component)
    └── docs/
        ├── index.md                (home page frontmatter)
        └── .vitepress/theme/       (HomeHero.vue custom component + custom.css)
```

---

## 1. VitePress Home Page Frontmatter Schema

The home page uses `layout: home` and a `hero` frontmatter block.

```yaml
---
layout: home

hero:
  name: "sidebar"          # Product name - rendered with brand color (gradient-clipped)
  text: "A terminal sidebar"  # h1 heading text
  tagline: "Switch sessions. Stay focused."  # subtitle
  image:
    src: /logo.svg         # Optional logo/image shown right of text on desktop
    alt: sidebar-tui       # Alt text for accessibility
  actions:
    - theme: brand         # Primary CTA button
      text: Get Started
      link: /guide/
    - theme: alt           # Secondary CTA button
      text: View on GitHub
      link: https://github.com/you/sidebar-tui

features:
  - icon: ⚡
    title: Fast
    details: Built in Rust. Instant response.
  - icon: 🎛️
    title: Keyboard-first
    details: Everything accessible by keyboard.
  - icon: 🔌
    title: Shell agnostic
    details: Works with bash, zsh, fish.
---
```

### TypeScript interfaces (from VitePress source)

```typescript
interface Hero {
  name?: string           // Brand-colored product name
  text: string            // Main h1 heading
  tagline?: string        // Subtitle - supports HTML entities
  image?: ThemeableImage  // Logo shown right of text on desktop
  actions?: HeroAction[]  // CTA buttons
}

type ThemeableImage =
  | string
  | { src: string; alt?: string }
  | { light: string; dark: string; alt?: string }  // different images for light/dark

interface HeroAction {
  theme?: 'brand' | 'alt'
  text: string
  link: string
  target?: string   // e.g. "_blank" for external
  rel?: string
}

interface Feature {
  icon?: string | { src: string; alt?: string; width?: string; height?: string }
  title: string
  details: string
  link?: string
  linkText?: string
  rel?: string
  target?: string
}
```

Source: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/vitepress/src/client/theme-default/components/VPHero.vue`

---

## 2. Embedding Video in the Hero Section

VitePress does NOT have a built-in video field in the hero frontmatter. There are three approaches:

### Approach A: Raw HTML below the hero (Starship pattern - simplest)

Starship's README.md (`/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/starship/docs/README.md`) uses this:

```markdown
---
layout: home
hero:
  image: /logo.svg
  tagline: The minimal, blazing-fast prompt for any shell!
  actions:
    - theme: brand
      text: Get Started →
      link: ./guide/
---

<video class="demo-video" muted autoplay loop playsinline>
  <source src="/demo.webm" type="video/webm">
  <source src="/demo.mp4" type="video/mp4">
</video>
```

The `.demo-video` CSS class in their `index.css` (see below) centers and constrains the video:

```css
.demo-video {
  max-width: 700px;
  width: 100%;
  margin: 50px auto;
  border-radius: 6px;
}
```

This places the video BELOW the hero frontmatter section. Simple and works immediately.

**Note**: VitePress renders raw HTML in markdown by default. For security, confirm `markdown.html: true` is NOT required for this — it works as-is.

### Approach B: Custom Vue component via layout slot (best for video INSIDE hero)

VitePress layout slots let you inject content inside the hero:

Available home hero slots:
- `home-hero-before` - before entire hero
- `home-hero-info-before` - before text/heading
- `home-hero-info` - replaces the text/heading area entirely
- `home-hero-info-after` - after text/heading
- `home-hero-actions-after` - after CTA buttons
- **`home-hero-image`** - replaces the image area on the right (desktop)
- `home-hero-after` - after entire hero
- `home-features-before` / `home-features-after` - around features

Create `.vitepress/theme/MyLayout.vue`:

```vue
<script setup>
import DefaultTheme from 'vitepress/theme'
const { Layout } = DefaultTheme
</script>

<template>
  <Layout>
    <template #home-hero-image>
      <video
        class="hero-video"
        muted
        autoplay
        loop
        playsinline
        style="border-radius: 8px; width: 100%; max-width: 540px;"
      >
        <source src="/demo.webm" type="video/webm">
        <source src="/demo.mp4" type="video/mp4">
      </video>
    </template>
    <template #home-hero-after>
      <!-- optionally put a larger video below the hero buttons too -->
    </template>
  </Layout>
</template>
```

Then in `.vitepress/theme/index.ts`:

```typescript
import DefaultTheme from 'vitepress/theme'
import MyLayout from './MyLayout.vue'
import './custom.css'

export default {
  extends: DefaultTheme,
  Layout: MyLayout,
}
```

### Approach C: Full custom hero component (Mise pattern)

Mise uses a completely custom hero Vue component (`HomeHero.vue`) injected via the `home-hero-info` slot or by overriding the full layout. See:

`/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/mise/docs/.vitepress/theme/HomeHero.vue`

This gives full control but requires maintaining the whole hero from scratch.

### Recommendation for sidebar-tui

Use **Approach A** first (raw video HTML below the hero). It takes 3 lines to implement and gives a clean demo section below the CTAs. If we want the video to replace the hero image slot (right side), use **Approach B**.

The `home-hero-image` slot (Approach B) is ideal if we want the terminal recording to appear on the right side of the page, next to the tagline and buttons — exactly like a product screenshot would.

---

## 3. Code Groups (Tabbed Install Methods)

Code groups use the `::: code-group` container syntax:

```markdown
## Install

::: code-group

```sh [curl]
curl -fsSL https://get.sidebar-tui.com | sh
```

```sh [Homebrew]
brew install your-tap/sidebar-tui
```

```sh [npm]
npm install -g sidebar-tui
```

```sh [bun]
bun add -g sidebar-tui
```

```sh [AUR]
yay -S sidebar-tui-bin
```

```sh [Cargo]
cargo install sidebar-tui
```

:::
```

The labels in `[brackets]` become the tab names. The active tab indicator uses `--vp-code-tab-active-bar-color` (defaults to `--vp-c-brand-1`).

For import-from-file syntax:
```markdown
::: code-group
<<< @/snippets/install-curl.sh [curl]
<<< @/snippets/install-brew.sh [Homebrew]
:::
```

Source: VitePress docs at https://vitepress.dev/guide/markdown#code-groups

The mise site also uses `vitepress-plugin-tabs` for a richer tabbing experience beyond code blocks:
```typescript
import { tabsMarkdownPlugin } from "vitepress-plugin-tabs"
```

---

## 4. Custom CSS Theming (Minimal Dark Design)

### File structure

```
docs/
└── .vitepress/
    └── theme/
        ├── index.ts      # Theme entry
        └── custom.css    # CSS variable overrides
```

### Minimal dark theme CSS starter

Based on the VitePress template + Starship + Mise examples:

```css
/* docs/.vitepress/theme/custom.css */

/* Override brand colors for a terminal/dark aesthetic */
:root {
  /* Primary brand - use a dim cyan or green for TUI feel */
  --vp-c-brand-1: #4ade80;       /* green-400 - primary text/links */
  --vp-c-brand-2: #22c55e;       /* green-500 - hover */
  --vp-c-brand-3: #16a34a;       /* green-600 - active/button bg */
  --vp-c-brand-soft: rgba(74, 222, 128, 0.14);

  /* Hero gradient text on the name field */
  --vp-home-hero-name-color: transparent;
  --vp-home-hero-name-background: linear-gradient(120deg, #4ade80, #22d3ee);

  /* Hero image glow */
  --vp-home-hero-image-background-image: linear-gradient(-45deg, #4ade80 50%, #22d3ee 50%);
  --vp-home-hero-image-filter: blur(44px);
}

/* Force deep dark backgrounds */
.dark {
  --vp-c-bg: #0d0d0d;
  --vp-c-bg-soft: #141414;
  --vp-c-bg-alt: #111111;
  --vp-c-bg-elv: #1a1a1a;

  --vp-c-divider: #2a2a2a;
  --vp-c-border: #333333;

  --vp-c-text-1: #e4e4e7;
  --vp-c-text-2: #a1a1aa;
  --vp-c-text-3: #71717a;

  --vp-code-block-bg: #0a0a0a;

  /* Punch up brand in dark mode */
  --vp-c-brand-1: #4ade80;
  --vp-c-brand-2: #22c55e;
  --vp-c-brand-3: #16a34a;
}

/* Mono font for terminal aesthetic */
:root {
  --vp-font-family-mono: 'JetBrains Mono', 'Fira Code', 'SF Mono', Menlo, Monaco, monospace;
}
```

### Key CSS variables reference

All from `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/vitepress/src/client/theme-default/styles/vars.css`:

| Variable | Purpose |
|---|---|
| `--vp-c-brand-1/2/3` | Brand color scale (text, hover, button bg) |
| `--vp-c-brand-soft` | Semi-transparent brand for badges/containers |
| `--vp-c-bg` | Main background |
| `--vp-c-bg-alt` | Sidebar/code block background |
| `--vp-c-bg-soft` | Subtle bg for tables, carbon ads |
| `--vp-c-bg-elv` | Elevated bg (dialogs) |
| `--vp-c-text-1/2/3` | Primary, muted, subtle text |
| `--vp-c-divider` | Section separators |
| `--vp-c-border` | Interactive element borders |
| `--vp-home-hero-name-color` | Hero name text color (set to `transparent` for gradient) |
| `--vp-home-hero-name-background` | Gradient applied to hero name via background-clip |
| `--vp-home-hero-image-background-image` | Glow blob behind hero image |
| `--vp-home-hero-image-filter` | Blur on glow blob |
| `--vp-code-block-bg` | Code block background |
| `--vp-code-tab-active-bar-color` | Code group active tab indicator color |
| `--vp-font-family-mono` | Monospace font stack |

### VitePress own site styles (good reference)

`/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/vitepress/docs/.vitepress/theme/styles.css` - uses gradient name trick:

```css
:root {
  --vp-home-hero-name-color: transparent;
  --vp-home-hero-name-background: -webkit-linear-gradient(120deg, #bd34fe 30%, #41d1ff);
  --vp-home-hero-image-background-image: linear-gradient(-45deg, #bd34fe 50%, #47caff 50%);
  --vp-home-hero-image-filter: blur(44px);
}
```

---

## 5. GitHub Pages Deployment Workflow

Save to `.github/workflows/deploy.yml`:

```yaml
name: Deploy VitePress site to Pages

on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: pages
  cancel-in-progress: false

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4
        with:
          fetch-depth: 0    # needed for lastUpdated feature

      - name: Setup Node
        uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm

      - name: Setup Pages
        uses: actions/configure-pages@v4

      - name: Install dependencies
        run: npm ci

      - name: Build with VitePress
        run: npm run docs:build

      - name: Upload artifact
        uses: actions/upload-pages-artifact@v3
        with:
          path: docs/.vitepress/dist

  deploy:
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    needs: build
    runs-on: ubuntu-latest
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
```

### Required GitHub repo configuration

1. Go to repo Settings > Pages
2. Under "Build and deployment" > "Source", select "GitHub Actions"
3. If deploying to a subdirectory (e.g. `username.github.io/sidebar-tui`), add `base` to VitePress config:

```typescript
// docs/.vitepress/config.ts
export default defineConfig({
  base: '/sidebar-tui/',  // Only needed if NOT using custom domain
  // ...
})
```

If using a custom domain (`sidebar-tui.dev`), omit the `base` option entirely.

---

## 6. VitePress Config Structure

```typescript
// docs/.vitepress/config.ts
import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'sidebar',
  description: 'A Rust TUI sidebar for your terminal',
  appearance: 'dark',  // Default to dark mode (mise uses this)

  cleanUrls: true,      // /guide/ instead of /guide.html
  lastUpdated: true,    // Shows "last updated" in docs

  head: [
    ['link', { rel: 'icon', href: '/favicon.ico' }],
    ['meta', { name: 'theme-color', content: '#4ade80' }],
    ['meta', { property: 'og:image', content: 'https://sidebar-tui.dev/og.png' }],
  ],

  themeConfig: {
    logo: '/logo.svg',
    nav: [
      { text: 'Guide', link: '/guide/' },
      { text: 'GitHub', link: 'https://github.com/you/sidebar-tui' },
    ],
    sidebar: {
      '/guide/': [
        {
          text: 'Getting Started',
          items: [
            { text: 'Installation', link: '/guide/installation' },
            { text: 'Quick Start', link: '/guide/quick-start' },
          ]
        }
      ]
    },
    socialLinks: [
      { icon: 'github', link: 'https://github.com/you/sidebar-tui' },
    ],
    footer: {
      message: 'Released under the MIT License.',
    },
    search: {
      provider: 'local',  // Free built-in search
    }
  }
})
```

---

## 7. Theme Entry File

```typescript
// docs/.vitepress/theme/index.ts
import DefaultTheme from 'vitepress/theme'
import './custom.css'

export default {
  extends: DefaultTheme,
}
```

Or with a custom layout for the hero video slot:

```typescript
import DefaultTheme from 'vitepress/theme'
import MyLayout from './MyLayout.vue'
import './custom.css'

export default {
  extends: DefaultTheme,
  Layout: MyLayout,
}
```

---

## 8. Minimal Project Structure

```
docs/
├── index.md                    # Home page (layout: home + frontmatter)
├── guide/
│   ├── index.md               # /guide/ page
│   ├── installation.md
│   └── quick-start.md
├── public/
│   ├── favicon.ico
│   ├── logo.svg
│   ├── demo.webm              # Terminal recording video
│   └── demo.mp4               # Fallback format
└── .vitepress/
    ├── config.ts
    └── theme/
        ├── index.ts
        ├── custom.css
        └── MyLayout.vue       # If using custom hero slot
```

`package.json` scripts:
```json
{
  "scripts": {
    "docs:dev": "vitepress dev docs",
    "docs:build": "vitepress build docs",
    "docs:preview": "vitepress preview docs"
  },
  "devDependencies": {
    "vitepress": "^1.5.0"
  }
}
```

---

## 9. Real-World References in This Folder

### Starship (most relevant - TUI CLI tool using VitePress with video)

- Home page with video: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/starship/docs/README.md`
- Custom CSS (brand colors + demo-video class): `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/starship/docs/.vitepress/theme/index.css`
- VitePress config: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/starship/docs/.vitepress/config.mts`

Key pattern from Starship: drop a `<video>` tag directly in markdown AFTER the frontmatter block. The video autoplays, loops, muted. Styled with `max-width: 700px; border-radius: 6px; margin: 50px auto;`.

### Mise (custom hero Vue component + dark CLI aesthetic)

- Home page frontmatter: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/mise/docs/index.md`
- Custom hero component: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/mise/docs/.vitepress/theme/HomeHero.vue`
- Dark CSS theme: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/mise/docs/.vitepress/theme/custom.css`
- VitePress config: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/mise/docs/.vitepress/config.ts`

Key patterns from Mise: `appearance: "dark"` to default dark mode, pure black backgrounds (`#0a0a0b`), custom `HomeHero.vue` with animated gradient orbs. Also uses `vitepress-plugin-group-icons` and `vitepress-plugin-tabs`.

### VitePress Source (default theme internals)

- All CSS variables: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/vitepress/src/client/theme-default/styles/vars.css`
- Hero Vue component: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/vitepress/src/client/theme-default/components/VPHero.vue`
- VitePress's own site config: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/vitepress/docs/.vitepress/config.ts`
- VitePress's own site styles: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/vitepress/docs/.vitepress/theme/styles.css`
- Project template style.css: `/Users/melchiahmauck/dev/tools/sidebar_tui/references/vitepress-website/vitepress/template/.vitepress/theme/style.css`

---

## 10. Recommended Approach for sidebar-tui

Based on this research, the recommended implementation plan:

1. **Video**: Use Starship's approach - drop `<video muted autoplay loop playsinline>` directly below the frontmatter in `index.md`. Record with VHS (see separate terminal-recording research).

2. **Install tabs**: Use `::: code-group` with labels `[curl]`, `[Homebrew]`, `[npm]`, `[bun]`, `[AUR]`, `[Cargo]`.

3. **Dark theme**: Override CSS variables in `custom.css`. Set `appearance: 'dark'` in config. Use a dim green or cyan as `--vp-c-brand-*`. Set `--vp-c-bg: #0d0d0d` in `.dark`.

4. **Hero name gradient**: Set `--vp-home-hero-name-color: transparent` and `--vp-home-hero-name-background: linear-gradient(...)` for the colored gradient name effect.

5. **GitHub Pages**: Use the workflow above. Do NOT set `base` if using a custom domain.

6. **No custom Vue needed**: Starship proves you can get a great result with plain frontmatter + one `<video>` tag + CSS overrides. Save Vue components for later polish if needed.
