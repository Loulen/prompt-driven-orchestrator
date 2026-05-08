// icons.jsx — simple line icons used across Maestro chrome
// All are 14x14 unless noted. stroke-width 1.6, currentColor.

const Ic = {};

const make = (path, w = 14, h = 14, attrs = {}) => (props) => (
  <svg width={w} height={h} viewBox={`0 0 ${w} ${h}`} fill="none"
       stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"
       {...attrs} {...props}>
    {path}
  </svg>
);

Ic.Pencil = make(<>
  <path d="M9.2 2.4l2.4 2.4M2 12l1-3.4 7.5-7.5a1 1 0 0 1 1.4 0l1 1a1 1 0 0 1 0 1.4L5.4 11 2 12z" />
</>);

Ic.Gear = make(<>
  <circle cx="7" cy="7" r="2" />
  <path d="M7 1.5v1.4M7 11.1v1.4M2.6 7H1.2M12.8 7h-1.4M3.6 3.6l1 1M9.4 9.4l1 1M3.6 10.4l1-1M9.4 4.6l1-1" />
</>);

Ic.Sun = make(<>
  <circle cx="7" cy="7" r="2.6" />
  <path d="M7 1.5v1.2M7 11.3v1.2M2.6 7H1.4M12.6 7h-1.2M3.6 3.6l.8.8M9.6 9.6l.8.8M3.6 10.4l.8-.8M9.6 4.4l.8-.8" />
</>);

Ic.Moon = make(<>
  <path d="M11.4 8.6A4.6 4.6 0 0 1 5.4 2.6a4.8 4.8 0 1 0 6 6z" />
</>);

Ic.Plus = make(<>
  <path d="M7 3v8M3 7h8" />
</>);

Ic.PlusSm = make(<>
  <path d="M6 2v8M2 6h8" />
</>, 12, 12);

Ic.Chevron = make(<>
  <path d="M3 5l4 4 4-4" />
</>);

Ic.ChevronR = make(<>
  <path d="M5 3l4 4-4 4" />
</>);

Ic.Search = make(<>
  <circle cx="6" cy="6" r="3.6" />
  <path d="M9 9l3 3" />
</>);

Ic.Copy = make(<>
  <rect x="4" y="4" width="8" height="8" rx="1.5" />
  <path d="M2 9V3a1 1 0 0 1 1-1h6" />
</>);

Ic.Kebab = make(<>
  <circle cx="3" cy="7" r=".6" fill="currentColor" />
  <circle cx="7" cy="7" r=".6" fill="currentColor" />
  <circle cx="11" cy="7" r=".6" fill="currentColor" />
</>);

Ic.X = make(<>
  <path d="M3 3l8 8M11 3l-8 8" />
</>);

Ic.External = make(<>
  <path d="M9 2h3v3M11.5 2.5L6 8" />
  <path d="M11 8v3a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h3" />
</>);

Ic.Terminal = make(<>
  <rect x="1.5" y="2.5" width="11" height="9" rx="1" />
  <path d="M4 5.5l1.5 1.5L4 8.5M7 9h2.5" />
</>);

Ic.Manager = make(<>
  <circle cx="7" cy="5" r="2" />
  <path d="M2.5 12c.6-2.4 2.4-3.6 4.5-3.6S11 9.6 11.5 12" />
  <circle cx="11" cy="3.5" r="1.4" fill="currentColor" stroke="none" />
</>);

Ic.Doc = make(<>
  <path d="M3 1.5h5l3 3V12a.5.5 0 0 1-.5.5h-7A.5.5 0 0 1 3 12V2a.5.5 0 0 1 .5-.5z" />
  <path d="M8 1.5v3h3M5 7h4M5 9h4M5 11h2.5" />
</>);

Ic.Code = make(<>
  <path d="M5 4l-3 3 3 3M9 4l3 3-3 3M8 2.5l-2 9" />
</>);

Ic.Halt = make(<>
  <path d="M4 1.5h6l2.5 2.5v6L10 12.5H4L1.5 10V4L4 1.5z" />
  <path d="M5 5l4 4M9 5l-4 4" />
</>);

Ic.Pulse = make(<>
  <path d="M1.5 7h2l1.5-4 2 8 1.5-4h4" />
</>);

Ic.Check = make(<>
  <path d="M2.5 7l3 3 6-6" />
</>);

Ic.Warn = make(<>
  <path d="M7 1.8l5.5 9.4a.6.6 0 0 1-.5.9H2a.6.6 0 0 1-.5-.9L7 1.8z" />
  <path d="M7 5.5v3M7 10.2v.1" />
</>);

Ic.Spark = make(<>
  <path d="M7 1.5L8.4 5.6 12.5 7 8.4 8.4 7 12.5 5.6 8.4 1.5 7l4.1-1.4L7 1.5z" />
</>);

Ic.Maximize = make(<>
  <path d="M2.5 5V2.5h2.5M9 2.5h2.5V5M11.5 9v2.5H9M5 11.5H2.5V9" />
</>);

Ic.Minus = make(<>
  <path d="M3 7h8" />
</>);

Ic.Grid = make(<>
  <rect x="2" y="2" width="4" height="4" rx=".5" />
  <rect x="8" y="2" width="4" height="4" rx=".5" />
  <rect x="2" y="8" width="4" height="4" rx=".5" />
  <rect x="8" y="8" width="4" height="4" rx=".5" />
</>);

Ic.Filter = make(<>
  <path d="M2 3h10l-3.5 4.5V11l-3-1V7.5L2 3z" />
</>);

Ic.Refresh = make(<>
  <path d="M2.5 7a4.5 4.5 0 0 1 8-2.8M11.5 7a4.5 4.5 0 0 1-8 2.8" />
  <path d="M9 2.5v2.5h2.5M5 11.5V9H2.5" />
</>);

Ic.Branch = make(<>
  <circle cx="3.5" cy="3" r="1.2" />
  <circle cx="3.5" cy="11" r="1.2" />
  <circle cx="10.5" cy="4" r="1.2" />
  <path d="M3.5 4.2v5.6M3.5 7c0-1.7 1.3-3 3-3h2" />
</>);

Ic.Logo = (props) => (
  <svg width="18" height="18" viewBox="0 0 18 18" fill="none" {...props}>
    <path d="M3 14V4l3 4 3-4 3 4 3-4v10" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/>
  </svg>
);

Ic.Library = make(<>
  <path d="M2.5 2.5h2.5v9h-2.5z"/>
  <path d="M5 2.5h2.5v9h-2.5z"/>
  <path d="M9.4 3.4l2.3.7-2.3 7.4-2.3-.7z"/>
</>);

Ic.Loop = make(<>
  <path d="M11 5.5a4.5 4.5 0 1 0-1.3 3.2"/>
  <path d="M11 2.5v3h-3"/>
</>);

Ic.Switch = make(<>
  <path d="M2 6.5h3l2-3h4M9 3.5l2-1M9 3.5l1.5 2M2 6.5l3 3h4M9 9.5l2 1M9 9.5l1.5-2"/>
</>);

Ic.Star = make(<>
  <path d="M7 1.6l1.7 3.6 3.9.5-2.9 2.7.8 3.9L7 10.4 3.5 12.3l.8-3.9L1.4 5.7l3.9-.5z"/>
</>);

Ic.StarFill = (props) => (
  <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor"
       stroke="currentColor" strokeWidth="0.5" strokeLinejoin="round" {...props}>
    <path d="M7 1.6l1.7 3.6 3.9.5-2.9 2.7.8 3.9L7 10.4 3.5 12.3l.8-3.9L1.4 5.7l3.9-.5z"/>
  </svg>
);

Ic.Trash = make(<>
  <path d="M2.5 3.5h9M5 3.5V2.5h4v1M3.5 3.5l.5 8a1 1 0 0 0 1 .9h4a1 1 0 0 0 1-.9l.5-8M6 6v4M8 6v4"/>
</>);

Ic.Info = make(<>
  <circle cx="7" cy="7" r="5.5"/>
  <path d="M7 6.5v3.5M7 4.5v.1"/>
</>);

Ic.Floppy = make(<>
  <path d="M2.5 2.5h7.5l2 2v7a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1v-8a1 1 0 0 1 1-1z"/>
  <path d="M4.5 2.5v3h5v-3"/>
  <path d="M4.5 12.5V8h5v4.5"/>
</>);

window.Ic = Ic;
