import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    adapter: adapter({
      fallback: 'index.html'
    }),
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
  }
};

export default config;
