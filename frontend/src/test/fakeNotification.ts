/**
 * A recording stand-in for the browser's Notification API (#1011): jsdom has
 * none. `install` puts it on `window`, with the permission the test wants and
 * the answer `requestPermission` will give; `shown` records every notification.
 */
export interface FakeNotificationRecord {
  title: string;
  body?: string;
  tag?: string;
  onclick: ((event?: Event) => void) | null;
  closed: boolean;
  close: () => void;
}

export interface FakeNotificationControl {
  shown: FakeNotificationRecord[];
  requests: number;
  setPermission: (p: NotificationPermission) => void;
  /** What the next `requestPermission()` resolves to (and leaves as the permission). */
  answer: NotificationPermission;
  /** Hold `requestPermission()` until `release()` (the prompt stays open). */
  hold: boolean;
  release: () => void;
}

export function installFakeNotification(
  initial: NotificationPermission = "default",
): FakeNotificationControl {
  let pending: (() => void) | null = null;
  const control: FakeNotificationControl = {
    shown: [],
    requests: 0,
    answer: "granted",
    hold: false,
    setPermission: (p) => {
      FakeNotification.permission = p;
    },
    release: () => {
      pending?.();
      pending = null;
    },
  };

  class FakeNotification {
    static permission: NotificationPermission = initial;
    static requestPermission(): Promise<NotificationPermission> {
      control.requests += 1;
      const settle = () => {
        FakeNotification.permission = control.answer;
      };
      if (control.hold) {
        return new Promise((resolve) => {
          pending = () => {
            settle();
            resolve(FakeNotification.permission);
          };
        });
      }
      settle();
      return Promise.resolve(FakeNotification.permission);
    }
    title: string;
    body?: string;
    tag?: string;
    onclick: ((event?: Event) => void) | null = null;
    closed = false;
    constructor(title: string, options: NotificationOptions = {}) {
      this.title = title;
      this.body = options.body;
      this.tag = options.tag;
      control.shown.push(this);
    }
    close() {
      this.closed = true;
    }
  }

  Object.defineProperty(window, "Notification", {
    configurable: true,
    writable: true,
    value: FakeNotification,
  });
  return control;
}

export function uninstallFakeNotification(): void {
  delete (window as unknown as { Notification?: unknown }).Notification;
}
