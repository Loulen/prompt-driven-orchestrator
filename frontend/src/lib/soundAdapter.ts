/**
 * The browser side of the waiting alert's sound (#1012, ADR-0081): render a
 * chime of `chimes.ts` with WebAudio. No audio file — sines, envelopes, a
 * low-pass, a generated reverb.
 *
 * Every call is guarded: a browser without WebAudio, or one that refuses to
 * start the context, plays nothing and raises nothing. One `AudioContext` is
 * kept for the page (browsers cap how many may live); each chime builds its own
 * short-lived chain (filter → dry + reverb → master → compressor → output).
 */
import {
  CHIME_MASTER_GAIN,
  CHIMES,
  chimeVoices,
  type ChimeName,
  type ChimeRecipe,
} from "./chimes";

type AudioContextCtor = typeof AudioContext;

let context: AudioContext | null = null;

function audioContextCtor(): AudioContextCtor | null {
  if (typeof window === "undefined") return null;
  const w = window as unknown as { AudioContext?: AudioContextCtor; webkitAudioContext?: AudioContextCtor };
  return w.AudioContext ?? w.webkitAudioContext ?? null;
}

/** Can this browser play a chime at all? */
export function soundSupported(): boolean {
  return audioContextCtor() !== null;
}

/**
 * Has the user interacted with this page yet? Browsers only let a page that
 * was interacted with start its sound (autoplay policy); the tab coordinator
 * prefers such a tab to emit. Browsers without `userActivation` read as yes.
 */
export function canPlaySoundNow(): boolean {
  if (!soundSupported()) return false;
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

/** Noise × (1 − t)³, stereo, `tail` seconds long. */
function impulseResponse(ctx: AudioContext, tail: number): AudioBuffer {
  const length = Math.max(1, Math.floor(ctx.sampleRate * tail));
  const buffer = ctx.createBuffer(2, length, ctx.sampleRate);
  for (let channel = 0; channel < 2; channel += 1) {
    const data = buffer.getChannelData(channel);
    for (let i = 0; i < length; i += 1) {
      data[i] = (Math.random() * 2 - 1) * (1 - i / length) ** 3;
    }
  }
  return buffer;
}

function render(ctx: AudioContext, recipe: ChimeRecipe): void {
  const t0 = ctx.currentTime + 0.01;

  const compressor = ctx.createDynamicsCompressor();
  let ringing = 0;
  // Tear the chain down once the last oscillator has ended.
  const ended = () => {
    ringing -= 1;
    if (ringing === 0) {
      try {
        compressor.disconnect();
      } catch {
        // already gone
      }
    }
  };
  compressor.connect(ctx.destination);
  const master = ctx.createGain();
  master.gain.value = CHIME_MASTER_GAIN;
  master.connect(compressor);

  const lowpass = ctx.createBiquadFilter();
  lowpass.type = "lowpass";
  lowpass.frequency.value = recipe.lowpass;

  const dry = ctx.createGain();
  dry.gain.value = 1 - recipe.reverb.wet / 2;
  lowpass.connect(dry);
  dry.connect(master);

  const convolver = ctx.createConvolver();
  convolver.buffer = impulseResponse(ctx, recipe.reverb.tail);
  const wet = ctx.createGain();
  wet.gain.value = recipe.reverb.wet;
  lowpass.connect(convolver);
  convolver.connect(wet);
  wet.connect(master);

  for (const voice of chimeVoices(recipe)) {
    const osc = ctx.createOscillator();
    osc.type = "sine";
    osc.frequency.value = voice.freq;
    const env = ctx.createGain();
    env.gain.setValueAtTime(0, t0 + voice.start);
    env.gain.linearRampToValueAtTime(voice.peak, t0 + voice.peakAt);
    env.gain.exponentialRampToValueAtTime(0.0001, t0 + voice.end);
    osc.connect(env);
    env.connect(lowpass);

    if (voice.fm) {
      const modulator = ctx.createOscillator();
      modulator.type = "sine";
      modulator.frequency.value = voice.fm.freq;
      const depth = ctx.createGain();
      // Deviation (Hz) = index × modulator frequency.
      depth.gain.setValueAtTime(voice.fm.index * voice.fm.freq, t0 + voice.start);
      depth.gain.exponentialRampToValueAtTime(voice.fm.floor * voice.fm.freq, t0 + voice.fm.floorAt);
      modulator.connect(depth);
      depth.connect(osc.frequency);
      modulator.start(t0 + voice.start);
      modulator.stop(t0 + voice.end + 0.05);
    }
    ringing += 1;
    osc.onended = ended;
    osc.start(t0 + voice.start);
    osc.stop(t0 + voice.end + 0.05);
  }
}

/**
 * Play one chime. Returns `false` when this browser cannot (no WebAudio, or the
 * context refused to start).
 */
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
