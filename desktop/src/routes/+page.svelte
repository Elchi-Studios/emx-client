<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from '$lib/state.svelte';
  import { emx, describe, retryable, outage } from '$lib/emx';
  import SignIn from '$lib/SignIn.svelte';
  import Shell from '$lib/Shell.svelte';
  import Offline from '$lib/Offline.svelte';
  import Confirm from '$lib/Confirm.svelte';

  let ready = $state(false);
  // Set while a kept sign-in cannot reach EMX: the person is still
  // signed in, so the window says it is offline and keeps trying
  // instead of showing the sign-in form.
  let offline = $state('');
  let title = $state('');
  let trying = $state(false);
  let pause = 2;
  let timer: ReturnType<typeof setTimeout> | undefined;
  // Counts sign-outs. Signing out stays possible while an attempt to
  // connect is under way; when that attempt ends after a sign-out, it
  // finds a newer number and changes nothing, so it neither starts the
  // app nor brings the offline screen back.
  let generation = 0;

  async function resume(): Promise<void> {
    clearTimeout(timer);
    const mine = generation;
    trying = true;
    try {
      const me = await emx.resume();
      if (mine !== generation) return;
      offline = '';
      pause = 2;
      if (me) await app.start(me);
    } catch (e) {
      if (mine !== generation) return;
      if (retryable(e)) {
        offline = describe(e);
        title = outage(e);
        timer = setTimeout(resume, pause * 1000);
        pause = Math.min(pause * 2, 60);
      } else {
        offline = '';
        app.fail(e);
      }
    } finally {
      if (mine === generation) trying = false;
      ready = true;
    }
  }

  async function signOut(): Promise<void> {
    generation++;
    clearTimeout(timer);
    trying = false;
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
  <Offline {title} message={offline} {trying} onretry={resume} onsignout={signOut} />
{:else}
  <SignIn />
{/if}

{#if app.asking}<Confirm />{/if}

{#if app.notice}<div class="toast">{app.notice}</div>{/if}
{#if app.problem}<div class="toast problem">{app.problem}</div>{/if}

<style>
  .center {
    height: 100vh;
    display: grid;
    place-items: center;
  }
</style>
