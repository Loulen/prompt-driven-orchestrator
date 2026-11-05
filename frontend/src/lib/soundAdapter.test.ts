import { afterEach, describe, expect, it } from "vitest";
import { installFakeAudioContext, uninstallFakeAudioContext } from "../test/fakeAudioContext";
import { canPlaySoundNow, playChime, resetSoundAdapter, soundSupported } from "./soundAdapter";

afterEach(() => {
  uninstallFakeAudioContext();
  resetSoundAdapter();
});

describe("soundAdapter (#1012)", () => {
  it("no WebAudio: plays nothing, raises nothing", () => {
    expect(soundSupported()).toBe(false);
    expect(canPlaySoundNow()).toBe(false);
    expect(playChime("celesta")).toBe(false);
  });

  it("renders a chime as its sines — one output chain per chime, one context per page", () => {
    const audio = installFakeAudioContext();
    expect(playChime("glassPing")).toBe(true);
    expect(playChime("celesta")).toBe(true);
    expect(audio.contexts).toBe(1);
    expect(audio.chimes).toHaveLength(2);
    expect(audio.chimes[0].frequencies).toEqual([1, 2.32, 4.25, 6.63].map((r) => 1318.5 * r));
    // Celesta: two carriers, each with its FM modulator.
    expect(audio.chimes[1].frequencies).toEqual([1046.5, 783.99]);
    expect(audio.chimes[1].oscillators).toBe(4);
  });

  it("suspended and never interacted with: plays nothing rather than ring late", () => {
    const audio = installFakeAudioContext("suspended");
    Object.defineProperty(navigator, "userActivation", { configurable: true, value: { hasBeenActive: false } });
    try {
      expect(canPlaySoundNow()).toBe(false);
      expect(playChime("celesta")).toBe(false);
      expect(audio.chimes).toHaveLength(0);
    } finally {
      Reflect.deleteProperty(navigator, "userActivation");
    }
  });

  it("resumes a suspended context before playing", () => {
    const audio = installFakeAudioContext("suspended");
    playChime("softGong");
    expect(audio.resumes).toBe(1);
    expect(audio.chimes[0].frequencies).toHaveLength(7);
  });
});
