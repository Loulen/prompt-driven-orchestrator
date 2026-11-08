/**
 * The browser side of the Alerte d'attente's sound: a chime of
 * `run/model/chimes.ts` rendered with WebAudio. No audio file: sines, their
 * envelopes, a low-pass, a compressor.
 *
 * Every call is guarded: a browser without WebAudio, or one that refuses to
 * start the context, plays nothing and raises nothing. One `AudioContext` for
 * the page (browsers cap how many may live); each chime builds its own short
 * chain.
 */
import { CHIME_MASTER_GAIN, CHIMES, chimeVoices, type ChimeName, type ChimeRecipe } from "../model/chimes";

type AudioContextCtor = typeof AudioContext;

let context: AudioContext | null = null;

function audioContextCtor(): AudioContextCtor | null {
  if (typeof window === "undefined") return null;
  const w = window as unknown as { AudioContext?: AudioContextCtor; webkitAudioContext?: AudioContextCtor };
  return w.AudioContext ?? w.webkitAudioContext ?? null;
}

/**
 * May this page start a sound now? Browsers only let a page the user has
 * interacted with ring (autoplay policy): the tab coordinator prefers such a
 * tab to emit. A browser without `userActivation` reads as yes.
 */
export function canPlaySoundNow(): boolean {
  if (!audioContextCtor()) return false;
  const activation = (navigator as unknown as { userActivation?: { hasBeenActive: boolean } }).userActivation;
  return activation ? activation.hasBeenActive : true;
}

function getContext(): AudioContext | null {
  if (context && context.state !== "closed") return context;
  const Ctor = audioContextCtor();
  if (!Ctor) return null;
  try {
    context = new Ctor();
  } catch {
    context = null;
  }
  return context;
}

function render(ctx: AudioContext, recipe: ChimeRecipe): void {
  const t0 = ctx.currentTime + 0.01;
  const compressor = ctx.createDynamicsCompressor();
  compressor.connect(ctx.destination);
  const master = ctx.createGain();
  master.gain.value = CHIME_MASTER_GAIN;
  master.connect(compressor);
  const lowpass = ctx.createBiquadFilter();
  lowpass.type = "lowpass";
  lowpass.frequency.value = recipe.lowpass;
  lowpass.connect(master);

  const voices = chimeVoices(recipe);
  let ringing = voices.length;
  // Tear the chain down once the last oscillator has ended.
  const ended = () => {
    ringing -= 1;
    if (ringing > 0) return;
    try {
      compressor.disconnect();
    } catch {
      // already gone
    }
  };
  for (const voice of voices) {
    const osc = ctx.createOscillator();
    osc.type = "sine";
    osc.frequency.value = voice.freq;
    const env = ctx.createGain();
    env.gain.setValueAtTime(0, t0 + voice.start);
    env.gain.linearRampToValueAtTime(voice.peak, t0 + voice.peakAt);
    env.gain.exponentialRampToValueAtTime(0.0001, t0 + voice.end);
    osc.connect(env);
    env.connect(lowpass);
    osc.onended = ended;
    osc.start(t0 + voice.start);
    osc.stop(t0 + voice.end + 0.05);
  }
}

/** Play one chime. `false` when this browser cannot (no WebAudio, or a context that will not start). */
export function playChime(name: ChimeName): boolean {
  const ctx = getContext();
  if (!ctx) return false;
  try {
    if (ctx.state === "suspended") {
      // Never interacted with: the browser would keep it suspended, and the
      // chime would ring late, on top of a later one. Play nothing instead.
      if (!canPlaySoundNow()) return false;
      void ctx.resume().catch(() => {});
    }
    render(ctx, CHIMES[name]);
    return true;
  } catch {
    return false;
  }
}

/** Test seam: forget the page's context. */
export function resetSoundAdapter(): void {
  context = null;
}
