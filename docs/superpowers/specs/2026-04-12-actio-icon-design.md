# Actio Icon Design Spec

## Summary

An animated icon for the Actio desktop app, displayed in the system tray (replacing the current green pulsing dot) and the board header (replacing the current checkmark brand-mark). The icon is the letter "A" in **Grand Hotel** (OFL-licensed cursive font from Google Fonts), with a **CSS Houdini conic-gradient** flowing through the letter shape to represent three app states.

## Font

- **Grand Hotel** — Google Fonts, SIL Open Font License
- Bundle the `.woff2` file in the Tauri app for offline use
- No font-weight variations needed (single weight)

## Animation Technique

Use `background-clip: text` to make a conic gradient visible only through the letter shape. CSS Houdini `@property` enables smooth animation of the gradient angle (`--gradient-angle`), creating the illusion of light/color flowing through the strokes.

### Core CSS

```css
@property --gradient-angle {
  syntax: "<angle>";
  initial-value: 0deg;
  inherits: false;
}

.actio-icon {
  font-family: 'Grand Hotel', cursive;
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  --gradient-angle: 0deg;
}

@keyframes flow {
  to { --gradient-angle: 360deg; }
}
```

### Fallback (non-Houdini browsers)

If `@property` is not supported, fall back to a static gradient. Tauri uses WebView2 (Chromium) on Windows which supports Houdini, but for safety:

```css
@supports not (syntax: "<angle>") {
  .actio-icon {
    background-image: linear-gradient(135deg, #8b5cf6, #c4b5fd, #8b5cf6);
  }
}
```

## Three States

### Recording

Active voice capture. The icon shows warm violet light flowing through the letter.

```css
.recording .actio-icon {
  background-image: conic-gradient(
    from var(--gradient-angle) at 50% 55%,
    #8b5cf6 0deg,
    #8b5cf6 60deg,
    #9b8af5 90deg,
    #b4a4fc 120deg,
    #c4b5fd 150deg,
    #ddd6fe 180deg,
    #c4b5fd 210deg,
    #b4a4fc 230deg,
    #9b8af5 260deg,
    #8b5cf6 300deg,
    #8b5cf6 360deg
  );
  animation: flow 4s linear infinite;
}
```

- **Palette:** Violet family (#8b5cf6 base → #ddd6fe highlight)
- **Speed:** 4s per rotation
- **Character:** Subtle bright sweep (~90° arc) flowing through the strokes continuously

### Processing

LLM inference or transcript aggregation in progress. Indigo-to-teal color shift, slower flow.

```css
.processing .actio-icon {
  background-image: conic-gradient(
    from var(--gradient-angle) at 50% 55%,
    #4f46e5 0deg,
    #4f46e5 60deg,
    #6366f1 90deg,
    #06b6d4 130deg,
    #22d3ee 170deg,
    #67e8f9 190deg,
    #22d3ee 220deg,
    #06b6d4 250deg,
    #6366f1 280deg,
    #4f46e5 320deg,
    #4f46e5 360deg
  );
  animation: flow 6s linear infinite;
}
```

- **Palette:** Indigo (#4f46e5) → Teal (#67e8f9)
- **Speed:** 6s per rotation (slower, deliberate)
- **Character:** Contemplative, working

### Paused

Idle / quiet queue. Static monochrome, no animation.

```css
.paused .actio-icon {
  background-image: conic-gradient(
    from 135deg at 50% 55%,
    #475569 0deg,
    #475569 90deg,
    #64748b 150deg,
    #94a3b8 180deg,
    #64748b 210deg,
    #475569 270deg,
    #475569 360deg
  );
  /* No animation */
}
```

- **Palette:** Slate monochrome (#475569 → #94a3b8)
- **Speed:** None (static)
- **Character:** Quiet, ready

## Sizing

| Context | Size | Notes |
|---------|------|-------|
| System tray (StandbyTray) | ~18-20px | Replaces the 8px pulsing green dot |
| Board header (Header) | ~36-38px | Replaces the 42x42 checkmark brand-mark |
| Splash / about | 120-160px | Optional, for branding moments |

## Light Background Handling

On light backgrounds, apply a filter to maintain contrast:

```css
.light-bg .actio-icon {
  filter: brightness(0.8) saturate(1.3);
}
```

## Integration Points

### 1. StandbyTray (`frontend/src/components/StandbyTray.tsx`)

Replace the current `<span className="tray-brand-dot">` with the animated icon component. The state class (`recording` / `processing` / `paused`) should be driven by the voice store's current state.

### 2. Board Header (`frontend/src/components/Header.tsx`)

Replace the current `.brand-mark` SVG (checkmark in 42x42 box) with the animated icon at 36-38px. Remove the `box-shadow` glow since the animation provides visual interest.

### 3. CSS (`frontend/src/styles/globals.css`)

Add the `@property` declaration, `.actio-icon` base styles, and the three state classes. Remove the `pulse` animation on `.tray-brand-dot` (no longer needed).

### 4. Font Loading

Bundle `GrandHotel-Regular.woff2` in `frontend/public/fonts/` and declare via `@font-face` rather than loading from Google Fonts CDN:

```css
@font-face {
  font-family: 'Grand Hotel';
  src: url('/fonts/GrandHotel-Regular.woff2') format('woff2');
  font-display: swap;
}
```

### 5. System Tray Icon (native)

For the actual OS-level tray icon (if using native system tray in the future), export static PNG/ICO versions of the "A" in each state color. The CSS animation only applies within the WebView.

## Gradient Design Rationale

- **Conic gradient** (not linear) so color flows *around* the letter, creating the illusion of light moving through the strokes
- **Gradient center at 50% 55%** — centered on the letter's visual mass (slightly below geometric center for the A's crossbar area)
- **Bright arc spans ~90°** with the remaining ~270° as uniform base color — this makes it look like a subtle highlight flowing rather than an obvious spinning disk
- **Houdini `@property`** enables smooth angle interpolation; without it, CSS would snap between keyframes

## Files to Modify

- `frontend/src/components/StandbyTray.tsx` — replace dot with icon
- `frontend/src/components/Header.tsx` — replace brand-mark with icon
- `frontend/src/styles/globals.css` — add icon styles, remove dot pulse
- `frontend/public/fonts/GrandHotel-Regular.woff2` — new file (bundled font)
- `backend/src-tauri/icons/icon.png` / `icon.ico` — static versions for OS-level icon

## Reference Mockup

Live interactive mockup at `.superpowers/brainstorm/1091-1775975468/content/final-design.html`
