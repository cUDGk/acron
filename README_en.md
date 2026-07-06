<div align="center">

# acron

### An on-device cron for Android that fires reliably even under Doze

[![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)](Cargo.toml)
[![Android](https://img.shields.io/badge/Android-3DDC84?style=flat&logo=android&logoColor=white)](#requirements)
[![Root](https://img.shields.io/badge/Root-required-critical?style=flat)](#requirements)
[![License: MIT](https://img.shields.io/badge/License-MIT-green?style=flat)](LICENSE)

**Runs on the wall clock even when WorkManager fires late or not at all.**

[日本語](README.md)

---

</div>

## Overview

Android's WorkManager / JobScheduler run late or skip entirely under Doze and battery optimization.

acron is a root daemon that evaluates the wall clock and fires like real cron. Its crontab is the standard 5-field syntax plus two extensions: `@Ns` (every N seconds, for sub-minute testing) and `@reboot`. Times honor the device timezone (`persist.sys.timezone`).

## Features

| Capability | Detail |
|------------|--------|
| Standard cron syntax | `min hour dom mon dow`. Supports `*/step`, ranges, lists, and the Vixie dom/dow OR quirk |
| Extensions | `@Ns` (every N seconds, sub-minute) and `@reboot` (once at start) |
| `run` | Run the scheduler as a resident daemon |
| `test` | Compute and print the next fire times for each entry |
| `check` | Evaluate what fires at a given minute |

## Requirements

- **Root required** (a device where `adb root` yields uid=0)
- Verified on: Android 14 / android-34 / x86_64 emulator
- Real devices (aarch64) need a rebuild with `--target arm64-v8a`

## Build

Needs Rust + Android NDK + [cargo-ndk](https://github.com/bbqsrc/cargo-ndk).

```bash
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/<version>
cargo ndk -t x86_64 --platform 34 build --release      # emulator
cargo ndk -t arm64-v8a --platform 34 build --release   # real device
```

## Usage

Example crontab:

```cron
# min hour dom mon dow  command
30 9 * * 1-5  echo weekday-morning
*/15 * * * *  echo quarter-hour
@30s          echo tick
@reboot       echo booted
```

```bash
adb push target/x86_64-linux-android/release/acron /data/local/tmp/

# preview next fire times
adb shell /data/local/tmp/acron test /data/local/tmp/crontab

# run resident (detached from the session)
adb shell "setsid /data/local/tmp/acron run /data/local/tmp/crontab --log /data/local/tmp/cron.log </dev/null >/dev/null 2>&1 &"
```

Run with no arguments to print usage.

## License

MIT License — see [LICENSE](LICENSE).
