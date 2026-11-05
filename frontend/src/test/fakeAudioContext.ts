/**
 * A recording stand-in for WebAudio (#1012): jsdom has none. `install` puts an
 * `AudioContext` on `window` whose nodes do nothing but record. Each chime
 * builds its own output chain, so every `createDynamicsCompressor()` opens a
 * new entry of `chimes`; the oscillators started after it land in that entry.
 */
export interface RecordedChime {
  /** Frequencies of the carrier oscillators started (FM modulators excluded). */
  frequencies: number[];
  /** Every oscillator started, modulators included. */
  oscillators: number;
}

export interface FakeAudioControl {
  chimes: RecordedChime[];
  contexts: number;
  resumes: number;
}

class FakeParam {
  value = 0;
  setValueAtTime(v: number) {
    this.value = v;
    return this;
  }
  linearRampToValueAtTime(v: number) {
    this.value = v;
    return this;
  }
  exponentialRampToValueAtTime(v: number) {
    this.value = v;
    return this;
  }
}

class FakeNode {
  connected: unknown[] = [];
  connect(target: unknown) {
    this.connected.push(target);
    return target;
  }
  disconnect() {}
}

export function installFakeAudioContext(initialState: AudioContextState = "running"): FakeAudioControl {
  const control: FakeAudioControl = { chimes: [], contexts: 0, resumes: 0 };

  class FakeOscillator extends FakeNode {
    type = "sine";
    frequency = new FakeParam();
    start() {
      const chime = control.chimes[control.chimes.length - 1];
      if (!chime) return;
      chime.oscillators += 1;
      // A modulator feeds another oscillator's frequency, not the chain.
      const isModulator = this.connected.some(
        (n) => n instanceof FakeGain && n.connected.some((m) => m instanceof FakeParam),
      );
      if (!isModulator) chime.frequencies.push(this.frequency.value);
    }
    stop() {}
  }
  class FakeGain extends FakeNode {
    gain = new FakeParam();
  }

  class FakeAudioContext {
    state: AudioContextState = initialState;
    currentTime = 0;
    sampleRate = 8000;
    destination = new FakeNode();
    constructor() {
      control.contexts += 1;
    }
    resume() {
      control.resumes += 1;
      this.state = "running";
      return Promise.resolve();
    }
    createOscillator() {
      return new FakeOscillator();
    }
    createGain() {
      return new FakeGain();
    }
    createBiquadFilter() {
      return Object.assign(new FakeNode(), { type: "lowpass", frequency: new FakeParam() });
    }
    createConvolver() {
      return Object.assign(new FakeNode(), { buffer: null as unknown });
    }
    createDynamicsCompressor() {
      control.chimes.push({ frequencies: [], oscillators: 0 });
      return new FakeNode();
    }
    createBuffer(channels: number, length: number) {
      const data = Array.from({ length: channels }, () => new Float32Array(length));
      return { getChannelData: (c: number) => data[c] };
    }
  }

  Object.defineProperty(window, "AudioContext", {
    configurable: true,
    writable: true,
    value: FakeAudioContext,
  });
  return control;
}

export function uninstallFakeAudioContext(): void {
  delete (window as unknown as { AudioContext?: unknown }).AudioContext;
}
