<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from '$lib/state.svelte';
  import { emx } from '$lib/emx';
  import SignIn from '$lib/SignIn.svelte';
  import Shell from '$lib/Shell.svelte';

  let ready = $state(false);

  onMount(async () => {
    try {
      const me = await emx.resume();
      if (me) await app.start(me);
    } catch (e) {
      app.fail(e);
    } finally {
      ready = true;
    }
  });
</script>

{#if !ready}
  <div class="center muted">Starting</div>
{:else if app.me}
  <Shell />
{:else}
  <SignIn />
{/if}

{#if app.notice}<div class="toast">{app.notice}</div>{/if}
{#if app.problem}<div class="toast problem">{app.problem}</div>{/if}

<style>
  .center {
    height: 100vh;
    display: grid;
    place-items: center;
  }
</style>
