# Changelog

## 0.1.1

### Added

- Background services window. It lists the servers and watchers agents leave
  running, grouped by session, with their ports, CPU and memory, and stops
  them one at a time, per session or all at once. Open it from the status
  bar, the Workspace menu, the Dock menu or with ⌘⇧B.
- The Dock menu opens the background services and the settings.

### Fixed

- Closing a session or quitting Koi also stops the services its agent
  detached from the terminal, such as those started with `nohup`.
- ⌘W closes the settings and background services windows.
- The Window menu and the Dock menu list Koi's windows when Koi runs in
  Chinese.
- Window titles cut long session names short, so the Dock and Window menus
  stay narrow.

## 0.1.0

First release.
