/**
 * The waiting alert's chimes (#1012, ADR-0081, chosen design §3): one short
 * synthesized sound per waiting reason, no audio file. Pure — the recipes and
 * the voice plan they expand to; `soundAdapter.ts` renders a plan with WebAudio.
 *
 * | reason       | chime          |
 * |--------------|----------------|
 * | `question`   | Celesta        | `declared` with a message
 * | `declared`   | Soft gong      | `declared` without a message
 * | `completion` | Kalimba triad  | `completion_not_released`
 * | `incident`   | Glass ping     | an incident park
 *
 * « Send a test » plays the Celesta. The mapping is fixed — not a setting.
 */
import type { WaitingReason } from "./waitingAlert";

export type ChimeName = "celesta" | "softGong" | "kalimbaTriad" | "glassPing";

/** One partial of an additive tone: frequency ratio, relative gain, decay factor. */
export interface Partial {
  ratio: number;
  gain: number;
  /** Multiplies the note's duration for this partial's decay (1 by default). */
  decay: number;
}

export interface ChimeNote {
  /** Hz. */
  freq: number;
  /** Seconds after the chime starts. */
  at: number;
  /** Seconds. */
  dur: number;
  gain: number;
}

/** Frequency modulation of a single sine carrier (the Celesta). */
export interface FmSpec {
  /** Modulator frequency = carrier × ratio. */
  ratio: number;
  /** Modulation index (deviation = index × modulator frequency). */
  index: number;
  /** The index decays to `index × indexFloor`… */
  indexFloor: number;
  /** …over `dur × indexDecay`. */
  indexDecay: number;
}

export interface ChimeRecipe {
  name: ChimeName;
  notes: ChimeNote[];
  /** Additive partials (ignored when `fm` is set: a single FM sine per note). */
  partials: Partial[];
  fm?: FmSpec;
  /** Seconds. */
  attack: number;
  /** Low-pass cutoff, Hz. */
  lowpass: number;
  reverb: { wet: number; tail: number };
}

/** The master gain before the compressor. */
export const CHIME_MASTER_GAIN = 0.32;

const p = (ratio: number, gain: number, decay = 1): Partial => ({ ratio, gain, decay });

const C5 = 523.25;
const E5 = 659.25;
const G5 = 783.99;
const C6 = 1046.5;
const D4 = 293.66;
const E6 = 1318.5;

export const CHIMES: Record<ChimeName, ChimeRecipe> = {
  celesta: {
    name: "celesta",
    notes: [
      { freq: C6, at: 0, dur: 0.9, gain: 0.7 },
      { freq: G5, at: 0.14, dur: 1.2, gain: 0.7 },
    ],
    partials: [p(1, 1)],
    fm: { ratio: 7, index: 1.5, indexFloor: 0.05, indexDecay: 0.6 },
    attack: 0.002,
    lowpass: 9000,
    reverb: { wet: 0.32, tail: 2 },
  },
  softGong: {
    name: "softGong",
    notes: [{ freq: D4, at: 0, dur: 2.6, gain: 0.9 }],
    partials: [p(0.5, 0.35, 1.4), p(1, 1), p(1.19, 0.45, 0.9), p(1.5, 0.3, 0.8), p(2, 0.35, 0.6), p(2.74, 0.15, 0.4), p(3.76, 0.08, 0.3)],
    attack: 0.02,
    lowpass: 2500,
    reverb: { wet: 0.4, tail: 3 },
  },
  kalimbaTriad: {
    name: "kalimbaTriad",
    notes: [
      { freq: C5, at: 0, dur: 0.8, gain: 0.7 },
      { freq: E5, at: 0.09, dur: 0.8, gain: 0.7 },
      { freq: G5, at: 0.18, dur: 1.1, gain: 0.7 },
    ],
    partials: [p(1, 1), p(5.4, 0.18, 0.12), p(2, 0.08, 0.5)],
    attack: 0.002,
    lowpass: 7000,
    reverb: { wet: 0.22, tail: 1.4 },
  },
  glassPing: {
    name: "glassPing",
    notes: [{ freq: E6, at: 0, dur: 1.6, gain: 0.8 }],
    partials: [p(1, 1), p(2.32, 0.3, 0.6), p(4.25, 0.12, 0.35), p(6.63, 0.05, 0.2)],
    attack: 0.002,
    lowpass: 9000,
    reverb: { wet: 0.35, tail: 2.2 },
  },
};

/** The chime of each waiting reason. */
export function chimeForReason(reason: WaitingReason): ChimeName {
  switch (reason) {
    case "question":
      return "celesta";
    case "declared":
      return "softGong";
    case "completion":
      return "kalimbaTriad";
    case "incident":
      return "glassPing";
  }
}

/** « Send a test » plays the chime of a question. */
export const TEST_CHIME: ChimeName = "celesta";

/** One sine oscillator with its envelope, relative to the chime's start. */
export interface Voice {
  freq: number;
  /** Seconds. */
  start: number;
  /** Peak gain = note gain × partial gain / Σ partial gains. */
  peak: number;
  /** End of the linear attack (seconds). */
  peakAt: number;
  /** End of the exponential decay (seconds) — the oscillator stops there. */
  end: number;
  fm?: { freq: number; index: number; floor: number; floorAt: number };
}

/**
 * Expand a recipe into its voices: per note and per partial, a sine whose gain
 * rises linearly to its peak during the attack, then decays exponentially over
 * `dur × decay`.
 */
export function chimeVoices(recipe: ChimeRecipe): Voice[] {
  const partials = recipe.fm ? [p(1, 1)] : recipe.partials;
  const total = partials.reduce((sum, partial) => sum + partial.gain, 0) || 1;
  const voices: Voice[] = [];
  for (const note of recipe.notes) {
    for (const partial of partials) {
      const freq = note.freq * partial.ratio;
      const peakAt = note.at + recipe.attack;
      const voice: Voice = {
        freq,
        start: note.at,
        peak: (note.gain * partial.gain) / total,
        peakAt,
        end: peakAt + note.dur * partial.decay,
      };
      if (recipe.fm) {
        voice.fm = {
          freq: freq * recipe.fm.ratio,
          index: recipe.fm.index,
          floor: recipe.fm.index * recipe.fm.indexFloor,
          floorAt: note.at + note.dur * recipe.fm.indexDecay,
        };
      }
      voices.push(voice);
    }
  }
  return voices;
}

/** How long the whole chime rings, reverb tail included (seconds). */
export function chimeLength(recipe: ChimeRecipe): number {
  const lastVoice = Math.max(...chimeVoices(recipe).map((v) => v.end));
  return lastVoice + recipe.reverb.tail;
}
