import { describe, expect, it } from "vitest";
import { CHIMES, TEST_CHIME, chimeForReason, chimeLength, chimeVoices } from "./chimes";

describe("chimes (#1012) — one synthesized sound per waiting reason", () => {
  it("maps each reason to its chime; the test plays the Celesta", () => {
    expect(chimeForReason("question")).toBe("celesta");
    expect(chimeForReason("declared")).toBe("softGong");
    expect(chimeForReason("completion")).toBe("kalimbaTriad");
    expect(chimeForReason("incident")).toBe("glassPing");
    expect(TEST_CHIME).toBe("celesta");
  });

  it("Celesta: two FM sines, C6 then G5 140 ms later, the index decaying to 5 %", () => {
    const voices = chimeVoices(CHIMES.celesta);
    expect(voices.map((v) => [v.freq, v.start])).toEqual([
      [1046.5, 0],
      [783.99, 0.14],
    ]);
    expect(voices[0].peak).toBeCloseTo(0.7);
    expect(voices[0].peakAt).toBeCloseTo(0.002);
    expect(voices[0].end).toBeCloseTo(0.902);
    expect(voices[1].end).toBeCloseTo(0.14 + 0.002 + 1.2);
    expect(voices[0].fm).toEqual({ freq: 1046.5 * 7, index: 1.5, floor: 1.5 * 0.05, floorAt: 0.9 * 0.6 });
  });

  it("Soft gong: seven bell partials of D4, peaks normalised by Σ gains", () => {
    const voices = chimeVoices(CHIMES.softGong);
    expect(voices).toHaveLength(7);
    expect(voices.map((v) => v.freq)).toEqual(
      [0.5, 1, 1.19, 1.5, 2, 2.74, 3.76].map((r) => 293.66 * r),
    );
    const total = 0.35 + 1 + 0.45 + 0.3 + 0.35 + 0.15 + 0.08;
    expect(voices[1].peak).toBeCloseTo(0.9 / total);
    expect(voices.reduce((s, v) => s + v.peak, 0)).toBeCloseTo(0.9);
    // Attack 20 ms, the sub-octave rings 1.4 × the note.
    expect(voices[0].peakAt).toBeCloseTo(0.02);
    expect(voices[0].end).toBeCloseTo(0.02 + 2.6 * 1.4);
  });

  it("Kalimba triad: C5, E5, G5 arpeggiated, three partials each", () => {
    const voices = chimeVoices(CHIMES.kalimbaTriad);
    expect(voices).toHaveLength(9);
    expect(voices.filter((v) => v.freq === 523.25 || v.freq === 659.25 || v.freq === 783.99).map((v) => v.start)).toEqual([0, 0.09, 0.18]);
  });

  it("Glass ping: E6 with its inharmonic partials", () => {
    const voices = chimeVoices(CHIMES.glassPing);
    expect(voices.map((v) => v.freq)).toEqual([1, 2.32, 4.25, 6.63].map((r) => 1318.5 * r));
    expect(CHIMES.glassPing.reverb).toEqual({ wet: 0.35, tail: 2.2 });
  });

  it("stays short — every chime rings out within a few seconds", () => {
    for (const recipe of Object.values(CHIMES)) {
      expect(chimeLength(recipe)).toBeLessThan(7);
    }
  });
});
