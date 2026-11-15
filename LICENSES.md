# Third-party code and assets

## Not licensed for redistribution

The files below are third-party proprietary assets that were used during development. Cereal holds no licence to distribute them. Replace them, or obtain a licence, before distributing a build.

- Fonts in the desktop app: `apps/desktop/assets/fonts/kmrmelangegrotesk-book.ttf`, `kmrmelangegrotesk-regular.ttf`, `kmrmelangegrotesk-medium.ttf`, `kmrmelangegrotesk-semibold.ttf` and `QuadrantNotepad.ttf`.
- Fonts in the web app: `apps/web/src/fonts/melange.ttf` and `quadrant.ttf`.

## Icons

The icons in `apps/desktop/assets/icons` are original drawings made for Cereal, with these exceptions:

- `chat.svg` uses the `message-circle` path from [Feather](https://github.com/feathericons/feather), copyright (c) 2013-2023 Cole Bemis, MIT licence. `arrow-left.svg`, `arrow-up.svg`, `chevron-down.svg`, `close.svg`, `plus.svg` and `more.svg` have the same geometry as Feather's icons of those shapes.
- `plus-small.svg` has the same geometry as the plus icon in [Heroicons](https://github.com/tailwindlabs/heroicons), copyright (c) Tailwind Labs, Inc., MIT licence.
- `google.svg` is Google's "G" mark, used to identify Google sign-in and calendar connections. It belongs to Google.

## Fonts with an open licence

`apps/desktop/assets/fonts/Inter.ttf` is Inter, distributed under the SIL Open Font License 1.1. Include the licence text when redistributing it.

## Original artwork

The Cereal logo is original artwork drawn by `scripts/logo/cereal_logo.py`: the app icons in `apps/desktop/assets/app-icons` and `apps/ios/Cereal/Resources/Assets.xcassets`, the single-colour mark in `apps/desktop/assets/icons/cereal.svg`, `apps/web/public/mark.svg` and `apps/web/src/assets/mark.svg`, and the web app icon and touch icon in `apps/web/src/assets`.

## Open-source code

- **OpenOats.** The desktop audio capture adapter in `apps/desktop/native` is adapted from [OpenOats](https://github.com/yazinsai/OpenOats), copyright (c) 2026 OpenOats contributors, MIT licence. The notice is kept in `apps/desktop/native/OpenOats-LICENSE`, in the derived source files and in the packaged app's Resources. Baseline commit `b4353cd71ca9677f330e74751ffb669172a0b575`; only the Core Audio and microphone capture approach is used.
- **GPUI.** GPUI 0.2.2 is copyright Zed Industries and contributors, Apache-2.0. The copy in `vendor/gpui` keeps its upstream licence files; `vendor/gpui/LOCAL_PATCH.md` describes the local change. The text input in `apps/desktop/src/input.rs` is adapted from GPUI's Apache-2.0 example and keeps its attribution.
- **shadcn/ui.** The Button, Input, Badge, Card and Select components in `apps/web/src/components/ui` are adapted from [shadcn/ui](https://github.com/shadcn-ui/ui), copyright (c) 2023 shadcn, MIT licence (`apps/web/licenses/shadcn-ui-MIT.txt`).
- **Lobe Icons.** The provider marks in `apps/web/public/providers` are from [Lobe Icons](https://github.com/lobehub/lobe-icons), copyright (c) 2023–2026 LobeHub, MIT licence (`apps/web/licenses/lobe-icons-MIT.txt`). Provider names and marks belong to their owners.

Rust dependencies are recorded in `Cargo.lock` and JavaScript dependencies in `bun.lock`; each keeps its own licence. This file does not relicense any third-party asset or dependency.
