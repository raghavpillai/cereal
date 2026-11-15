# Local macOS focus fix

This is GPUI 0.2.2 from the Cargo registry with two small local changes:

- In `src/platform/mac/window.rs`, `window_did_change_key_status` releases the window-state mutex before calling AppKit `resignKeyWindow`. AppKit can synchronously invoke `windowDidResignKey`, which acquires the same mutex. Holding it across that call deadlocked when activating the main window around a nonactivating recording panel.
- In `src/window.rs`, `DispatchEventResult` is public, matching the existing public `Window::dispatch_event` API. This lets the native validation workflow dispatch button-free mouse movement and mouse-exit events through the real window event listeners, rather than bypassing them by calling a hover handler directly. Event dispatch behavior is unchanged.

The original source and its licenses are retained. `Cargo.toml` selects this copy through `[patch.crates-io]`, leaving the machine’s shared Cargo registry unchanged. The expanded native smoke workflow exercises background/foreground panel transitions.
