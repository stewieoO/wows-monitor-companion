# WoWs Monitor Companion

<img src="assets/wows-monitor-icon.png" alt="WoWs Monitor Companion icon" width="64">

WoWs Monitor Companion is a small tray app that helps WoWs Monitor read battle replay data when Chrome cannot access the game's system folders through its file API.

The companion watches the replay folder used by your World of Warships client and copies `tempArenaInfo.json` to a folder you choose. WoWs Monitor can then use the copied file without needing browser access to the game's protected folders.

## Getting started

1. Start WoWs Monitor Companion.
2. Choose your World of Warships installation folder.
3. Choose a destination folder for the copied `tempArenaInfo.json` file, such as a folder you can select from WoWs Monitor.
4. Select **Save and start monitoring**.
5. In WoWs Monitor, drag and drop in the destination folder you selected.

The companion finds the replay folder from your game configuration. If replay settings change, it updates the folder it watches. An existing `tempArenaInfo.json` file is copied when monitoring starts, and new or changed files are copied as they appear.

Closing the app window leaves it running in the system tray. Use the tray icon to reopen the window or quit the app. Enable **Start this app when I log in** in the app if you want it to start minimized each time you sign in.

## More information

See [How to build](docs/how-to-build.md) for developer build instructions.
