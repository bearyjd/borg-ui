import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [
    sveltekit({
      adapter: adapter({ fallback: 'index.html' }),
      // `$lib` is no longer built in as of SvelteKit v3; keep it via the
      // (deprecated) alias option rather than rewriting ~30 files to subpath imports.
      alias: { $lib: 'src/lib' },
      // The webview had no Content-Security-Policy at all (tauri.conf.json set
      // "csp": null). Nothing in this app injects HTML — there is no {@html},
      // innerHTML, eval or new Function — so it was not exploitable, but every
      // Tauri command is callable from any script in the webview, and those now
      // include changing a repository's real encryption passphrase. A CSP caps
      // what a future injection could reach.
      //
      // Emitted by SvelteKit rather than pinned in tauri.conf.json because
      // SvelteKit's bootstrap is an INLINE script whose hash changes on every
      // build (it embeds a per-build nonce-like global and hashed module paths).
      // A static hash in tauri.conf.json would white-screen the app on the next
      // build; hash mode regenerates it as part of the build.
      csp: {
        mode: 'hash',
        directives: {
          'default-src': ['self'],
          'script-src': ['self'],
          // Svelte ships component CSS as external files, but scoped style
          // injection and the theme toggle still set styles inline.
          'style-src': ['self', 'unsafe-inline'],
          'img-src': ['self', 'data:', 'asset:', 'http://asset.localhost'],
          // Tauri's IPC bridge. Without these, every invoke() fails.
          'connect-src': ['self', 'ipc:', 'http://ipc.localhost'],
          'font-src': ['self', 'data:'],
          // No remote content, no framing, no form posts anywhere.
          'object-src': ['none'],
          'base-uri': ['self'],
          'frame-src': ['none'],
          'form-action': ['none']
        }
      }
    })
  ],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 5183 } : undefined,
    watch: {
      ignored: ['**/src-tauri/**']
    }
  }
});
