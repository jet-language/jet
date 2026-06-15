# HalcyonOmega NixOS parity audit

**Source inspected:** `/home/nate/nixos`, read-only, 2026-06-15.
**Purpose:** JetOS must be able to represent every capability used by this
configuration. The target is not a line-for-line port; it is the same structure
and power with better Jet syntax and ergonomics.

## Root Shape

`flake.nix` is the model for the Jet pack file. The role is ratified; the
Ratified filenames: `pack.jet` and `pack.lock`.

Current root responsibilities:

- Pin inputs: stable nixpkgs, unstable nixpkgs, plasma beta/master nixpkgs,
  flake-parts, import-tree, disko, Home Manager, plasma-manager, NUR, Stylix,
  nixcord, nix-gaming, CachyOS kernel, nix-flatpak, Ghostty, Vicinae,
  nix-vscode-extensions, browser flakes, Spicetify, Nexos, and a local Jet path.
- Express follows relationships between inputs.
- Define shared values: username, system, GitHub username/email, unstable pkgs.
- Auto-import `./modules` through import-tree.
- Pass common args to every module.

JetOS requirements:

- The Jet pack file must be the root repo config equivalent to `flake.nix`.
- Inputs need pins, follows relationships, local paths, and named package sets.
- Shared values must be declared once and available to modules without Nix-style
  argument boilerplate.
- Import-tree should be default for normal module directories.

## Directory Shape

Current high-level directories:

- `modules/hosts/desktop`: `halcyon`, `halcyon-plasma-beta`, hardware config.
- `modules/hosts/iso`: graphical installer ISO.
- `modules/core`: boot, system, user, audio, network, security,
  virtualization, localization, zram, Wayland/Xorg, XDG MIME defaults.
- `modules/core/hardware`: keyboard, mouse, graphics, bluetooth, smartcard.
- `modules/core/style`: fonts, Stylix, GTK, Qt.
- `modules/apps`: Flatpak, AppImage, terminal, dev, gaming, general, utilities.
- `modules/desktop-environments/kde`: Plasma and plasma-manager.
- `modules/nix`: nixpkgs, settings, substituters, Home Manager, nh helper.
- `overlays`: Plasma beta overlay and patch files.
- `assets`: wallpapers, themes, logos, config JSON, scripts, patches.

JetOS requirements:

- Host, core, apps, desktop, package-manager, overlays, and assets all need
  first-class placement.
- Feature files must be liftable: a single app module can include packages,
  user config, services, files, and activation hooks.
- Parking drafts with `_` must work.

## Host and ISO Model

Current hosts:

- `halcyon`: desktop host using shared apps/core/desktop/nix modules.
- `halcyon-plasma-beta`: host variant using a different nixpkgs input, an
  overlay, display-manager override, system tags, and stateVersion override.
- `iso`: graphical Calamares Plasma installer image with curated packages and
  installer/channel modules.

JetOS requirements:

- Multiple host outputs from one root.
- Host variants with alternate package inputs and overlays.
- Hardware configuration import/generation story.
- ISO output from the same config repo.
- QEMU/VM test target for ISO boot and install sanity.

## Option and Merge Semantics

Patterns used:

- Custom option declaration: `nixos.performance.kernel` enum in boot config.
- Defaults and forced values: `mkDefault`, `mkForce`.
- Final value reads: config-driven kernel package selection and Stylix values.
- List/map merging: packages, modules, settings, services, MIME maps.
- Conflict avoidance: force flags for generated config files.

JetOS requirements:

- Declared options with type, default, docs, and enum support.
- Priorities: default, normal, force.
- Final-value reads with cycle diagnostics.
- Deterministic list/map merge independent of file discovery order.
- Conflict errors with file/line and clear fixes.

## Package Sources

Patterns used:

- Packages from stable nixpkgs, unstable nixpkgs, flake inputs, overlays,
  custom derivations, Flatpak, AppImage, and local path inputs.
- Custom derivation with `fetchFromGitHub`, pinned hash, Python environment,
  patches, wrapper script, env vars, and install phase.
- Overlay with source replacement, package overrides, and patch application.

JetOS requirements:

- Phase 1 can orchestrate Nix for these.
- JetOS must model multiple package providers and custom package recipes.
- Patch files and repo assets must be addressable ergonomically.
- Wrapper scripts and environment variables must be first-class recipe outputs.
- Flatpak/AppImage support cannot be treated as out of scope.

## System and User Scopes

Patterns used:

- System packages/services: bootloader, kernel, audio, networking, virtualization,
  SSH, Tailscale, Docker, Flatpak, AppImage, KDE, fonts.
- User packages/config through Home Manager.
- `home-manager.sharedModules` for plasma-manager, nixcord, nix-flatpak,
  Vicinae, and external home modules.
- Per-user config files: Git, Fastfetch, MIME apps, VSCode, Ghostty, Helix,
  Btop, Yazi, Starship, Fish, SSH.
- Session variables and desktop MIME defaults.

JetOS requirements:

- One feature module must be allowed to touch system scope and user scope.
- Home-manager-style user config must not require a second language.
- External user-module ecosystems need an integration path.
- Raw file emission, source links, generated text, and force/backup semantics
  must be supported.

## Desktop Ergonomics

KDE/Plasma patterns used:

- Plasma desktop enablement and display-manager selection.
- plasma-manager for panels, widgets, shortcuts, window rules, desktop behavior,
  power/session behavior, wallpaper, and application integration.
- Stylix theme, base16 scheme, wallpapers, fonts, GTK/Qt integration.
- KDE Connect, ydotool, login manager config, excluded packages.

JetOS requirements:

- Desktop environment modules need high-level ergonomic APIs, not raw nested
  key/value blobs only.
- KDE/Plasma should be a reference desktop target for the prototype because the
  current setup depends on it heavily.
- The ISO milestone should target a graphical Plasma installer image unless the
  owner changes that direction.

## Imperative Edges

Patterns used:

- Flatpak activation script reconciles desired apps against installed apps.
- Shell scripts and generated wrappers.
- Service/user activation behavior.

JetOS requirements:

- Declarative desired state remains authoritative.
- Activation hooks are supported but clearly marked and ordered.
- Hooks must be testable and explainable in diffs.

## Compatibility Rule

If `/home/nate/nixos` can express it today, JetOS must either:

1. express it directly with better syntax,
2. provide a compatibility bridge through Nix/Jetpack during migration, or
3. record an explicit owner-approved exception.

Absent an exception, lack of support is a JetOS design bug.
