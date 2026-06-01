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
    <ul class="help-examples">
      {#each examples as ex (ex.input)}
        <li>
          <code>{ex.input}</code>
          {#if ex.output}
            <span class="arrow" aria-hidden="true">→</span>
            <code class="output">{ex.output}</code>
          {/if}
        </li>
      {/each}
    </ul>
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

  .help-examples {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-sm);
  }

  .help-examples li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    font-size: 0.7rem;
  }

  .help-examples code {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--color-text-muted);
  }

  .help-examples code.output {
    color: var(--color-accent);
  }

  .help-examples .arrow {
    color: var(--color-text-dim);
    font-size: 0.7rem;
  }
</style>
