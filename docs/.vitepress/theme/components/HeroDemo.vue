<template>
  <div class="hero-video-wrapper">
    <video
      v-if="showVideo"
      autoplay
      muted
      loop
      playsinline
      preload="metadata"
      class="hero-video"
    >
      <source :src="webmSrc" type="video/webm" />
      <source :src="mp4Src" type="video/mp4" />
    </video>
    <div v-else class="hero-placeholder">
      <div class="placeholder-window">
        <div class="placeholder-titlebar">
          <span class="dot red"></span>
          <span class="dot yellow"></span>
          <span class="dot green"></span>
          <span class="placeholder-title">sidebar-tui — zsh</span>
        </div>
        <pre class="placeholder-content">┌────────────┐┌─────────────────────────┐
│ <span class="ws-name">Default</span>    ││ $ git status            │
│ ...        ││ On branch main          │
│<span class="hl"> api-server </span>││ nothing to commit       │
│ scratchpad ││                         │
│ docs       ││                         │
└────────────┘└─────────────────────────┘
 <span class="kbd">ctrl+n</span> New  <span class="kbd">ctrl+b</span> Sidebar │ <span class="kbd">q</span> Quit</pre>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'
import { withBase } from 'vitepress'

// Paths set dynamically so Vite does not try to resolve them as imports at build time
const webmSrc = ref('')
const mp4Src = ref('')
const showVideo = ref(false)

onMounted(() => {
  const mp4 = withBase('/hero/demo.mp4')
  webmSrc.value = withBase('/hero/demo.webm')
  mp4Src.value = mp4

  fetch(mp4, { method: 'HEAD' })
    .then(r => { if (r.ok) showVideo.value = true })
    .catch(() => {})
})
</script>

<style scoped>
.hero-video-wrapper {
  display: flex;
  justify-content: center;
  align-items: center;
  width: 100%;
  padding: 8px 0;
}

.hero-video {
  width: 100%;
  max-width: 580px;
  border-radius: 10px;
  box-shadow: 0 24px 80px rgba(0, 0, 0, 0.6), 0 0 0 1px rgba(135, 95, 255, 0.2);
}

.hero-placeholder {
  width: 100%;
  max-width: 560px;
}

.placeholder-window {
  background: #0c0c0f;
  border: 1px solid rgba(135, 95, 255, 0.3);
  border-radius: 10px;
  overflow: hidden;
  box-shadow: 0 24px 80px rgba(0, 0, 0, 0.6);
}

.placeholder-titlebar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 10px 14px;
  background: #131318;
  border-bottom: 1px solid rgba(135, 95, 255, 0.12);
}

.placeholder-title {
  margin-left: 6px;
  font-size: 12px;
  color: #606078;
  font-family: var(--vp-font-family-mono, monospace);
}

.dot {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  flex-shrink: 0;
}
.dot.red    { background: #ff5f56; }
.dot.yellow { background: #ffbd2e; }
.dot.green  { background: #27c93f; }

.placeholder-content {
  margin: 0;
  padding: 16px 20px;
  font-family: 'JetBrains Mono', 'Cascadia Code', 'SF Mono', Menlo, Monaco, Consolas, monospace;
  font-size: 12.5px;
  line-height: 1.6;
  color: #e8e8f0;
  background: transparent;
  overflow-x: auto;
  white-space: pre;
}

.ws-name { color: #875fff; }
.hl { background: #2a2a3a; color: #e8e8f0; }
.kbd { color: #875fff; }
</style>
