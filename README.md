<div align="center">

# acron

### Doze でも確実に発火する、Android 端末内の cron

[![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)](Cargo.toml)
[![Android](https://img.shields.io/badge/Android-3DDC84?style=flat&logo=android&logoColor=white)](#動作環境)
[![Root](https://img.shields.io/badge/Root-required-critical?style=flat)](#動作環境)
[![License: MIT](https://img.shields.io/badge/License-MIT-green?style=flat)](LICENSE)

**WorkManager が遅れても落としても、壁時計どおりに実行する。**

[English](README_en.md)

---

</div>

## 概要

Android の WorkManager / JobScheduler は Doze やバッテリー最適化の影響で、指定時刻に遅れて動いたり、まったく発火しなかったりする。

acron は root 常駐デーモンとして壁時計で判定し、本物の cron のように確実に発火する。crontab は標準の 5 フィールド構文に、サブ分テスト用の `@Ns`（N 秒毎）と `@reboot` を足したもの。時刻は端末のタイムゾーン（`persist.sys.timezone`）に従う。

## 特徴

| 機能 | 内容 |
|------|------|
| 標準 cron 構文 | `分 時 日 月 曜日`。`*/step`・範囲・リスト・Vixie の日/曜日 OR に対応 |
| 拡張 | `@Ns`（N 秒毎、サブ分）と `@reboot`（起動時に一度） |
| `run` | スケジューラをデーモンとして常駐実行 |
| `test` | 各エントリの次回発火時刻を計算して表示 |
| `check` | 指定した時刻に何が発火するかを評価 |

## 動作環境

- **root 必須**（`adb root` で uid=0 が取れる環境）
- 検証済み: Android 14 / android-34 / x86_64 エミュレータ
- 実機（aarch64）は `--target arm64-v8a` で再ビルドが必要

## ビルド

要 Rust + Android NDK + [cargo-ndk](https://github.com/bbqsrc/cargo-ndk)。

```bash
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/<version>
cargo ndk -t x86_64 --platform 34 build --release      # エミュレータ
cargo ndk -t arm64-v8a --platform 34 build --release   # 実機
```

## 使い方

crontab の例:

```cron
# 分 時 日 月 曜日  コマンド
30 9 * * 1-5  echo weekday-morning
*/15 * * * *  echo quarter-hour
@30s          echo tick
@reboot       echo booted
```

```bash
adb push target/x86_64-linux-android/release/acron /data/local/tmp/

# 次回発火時刻を確認
adb shell /data/local/tmp/acron test /data/local/tmp/crontab

# 常駐実行（セッションから切り離す）
adb shell "setsid /data/local/tmp/acron run /data/local/tmp/crontab --log /data/local/tmp/cron.log </dev/null >/dev/null 2>&1 &"
```

引数なしで実行すると使い方を表示する。

## ライセンス

MIT License — 詳細は [LICENSE](LICENSE) を参照。
