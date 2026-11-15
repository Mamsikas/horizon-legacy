# Horizon Launcher for Windows 7 x86

This 32-bit edition requires **Windows 7 SP1 or newer** and a prepared Horizon installation. Windows XP is not supported. No Java installation is needed.

1. Unzip into a writable folder and start `horizon-launcher-win7.exe`.
2. Select your existing Horizon installation folder. It must contain `.local\horizon-client\26.3\horizon-resources.json`, or a `resources` folder with that file. You can also select the prepared resources folder itself. A client EXE alone is not a prepared installation.
3. Enter a nickname and press **Play**. The launcher checks the prepared version and required assets, then uses `horizon-client.exe` in the selected installation or beside the launcher. Use **Choose...** to select another client and keep its DLLs alongside it. The client must be **32-bit (x86)**; x64 and ARM64 executables are rejected.

This edition does not download a client or prepare Minecraft files. To make a new installation, use the x64 launcher or Horizon's resource preparer on a compatible system, then transfer your own prepared resources and a compatible x86 client. No Minecraft assets are included in this archive.

The launcher starts Horizon with an offline nickname. Saves and game settings use the same game folder as the x64 edition; x86 launcher choices are saved separately in `%APPDATA%\horizon-launcher\win7-launcher-x86.json`. **Game folder** opens saves and settings; **Open log** shows setup or client errors.

Launcher compatibility does not add x86, XP or DX9 support to the game itself. Use a client built for your operating system and hardware.
