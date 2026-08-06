# Building Rust for Flutter (FFI)

Notes on integrating the `nettest` crate into a Flutter app via FFI. Based on a
review of the `flutter` branch (commits `a9ad8de`, `edcd07d`, `5050956`,
`43a4ae6`), which had a working `flutter-ex/` example.

> The goal of these notes is to capture **exactly how** to build, so the time
> isn't spent again. A separate repository (`nettest-flutter-lib`) builds the
> Flutter library using this crate as a dependency.

---

## 1. Crate changes (`Cargo.toml`)

The key difference from the server build — the crate must compile as a
dynamic/static library for FFI:

```toml
[lib]
name = "nettest"
crate-type = ["cdylib", "rlib", "staticlib"]
```

- `cdylib` → `.so` (Android), `.dylib` (macOS), `.dll` (Windows)
- `staticlib` → `.a` (iOS, linked statically)
- `rlib` → so the crate can also be used as a regular Rust dependency

### Dependencies that break the mobile build

`plotters` / `plotters-backend` / `textplots` pull in **fontconfig**, which does
not build for Android. Move them out of the mobile targets:

```toml
[target.'cfg(not(target_os = "android"))'.dependencies]
plotters = "0.3.7"
plotters-backend = "0.3.7"
textplots = "0.8.7"
```

The `flutter` branch also dropped `mdns-sd` and `include_dir` (not needed by the
client).

## 2. Code changes (`src/lib.rs`)

### Conditional compilation of the server part

The server module is not needed on Android and pulls in extra dependencies:

```rust
#[cfg(not(target_os = "android"))]
pub mod mioserver;
```

### FFI functions with `#[no_mangle] extern "C"`

`src/lib.rs` exports C-compatible functions. The essentials:

- `client_run_ffi(args_json, config_json) -> *mut c_char` — synchronous run.
- `client_run_with_progress_ffi(...)` — run with progress; progress is stored in
  a global `lazy_static` `Mutex<Option<MeasurementProgress>>`, polled by Dart via
  `get_progress_ffi()`.
- `free_string(ptr)` — frees strings handed to Dart (`CString::into_raw`).

Arguments are passed as a **JSON array of strings** (`["-c", "--server", "..."]`),
the result is JSON (`{"success": true}` or `{"error": "..."}`). The FFI creates
its own `tokio::runtime::Runtime` and calls `block_on`.

> Note: the `nettest-flutter-lib` library uses a different, per-handle FFI surface
> (`nettest_measurement_*`). The notes above describe the original
> `flutter`-branch approach, but the build mechanics (crate-type, NDK targets,
> linking) are identical.

## 3. Building for Android

### Requirements
- Android NDK (via Android Studio: Tools → SDK Manager → SDK Tools → NDK).
- `cargo install cargo-ndk`
- `ANDROID_NDK_HOME` (optional) — the script can find the NDK in
  `~/Library/Android/sdk/ndk/*`.

### Targets
```bash
rustup target add aarch64-linux-android      # arm64-v8a
rustup target add armv7-linux-androideabi    # armeabi-v7a
rustup target add x86_64-linux-android       # x86_64
# i686-linux-android is NOT used — deprecated, atomic-operation issues in OpenSSL.
```

### Environment variables (disable fontconfig)
```bash
export FONTCONFIG_NO_PKG_CONFIG=1
export RUST_FONTCONFIG_DLOPEN=1
export PKG_CONFIG_ALLOW_CROSS=1
```

### Build command (per target)
```bash
cargo ndk -t aarch64-linux-android build --release
```

### Where to place the `.so`
Flutter automatically picks up native libs from `jniLibs/`:
```
android/app/src/main/jniLibs/arm64-v8a/libnettest.so
android/app/src/main/jniLibs/armeabi-v7a/libnettest.so
android/app/src/main/jniLibs/x86_64/libnettest.so
```
Copy from `target/<rust_target>/release/libnettest.so`.

A ready script does this in a loop: `build_android_simple.sh`.

### Loading in Dart
```dart
DynamicLibrary.open('libnettest.so');
```

## 4. Building for iOS

### Target
```bash
rustup target add aarch64-apple-ios-sim
cargo build --release --target aarch64-apple-ios-sim
```

> On Apple Silicon a **single** `aarch64-apple-ios-sim` library works for both
> the simulator and the device. For a real device release, build
> `aarch64-apple-ios` separately.

### Where to place the `.a`
```
ios/Rust/libnettest.a
```
Copy from `target/aarch64-apple-ios-sim/release/libnettest.a`.
Check the architecture: `lipo -info ios/Rust/libnettest.a`.

### Linking (Xcode via .xcconfig)
In `ios/Flutter/Debug.xcconfig` and `Release.xcconfig`:
```
LIBRARY_SEARCH_PATHS = $(inherited) $(PROJECT_DIR)/Rust
OTHER_LDFLAGS = $(inherited) -force_load $(PROJECT_DIR)/Rust/libnettest.a
```
`-force_load` is required, otherwise the linker drops the "unused" FFI symbols.

### Loading in Dart
The library is statically linked into the executable, so:
```dart
DynamicLibrary.process();
```

A ready script: `build_ios_simple.sh`.

> ⚠️ The `flutter` branch still has stale check scripts (`check_ios_lib.sh`,
> `check_xcode_setup.sh`, `test_symbols.sh`) referencing the old approach via
> `libnettest.xcframework` in `ios/Frameworks/`. The current working approach is
> the static `.a` + `-force_load` (see above), not an XCFramework.

## 5. Building for macOS (desktop)

```bash
cargo build --release            # -> target/release/libnettest.dylib
```
The dynamic lib is placed into the app bundle via a build-phase script
(`macos/copy_lib.sh`) that copies `Runner/Frameworks/libnettest.dylib` into
`<app>.app/Contents/Frameworks/` and fixes the install name:
```bash
install_name_tool -id @rpath/libnettest.dylib "$LIB_DEST"
```
Dart looks for the `.dylib` next to the executable / in `../Frameworks/` / in
`target/release/`.

## 6. Building for Linux / Windows (desktop)

- Linux: `cargo build --release` → `target/release/libnettest.so`,
  Dart looks in the current folder and `../target/release/`.
- Windows: `cargo build --release` → `target/release/nettest.dll`,
  `DynamicLibrary.open('nettest.dll')`.

## 7. Quick "how to build" checklist

| Platform | Artifact | Where | Load in Dart |
|----------|----------|-------|--------------|
| Android | `libnettest.so` (per-ABI) | `android/app/src/main/jniLibs/<abi>/` | `DynamicLibrary.open('libnettest.so')` |
| iOS | `libnettest.a` | `ios/Rust/` + `-force_load` in xcconfig | `DynamicLibrary.process()` |
| macOS | `libnettest.dylib` | app bundle `Contents/Frameworks/` | `DynamicLibrary.open(<path>)` |
| Linux | `libnettest.so` | next to exe / `target/release` | `DynamicLibrary.open(...)` |
| Windows | `nettest.dll` | next to exe | `DynamicLibrary.open('nettest.dll')` |

## 8. Main gotchas (where time was spent)

1. **fontconfig/plotters don't build for Android** → move them under
   `cfg(not(target_os = "android"))` + the `FONTCONFIG_*` env vars.
2. **i686-linux-android fails** on OpenSSL atomic operations → exclude it.
3. **iOS symbols are stripped by the linker** without `-force_load`.
4. **`crate-type`** must include `cdylib` (so/dylib) and `staticlib` (iOS .a).
5. **The `mioserver` server module** is not needed by the mobile client and pulls
   in extra deps → `#[cfg(not(target_os = "android"))]`.
6. On **Apple Silicon**, `aarch64-apple-ios-sim` is universal for sim+device
   during development — don't multiply targets prematurely.
