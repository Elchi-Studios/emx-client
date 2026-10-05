<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from '$lib/state.svelte';
  import { emx, describe, retryable } from '$lib/emx';
  import SignIn from '$lib/SignIn.svelte';
  import Shell from '$lib/Shell.svelte';
  import Offline from '$lib/Offline.svelte';

  let ready = $state(false);
  // Set while a kept sign-in cannot reach EMX: the person is still
  // signed in, so the window says it is offline and keeps trying
  // instead of showing the sign-in form.
  let offline = $state('');
  let trying = $state(false);
  let pause = 2;
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function resume(): Promise<void> {
    clearTimeout(timer);
    trying = true;
    try {
      const me = await emx.resume();
      offline = '';
      pause = 2;
      if (me) await app.start(me);
    } catch (e) {
      if (retryable(e)) {
        offline = describe(e);
        timer = setTimeout(resume, pause * 1000);
        pause = Math.min(pause * 2, 60);
      } else {
        offline = '';
        app.fail(e);
      }
    } finally {
      trying = false;
      ready = true;
    }
  }

  async function signOut(): Promise<void> {
    clearTimeout(timer);
    await emx.signOut();
    offline = '';
  }

  function online(): void {
    if (offline && !trying) {
      pause = 2;
      resume();
    }
  }

  onMount(() => {
    resume();
    return () => clearTimeout(timer);
  });
</script>

<svelte:window ononline={online} />

{#if !ready}
  <div class="center muted">Starting</div>
{:else if app.me}
  <Shell />
{:else if offline}
  <Offline message={offline} {trying} onretry={resume} onsignout={signOut} />
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
