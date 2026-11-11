# Horizon Launcher for Windows 7 x64

Unzip the archive into a writable folder and start `horizon-launcher-win7.exe`.

1. Select your existing Minecraft Java Edition **26.3** game folder (normally `%APPDATA%\.minecraft`). Install the 26.3 files with your Minecraft launcher first; running the original game on Windows 7 is not required. Horizon checks the original files against its pinned hashes.
2. Enter a nickname and press **Play**. Initial preparation downloads a verified Java runtime and official preparation files, then builds Horizon's resources locally. Later launches reuse the result.
3. By default the launcher uses `horizon-client.exe` beside it, an already installed release, or downloads a signed official Horizon release. Use **Choose…** to select your own client build instead. Keep that client's accompanying DLLs and files together.

This launcher uses an offline nickname. It does not sign in to Microsoft; authenticated servers require a signed-in client. The modern Horizon launcher retains its account features.

The current Horizon client still needs its original graphics APIs. Compatibility of the launcher does not add DX9 support to the game. A future compatible client can use the same launch button.

**Game folder** opens saves and settings. **Details** shows setup progress; **Open log** opens the latest setup or game log. If setup fails, correct the reported problem and press Play again. Cancel stops preparation safely; completed verified downloads are reused.

Launcher data and logs are in `%APPDATA%\horizon-launcher` by default. Preparation tools may use a separate ASCII path under `%ProgramData%` when the profile path contains spaces or non-ASCII characters. No administrator installation is required.
