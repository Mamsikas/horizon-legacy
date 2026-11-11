# Win7 resource preparation audit

This describes the experimental Windows 7 x64 launcher's preparation path. It is
not a claim that a fresh installation has completed on Windows 7. The reviewed
fork base is `59aa0e53df43fe087849250c7fd91de3b8636f2e`; the specification's
`36fc4fa187a4e1f3664c8891dc91e2c9e5a5b911` is an earlier available revision.

## Why Java 25 remains necessary

| Subsystem | Existing module and operation | Constraint | Legacy path |
|---|---|---|---|
| Version and game inputs | `research-lock.json`, `horizon-prepare/src/artifacts.rs` | The pinned Minecraft version requires Java 25. Jars, manifests and libraries must still match their pins. | Keep the original verified import/download path and ownership gate. |
| Preparation entry | `horizon-prepare/src/lib.rs`, `prepare_impl` | Calls `java::find_runtime(lock.java_major, ...)` before importing; no usable runtime means preparation stops. | Supply the separately downloaded managed runtime to the existing preparer. |
| Horizon exporter bundle | `horizon-prepare/src/exporters.rs`, `available` / `classes` | A matching bundle removes `javac` work, but its classes still execute with Minecraft classes in a JVM. The bundle checks its source key and class SHA-256 hashes. | Download the smaller JRE when the existing bundle is valid; otherwise download the full JDK for the original local compilation path. |
| Dynamic exports | `Run::run`, `PrepareWorker`, the server/client oracle lists | Registry, collision, block behaviour, models, entity and other data are computed by running the user's verified Minecraft jars. Copying ZIP resources does not replace this work. | Keep the original exporters; no game data is shipped or invented. |
| Data generator | `steps` in `horizon-prepare/src/lib.rs` | Runs the server jar with `-DbundlerMainClass=net.minecraft.data.Main --reports`. Precompiled Horizon exporters do not remove this JVM. | Run the same generator with the managed Java 25 runtime. |
| Memory and parallel work | `java.rs`, `worker.rs`, `pool.rs` | Persistent workers otherwise request `-Xmx2G`; the normal pipeline can run several JVMs together. | Native UI workers default to one preparation job and a preparation-only 512 MiB heap cap. Sufficiency is unmeasured. |
| Native code mods | `launcher/src/native_mods.rs`, `rust_toolchain.rs` | The original path rebuilds the entire client using a modern Rust host and standard Windows target. A matching PE machine does not establish Win7 ABI compatibility. | The legacy build reports this unsupported build path explicitly. A Win7 target/toolchain/cache or a delegated build service remains separate unfinished work. |

There is no native replacement for these dynamic Java exports in this change.
Importing already prepared resources is useful, but does not count as an
autonomous first installation on Win7.

## Pinned vendor runtime

The managed runtime in `launcher/src/legacy_java.rs` is **Liberica
25.0.5.0.3+5 for Windows amd64**. Official assets were checked on 7 November
2026, and their metadata was checked again on 10 November. They contain vendor
Java binaries, not Minecraft files.

| Package | Official download | Bytes | Pinned SHA-256 |
|---|---|---:|---|
| JRE | [bellsoft-jre25.0.5.0.3+5-windows-amd64.zip](https://github.com/bell-sw/Liberica/releases/download/25.0.5.0.3%2B5/bellsoft-jre25.0.5.0.3%2B5-windows-amd64.zip) | 78,025,864 | `6e1167a12a91fb5461f2cd304a91a5f4c4545dfb9e87a761a498c81e082414ae` |
| JDK | [bellsoft-jdk25.0.5.0.3+5-windows-amd64.zip](https://github.com/bell-sw/Liberica/releases/download/25.0.5.0.3%2B5/bellsoft-jdk25.0.5.0.3%2B5-windows-amd64.zip) | 249,291,501 | `d7c90823ea2a24778d7499a82379b5e18c7ce3f4cc7a9d9c9af5b133abe148f0` |

The [official release](https://github.com/bell-sw/Liberica/releases/tag/25.0.5.0.3%2B5)
was published on 26 October 2026. Its `release` files identify BellSoft,
`JAVA_VERSION="25.0.5.0.3"`, `OS_ARCH="x86_64"`, and the version date
24 October 2026. The archive root directories are respectively
`jre-25.0.5.0.3` and `jdk-25.0.5.0.3`.

The [release notes](https://docs.bell-sw.com/liberica-jdk/25.0.5.0.3b5/general/release-notes/)
describe the eMudhra root-certificate security fix (CVE-2026-114820). They
explicitly exclude Windows 32-bit and macOS 10.x. They do **not** provide an
explicit statement that this particular Java 25 build runs on Win7 x64.
The [supported configurations table](https://bell-sw.com/pages/supported-configurations/)
lists Windows 7 SP1+ and a JDK 25 column, but extracted text loses the cell
icons; that is insufficient evidence for this combination. This is therefore a
runtime candidate requiring the hardware checks below.

The retained local archives match the two hashes above. A preliminary normal
import-table scan covered 97 PE files in the JRE and 125 in the JDK. It found
no imports from the checked list including `WaitOnAddress`, `WakeByAddress*`,
`GetSystemTimePreciseAsFileTime`, `CreateFile2`, `GetDpiForWindow`,
`SetThreadDescription`, `ProcessPrng`, and `CreateDXGIFactory2`. The packages
contain local UCRT/API-set and Visual C++ runtime DLLs. This scan does not
establish that every DLL, forwarded export, dynamically resolved API or CPU
instruction works on Win7. In particular, a successful import scan is not an
Atom N455 execution test.

Only vendor documentation, release metadata, archive paths and binary metadata
were inspected. JVM implementation sources, including the JDK's `src.zip`,
were not read or copied. The launcher downloads the original vendor packages;
it does not repackage Java or Mojang assets in its own ZIP.

## Installation and security behaviour

`backend::java` selects this helper only for the legacy Windows build. The
modern launcher's discovery and Mojang runtime path remain unchanged.

The helper downloads with the existing verified TLS client and pinned SHA-256.
Legacy HTTPS requests also reject a redirect to HTTP. It does not disable
certificate verification or relax Minecraft ownership or input checks.

Java is installed under the core's existing ASCII-safe `backend::build_dir()` in
`java/win7-x64/` without
administrator privileges. An exclusive installation lease prevents concurrent
replacement. A complete cached archive is rehashed before extraction. A damaged
archive is downloaded again; there is no unchecked partial-file reuse and no
byte-range resume claim.

Extraction occurs in a private staging directory. The expected layout and
exact version are checked, and the staged `java.exe` is executed with
`-version`; compiler mode also probes the staged `javac.exe`. These probes
remove `JAVA_TOOL_OPTIONS`, `_JAVA_OPTIONS`, and `JDK_JAVA_OPTIONS` so they do
not substitute options from an unrelated Java installation. Neither PATH nor
an unrelated `JAVA_HOME` selects the program being probed.

Only after verification and successful probes does the helper replace its
managed directory. A failed probe leaves the old installation intact. Directory
replacement keeps a previous copy until the new copy is published, attempts
rollback on rename failure, and recovers an interrupted replacement on the next
attempt. Ambiguous recovery is reported with the previous directory retained.
Cancellation checks occur during archive hashing/download and before the
replacement boundary. There is no claim of immediate cancellation inside every
Java exporter or ZIP operation.

## Memory on the 2 GB target

The native UI's CLI worker sets the following only when the user has not set
them already:

```text
HORIZON_PREPARE_JOBS=1
HORIZON_PREPARE_MAX_HEAP_MIB=512
```

`JOBS=1` uses the existing sequential lanes. Each lane closes its persistent
exporter worker when finished. The new explicit heap limit applies to the
exporter workers, one-shot exporters, data generator and compiler JVM. It
replaces larger preparer `-Xmx` values, preserves smaller ones, and does not
alter game heaps or game options. Without this environment variable, the
existing JVM argument lists are preserved.

512 MiB is a test starting point, not a measured minimum or a guarantee of a
successful fresh preparation. A Java heap limit excludes metaspace, code,
native allocations and the launcher's memory. If an export fails, its log and
the diagnostic name the cap; increasing it must be judged against actual free
memory on the 2 GB machine. Manual CLI preparation needs these variables set
explicitly if it should use the same limits.

## Independent runtime check and remaining acceptance work

The legacy executable can check the vendor runtime without game files, a
Microsoft application ID, or a Minecraft account:

```bat
horizon-launcher-win7.exe --check-prepare-runtime
horizon-launcher-win7.exe --check-prepare-runtime --compiler
```

The first command downloads/probes the JRE; the second also verifies the JDK
and compiler. These commands do not prepare Minecraft resources, grant
ownership, or prove a game can run. Test in a fresh launcher data folder and
retain the command output. Repeat after disconnecting the network to verify
that a valid managed runtime can be reused.

| Check | Status at implementation time |
|---|---|
| Official package metadata, archive SHA-256 and release layout | Checked for both packages. |
| Preliminary normal PE imports | Checked as described above; not a full runtime proof. |
| Heap argument unit tests | Five Java module tests passed, including worker/datagen caps and unchanged uncapped arguments. |
| Failed runtime probe and damaged cached archive preserve the old installation | Covered by `legacy_java::tests`; see the main validation report for its suite result. |
| `java -version` / `javac -version` on physical Win7 x64 | **NOT TESTED.** |
| Exporters and data generator on Atom N455/GMA3150, 2 GB RAM | **NOT TESTED.** Heap sufficiency and CPU compatibility are unmeasured. |
| Fresh install on Win7 with valid Microsoft ownership | **INCOMPLETE.** No `HORIZON_MSA_CLIENT_ID`, licensed test files or physical Win7 machine were available. |
| Radeon HD4570 Win7 x64 test | **NOT TESTED.** |

For acceptance, first run both runtime checks on Win7 SP1 build 7601 x64.
Record OS updates, runtime version, exit status, missing-DLL or entry-point
errors and peak memory. Then configure a legitimate Minecraft-capable public
Microsoft client ID, sign in with an owner account, and perform the complete
official download and preparation in empty folders on that same machine.
Retain exporter/data-generator logs and verify the final resource stamp.
Repeat interrupted download, wrong-hash, offline, low-disk-space, Cyrillic and
long-path checks. Do not distribute the resulting Minecraft files.

Until those steps pass, this candidate path does **not** close L3 or the fresh
installation acceptance criterion.

## Observed Wine path issue and correction

The real managed helper verified and extracted the official JRE ZIP and completed its
staged version probe in an ASCII directory. The same archive failed at the version
probe in a Cyrillic directory with `could not find java.dll`. Direct invocation of the
same vendor runtime at that Cyrillic path failed too. The helper kept its archive and
did not publish an unchecked runtime.

The managed runtime now uses the core's existing ASCII tool location (a per-home folder
under ProgramData when needed), and Java discovery includes both old and tool locations.
This addresses the bootstrap path without renaming instance/world folders. Wine can
exercise the correction, but cannot establish actual Windows 7 or exporter compatibility.
Manual `java install 25` uses the same pinned helper; unsupported modern Rust installation
is rejected before downloading on Win7.
