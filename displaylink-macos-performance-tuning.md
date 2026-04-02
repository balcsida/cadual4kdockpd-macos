# DisplayLink macOS Hidden Performance Settings

Undocumented `defaults write` preferences for DisplayLink Manager on macOS, reverse-engineered from the `DisplayLinkUserAgent` binary (v15.1.0).

Tested on: **Apple M3 Max, macOS Tahoe 26.4, DisplayLink Manager 15.1.0, DisplayLink USB-C Dual-4K Dock, 2x HP E273 (1080p @ 60Hz)**

## The Key Setting: `ForceScreenRefresh`

This is the single most impactful setting for perceived smoothness on DisplayLink displays:

```bash
defaults write com.displaylink.DisplayLinkUserAgent ForceScreenRefresh -bool true
```

Forces DisplayLink to capture fresh frames more aggressively rather than skipping frames it considers unchanged. Results in noticeably smoother window dragging and scrolling on DisplayLink-connected monitors.

**To revert:**

```bash
defaults delete com.displaylink.DisplayLinkUserAgent ForceScreenRefresh
```

After changing, restart DisplayLink:

```bash
killall DisplayLinkUserAgent && sleep 2 && open "/Applications/DisplayLink Manager.app"
```

## Other Hidden Settings

Found in the binary but with less measurable impact:

| Setting | Type | Description |
|---|---|---|
| `ForceBackendAPI` | String | Accepts `"ScreenCaptureKit"`. On v15.1.0, the app already uses ScreenCaptureKit by default (confirmed via stack trace sampling), so this may not change behavior. |
| `DisableGpuDiffing` | Bool | Disables Metal-based GPU frame diffing. **Don't set this** — GPU diffing is beneficial. |
| `DisableFluxGammaSupport` | Bool | Skips f.lux color processing integration. |
| `DisableNightShiftSupport` | Bool | Skips Night Shift color temperature processing. |
| `DisableRestartOnCrash` | Bool | Prevents auto-restart on crash. |
| `EnableExperimentalModes` | Bool | Enables experimental display modes. [Officially documented for HiDPI](https://support.displaylink.com/knowledgebase/articles/1993915-displaylink-manager-app-experimental-hidpi-modes). |
| `EnableLargeHiDPIModes` | Bool | Enables scaled 3008x, 2560x and more modes. |
| `FallbackBackendAPI` | String | Fallback capture API if the primary fails. |

## How These Were Found

These keys were extracted from the binary at `/Applications/DisplayLink Manager.app/Contents/MacOS/DisplayLinkUserAgent` using `strings` and correlated with `NSUserDefaults` access patterns (`boolForKey:`, `stringForKey:`, etc.) found in the same binary.

The `ForceBackendAPI` / `ScreenCaptureKit` setting was verified using `sample` (call stack sampling) — both the default and forced modes showed `SCStream` / `SCShareableContent` calls from Apple's ScreenCaptureKit framework, with zero `CGDisplayStream` calls, confirming v15.1.0 already uses ScreenCaptureKit natively.

## Notes

- These are **undocumented internal preferences** — they may change or disappear with any DisplayLink update.
- DisplayLink on macOS works via screen capture (not native GPU output), which is a fundamental architectural limitation imposed by Apple. These tweaks help but can't make it match a native display connection.
- Tested April 2026. YMMV on different versions.
