/**
 * The chimes of the Alerte d'attente: one short synthesized sound per waiting
 * reason, no audio file. Pure: the recipes and the voices they expand to; the
 * sound adapter renders the voices with WebAudio.
 *
 * | reason                   | chime                      |
 * |--------------------------|----------------------------|
 * | `question`, `declared`   | `rising`: two rising notes |
 * | `completion`             | `soft`: one soft note      |
 * | `incident`               | `falling`: two low notes, descending |
 *
 * The sound's « Test » plays `rising`. The mapping is fixed, not a setting.
 */
import type { WaitingReason } from "./waitingAlert";

export type ChimeName = "rising" | "soft" | "falling";

/** One partial of a note: frequency ratio, relative gain, decay factor. */
export interface ChimePartial {
  ratio: number;
  gain: number;
  /** Multiplies the note's duration for this partial's decay. */
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

export interface ChimeRecipe {
  name: ChimeName;
  notes: ChimeNote[];
  partials: ChimePartial[];
  /** Seconds. */
  attack: number;
  /** Low-pass cutoff, Hz. */
  lowpass: number;
}

/** The master gain of every chime. */
export const CHIME_MASTER_GAIN = 0.3;

const p = (ratio: number, gain: number, decay = 1): ChimePartial => ({ ratio, gain, decay });

const BELL = [p(1, 1), p(2, 0.25, 0.6), p(3.01, 0.08, 0.35)];

export const CHIMES: Record<ChimeName, ChimeRecipe> = {
  // A fourth up, E5 then A5: « look, a question ».
  rising: {
    name: "rising",
    notes: [
      { freq: 659.25, at: 0, dur: 0.5, gain: 0.8 },
      { freq: 880, at: 0.16, dur: 0.9, gain: 0.8 },
    ],
    partials: BELL,
    attack: 0.004,
    lowpass: 8000,
  },
  // One round C5: done, nothing urgent.
  soft: {
    name: "soft",
    notes: [{ freq: 523.25, at: 0, dur: 1.1, gain: 0.7 }],
    partials: [p(1, 1), p(2, 0.12, 0.5)],
    attack: 0.03,
    lowpass: 3000,
  },
  // A minor third down, low, C4 then A3: something went wrong.
  falling: {
    name: "falling",
    notes: [
      { freq: 261.63, at: 0, dur: 0.45, gain: 0.9 },
      { freq: 220, at: 0.2, dur: 0.9, gain: 0.9 },
    ],
    partials: BELL,
    attack: 0.006,
    lowpass: 2500,
  },
};

/** The chime of each waiting reason. */
export function chimeForReason(reason: WaitingReason): ChimeName {
  switch (reason) {
    case "question":
    case "declared":
      return "rising";
    case "completion":
      return "soft";
    case "incident":
      return "falling";
  }
}

/** The sound's « Test » plays the chime of a question. */
export const TEST_CHIME: ChimeName = "rising";

/** One sine with its envelope, in seconds from the chime's start. */
export interface Voice {
  freq: number;
  start: number;
  /** Peak gain = note gain × partial gain / Σ partial gains. */
  peak: number;
  /** End of the linear attack. */
  peakAt: number;
  /** End of the exponential decay: the oscillator stops there. */
  end: number;
}

/** A recipe as its voices: per note and per partial, a sine that rises then decays. */
export function chimeVoices(recipe: ChimeRecipe): Voice[] {
  const total = recipe.partials.reduce((sum, partial) => sum + partial.gain, 0) || 1;
  return recipe.notes.flatMap((note) =>
    recipe.partials.map((partial) => {
      const peakAt = note.at + recipe.attack;
      return {
        freq: note.freq * partial.ratio,
        start: note.at,
        peak: (note.gain * partial.gain) / total,
        peakAt,
        end: peakAt + note.dur * partial.decay,
      };
    }),
  );
}

/** The fundamentals of a chime, in the order they ring. */
export function chimeMelody(name: ChimeName): number[] {
  return CHIMES[name].notes.map((note) => note.freq);
}
