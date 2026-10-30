/**
 * The Acogni mark: the gear, drawn in the current colour, and its triangle in
 * the ruby, under every Theme (`brand.css`). The app's logo, where the name does not fit.
 */
const GEAR =
  "M-4.6 -19.8L-6.2 -19.4L-7.8 -18.8L-9.3 -18.1L-10.8 -17.3L-11.5 -18.2L-13.6 -20.8L-15.1 -22.4L-16.3 -23.1L-17.2 -23L-23 -17.2L-23.1 -16.3L-22.4 -15.1L-20.8 -13.6L-18.2 -11.5L-17.3 -10.8L-18.1 -9.3L-18.8 -7.8L-19.4 -6.2L-19.8 -4.6L-21 -4.7L-24.3 -5L-26.5 -5.1L-27.9 -4.8L-28.4 -4.1L-28.4 4.1L-27.9 4.8L-26.5 5.1L-24.3 5L-21 4.7L-19.8 4.6L-19.4 6.2L-18.8 7.8L-18.1 9.3L-17.3 10.8L-18.2 11.5L-20.8 13.6L-22.4 15.1L-23.1 16.3L-23 17.2L-17.2 23L-16.3 23.1L-15.1 22.4L-13.6 20.8L-11.5 18.2L-10.8 17.3L-9.3 18.1L-7.8 18.8L-6.2 19.4L-4.6 19.8L-4.7 21L-5 24.3L-5.1 26.5L-4.8 27.9L-4.1 28.4L4.1 28.4L4.8 27.9L5.1 26.5L5 24.3L4.7 21L4.6 19.8L6.2 19.4L7.8 18.8L9.3 18.1L10.8 17.3L11.5 18.2L13.6 20.8L15.1 22.4L16.3 23.1L17.2 23L23 17.2L23.1 16.3L22.4 15.1L20.8 13.6L18.2 11.5L17.3 10.8L18.1 9.3L18.8 7.8L19.4 6.2L19.8 4.6L21 4.7L24.3 5L26.5 5.1L27.9 4.8L28.4 4.1L28.4 -4.1L27.9 -4.8L26.5 -5.1L24.3 -5L21 -4.7L19.8 -4.6L19.4 -6.2L18.8 -7.8L18.1 -9.3L17.3 -10.8L18.2 -11.5L20.8 -13.6L22.4 -15.1L23.1 -16.3L23 -17.2L17.2 -23L16.3 -23.1L15.1 -22.4L13.6 -20.8L11.5 -18.2L10.8 -17.3L9.3 -18.1L7.8 -18.8L6.2 -19.4L4.6 -19.8L4.7 -21L5 -24.3L5.1 -26.5L4.8 -27.9L4.1 -28.4L-4.1 -28.4L-4.8 -27.9L-5.1 -26.5L-5 -24.3L-4.7 -21Z";
const TRIANGLE = "M0 -9.4L8.1 4.7L-8.1 4.7Z";

export function AcogniMark({ size = 18, title = "Acogni Driver" }: { size?: number; title?: string }) {
  return (
    <svg
      className="acogni-brand"
      width={size}
      height={size}
      viewBox="0 0 64 64"
      role="img"
      aria-label={title}
      data-testid="acogni-mark"
      fill="none"
      strokeLinejoin="round"
      strokeWidth={4.2}
    >
      <title>{title}</title>
      <g transform="translate(32 32)">
        <path d={GEAR} stroke="currentColor" />
        <path d={TRIANGLE} stroke="var(--primary)" />
      </g>
    </svg>
  );
}
