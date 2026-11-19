# Horizon Launcher for Windows 7 x86

This 32-bit edition requires **Windows 7 SP1 or newer** and a prepared Horizon installation. Windows XP is not supported. No Java installation is needed.

1. Unzip into a writable folder and start `horizon-launcher-win7.exe`.
2. Select `%APPDATA%\horizon-launcher` (or your relocated launcher data folder). The launcher finds complete compatible resources in its `win7-prepare` caches and skips unfinished entries. You can also select `win7-prepare`, an installation containing `.local\horizon-client\26.3`, a `resources` folder, or the folder containing `horizon-resources.json` itself. Selecting the launcher EXE's folder also searches the configured data cache. A client EXE alone is not a prepared installation.
3. Enter a nickname and press **Play**. The launcher checks the prepared version and required assets, then uses `horizon-client.exe` in the selected installation or beside the launcher. Use **Choose...** to select another client and keep its DLLs alongside it. The client must be **32-bit (x86)**; x64 and ARM64 executables are rejected.

This edition does not download a client or prepare Minecraft files. To make a new installation, use the x64 launcher or Horizon's resource preparer on a compatible system, then transfer your own prepared resources and a compatible x86 client. No Minecraft assets are included in this archive.

When updating Horizon, also update the launcher and prepared resources. An older resource format is rejected. Selecting the launcher data folder finds the new compatible cache automatically after it has been prepared with the x64 edition.

The launcher starts Horizon with an offline nickname. Saves and game settings use the same game folder as the x64 edition; x86 launcher choices are saved separately in `%APPDATA%\horizon-launcher\win7-launcher-x86.json`. **Game folder** opens saves and settings; **Open log** shows setup or client errors.

The log names the resource folder used for each launch. Keep that folder: resources are used in place. Other unused or unfinished preparation caches can be deleted once the working installation has been identified. Each hash folder belongs to a preparer version; it is not a folder of saved worlds.

Launcher compatibility does not add x86, XP or DX9 support to the game itself. Use a client built for your operating system and hardware.
