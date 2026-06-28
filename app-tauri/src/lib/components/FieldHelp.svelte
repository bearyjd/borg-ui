<script lang="ts">
  interface Example {
    input: string;
    output?: string;
  }

  interface Props {
    text?: string;
    examples?: Example[];
    children?: import('svelte').Snippet;
  }

  let { text, examples, children }: Props = $props();
</script>

<div class="field-help">
  {#if text}
    <p class="help-text">{text}</p>
  {/if}
  {#if children}
    <p class="help-text">{@render children()}</p>
  {/if}
  {#if examples && examples.length > 0}
    <p class="help-examples">
      <span class="eg">e.g.</span>
      {#each examples as ex, i (ex.input)}<span class="example"><code>{ex.input}</code>{#if ex.output}<span class="arrow" aria-hidden="true">→</span><code class="output">{ex.output}</code>{/if}</span>{#if i < examples.length - 1}<span class="sep" aria-hidden="true">·</span>{/if}{/each}
    </p>
  {/if}
</div>

<style>
  .field-help {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .help-text {
    font-size: var(--text-xs);
    color: var(--color-text-dim);
    line-height: 1.5;
  }

  .help-text :global(code) {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--color-text-muted);
    background: var(--color-bg);
    padding: 1px 4px;
    border-radius: var(--radius-sm);
  }

  /* Examples render as a subtle inline hint, not a bordered box — a box with
     a background + monospace text reads as a second, disabled input field. */
  .help-examples {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1) var(--space-2);
    font-size: 0.7rem;
    color: var(--color-text-dim);
  }

  .help-examples .eg {
    font-style: italic;
  }

  .help-examples .example {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }

  .help-examples code {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--color-text-muted);
  }

  .help-examples code.output {
    color: var(--color-accent);
  }

  .help-examples .arrow,
  .help-examples .sep {
    color: var(--color-text-dim);
  }
</style>
