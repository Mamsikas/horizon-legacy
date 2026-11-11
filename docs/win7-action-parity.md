# Win7 native action parity

Source: `horizon-launcher-win7 --list-native-actions`; 108 forms generated from the existing CLI.

Wine PASS records only the stated additional check outside real Windows. Fields and flags
are validated by the same clap parser, and the shared core performs the operation.

| Action | Additional evidence | Win7 x64 GMA 3150 / N455 | Win7 x64 HD 4570 |
|---|---|---|---|
| `legacy wardrobe-list` | Shared core API; ownership/confirmation unit checks; live service not tested | NOT TESTED | NOT TESTED |
| `legacy wardrobe-equip` | Shared core API; ownership/confirmation unit checks; live service not tested | NOT TESTED | NOT TESTED |
| `legacy wardrobe-unequip` | Shared core API; ownership/confirmation unit checks; live service not tested | NOT TESTED | NOT TESTED |
| `legacy wardrobe-visible` | Shared core API; ownership/confirmation unit checks; live service not tested | NOT TESTED | NOT TESTED |
| `legacy wardrobe-preferences` | Shared core API; ownership/confirmation unit checks; live service not tested | NOT TESTED | NOT TESTED |
| `legacy wardrobe-claim` | Shared core API; ownership/confirmation unit checks; live service not tested | NOT TESTED | NOT TESTED |
| `legacy shop-catalogue` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `legacy shop-history` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `legacy shop-quote` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `legacy shop-checkout` | Explicit Confirm and existing HTTPS checkout; no live transaction performed | NOT TESTED | NOT TESTED |
| `legacy shop-claim` | Explicit Confirm and existing HTTPS checkout; no live transaction performed | NOT TESTED | NOT TESTED |
| `legacy storage-status` | Wine PASS: reading active roots | NOT TESTED | NOT TESTED |
| `legacy storage-move` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `legacy storage-retry` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `legacy storage-default` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `versions` | Wine PASS: --installed and worker completion | NOT TESTED | NOT TESTED |
| `install` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `remove` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `import-build` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `prepare` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `play` | Parser argument contract PASS; full scenario not tested | NOT TESTED | NOT TESTED |
| `server` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `native-cache` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `native-build` | Win7 native-build/host toolchain unsupported; explicit refusal | NOT TESTED | NOT TESTED |
| `notes` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `links` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `account list` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `account refresh` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `account sign-out` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `account select` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `account offline` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `account forget-offline` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `account skin` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `settings` | Parser argument contract PASS; full scenario not tested | NOT TESTED | NOT TESTED |
| `game-args` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `share-log` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `unshare-log` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `logs` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `update` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `desktop-entry` | Linux-only operation; not applicable to Windows | NOT TESTED | NOT TESTED |
| `data status` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `data allow` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `data deny` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `data show` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `data delete` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `data send` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `data bug` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `data benchmark` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance list` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance create` | Wine PASS: Unicode name, JSON persistence, worker completion; parser test | NOT TESTED | NOT TESTED |
| `instance versions` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance java` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance loader` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance rename` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance delete` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance icon` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance group` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance select` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance version` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance export` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance launch` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance shortcut` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance import` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance others` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `instance copy-from` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `worlds list` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `worlds backup` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `worlds copy` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `worlds delete` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `worlds datapack-list` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `worlds datapack-add` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `worlds datapack-remove` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `packs list` | Parser argument contract PASS; full scenario not tested | NOT TESTED | NOT TESTED |
| `packs add` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `packs on` | Parser argument contract PASS; full scenario not tested | NOT TESTED | NOT TESTED |
| `packs off` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `packs remove` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `java list` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `java install` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `screenshots list` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `screenshots delete` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `multiplayer list` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `multiplayer add` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `multiplayer remove` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `modpack list` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `modpack import` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `modpack port` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `modpack play` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `modpack remove` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `mods list` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `mods search` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `mods check` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `mods install` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `mods update` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `mods add-github` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `mods update-github` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `mods remove` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `mods enable` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `mods disable` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `rust status` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `rust install` | Win7 native-build/host toolchain unsupported; explicit refusal | NOT TESTED | NOT TESTED |
| `rust remove` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `servers list` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `servers create` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `servers set-up` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `servers run` | Parser argument contract PASS; full scenario not tested | NOT TESTED | NOT TESTED |
| `servers forget` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| `no-ai` | Not exercised separately; see core tests and the hardware checklist | NOT TESTED | NOT TESTED |
| Microsoft Device Code panel | Wine PASS: safe refusal without client ID; real OAuth not tested | NOT TESTED | NOT TESTED |
| `--smoke-client` | Wine PASS: Unicode paths, resources/home/env; mock only, not the game | NOT TESTED | NOT TESTED |
| `--check-prepare-runtime` | Verified local vendor archive and JVM probe under Wine; network/exporters not tested | NOT TESTED | NOT TESTED |

GPUI visual previews/gallery and drag-and-drop are not ported. Native controls provide
text forms; the user opens a returned checkout URL in an external browser. Each row needs
its own PASS/FAIL and log on both physical systems before hardware parity can be accepted.
