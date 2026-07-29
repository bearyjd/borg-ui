<script lang="ts">
  interface Props {
    open: boolean;
    busy: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  }

  let { open, busy, onConfirm, onCancel }: Props = $props();

  // Close with Escape, mirroring click-backdrop-to-close.
  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCancel();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
</script>

{#if open}
  <div class="modal-backdrop" onclick={onCancel} role="presentation">
    <div
      class="modal"
      onclick={(e) => e.stopPropagation()}
      onkeydown={() => {}}
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="overwrite-key-title"
    >
      <h2 id="overwrite-key-title">Replace generated SSH key?</h2>
      <p>A BorgUI-managed SSH key already exists. Replacing it will prevent server access until you install the new public key on the server.</p>
      <div class="modal-actions">
        <button type="button" class="btn btn-secondary" onclick={onCancel}>Cancel</button>
        <button type="button" class="btn btn-delete-confirm" disabled={busy} onclick={onConfirm}>
          {busy ? 'Replacing…' : 'Replace key'}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: var(--color-backdrop);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    max-width: 440px;
    width: 90%;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .modal h2 {
    font-size: var(--text-lg);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .modal p {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
</style>
