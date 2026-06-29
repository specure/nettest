# Сборка Rust под Flutter (FFI)

Заметки по интеграции крейта `nettest` (этот репозиторий) в Flutter-приложение
через FFI. Основано на разборе ветки `flutter` (коммиты `a9ad8de`, `edcd07d`,
`5050956`, `43a4ae6`), где был рабочий пример `flutter-ex/`.

> Цель этих заметок — зафиксировать **как именно** собирать, чтобы не тратить
> время повторно. В дальнейшем планируется отдельный репозиторий, который будет
> собирать Flutter-библиотеку, используя текущий репозиторий как крейт.

---

## 1. Что нужно изменить в крейте (`Cargo.toml`)

Ключевое отличие от серверной сборки — крейт должен компилироваться как
динамическая/статическая библиотека для FFI:

```toml
[lib]
name = "nettest"
crate-type = ["cdylib", "rlib", "staticlib"]
```

- `cdylib` → `.so` (Android), `.dylib` (macOS), `.dll` (Windows)
- `staticlib` → `.a` (iOS, линкуется статически)
- `rlib` → чтобы крейт можно было использовать и как обычную Rust-зависимость

### Зависимости, ломающие мобильную сборку

`plotters` / `plotters-backend` / `textplots` тянут **fontconfig**, который не
собирается под Android. Их нужно вынести из мобильных таргетов:

```toml
[target.'cfg(not(target_os = "android"))'.dependencies]
plotters = "0.3.7"
plotters-backend = "0.3.7"
textplots = "0.8.7"
```

В ветке `flutter` также были убраны `mdns-sd` и `include_dir` (не нужны клиенту).

## 2. Что изменить в коде (`src/lib.rs`)

### Условная компиляция серверной части

Серверный модуль не нужен на Android и тянет лишние зависимости:

```rust
#[cfg(not(target_os = "android"))]
pub mod mioserver;
```

### FFI-функции с `#[no_mangle] extern "C"`

В `src/lib.rs` экспортируются C-совместимые функции. Главное:

- `client_run_ffi(args_json, config_json) -> *mut c_char` — синхронный запуск.
- `client_run_with_progress_ffi(...)` — запуск с прогрессом; прогресс
  складывается в глобальный `lazy_static` `Mutex<Option<MeasurementProgress>>`,
  Dart опрашивает его через `get_progress_ffi()`.
- `free_string(ptr)` — освобождение строк, отданных в Dart (`CString::into_raw`).

Аргументы передаются как **JSON-массив строк** (`["-c", "--server", "..."]`),
результат — JSON (`{"success": true}` или `{"error": "..."}`).
Внутри FFI создаётся свой `tokio::runtime::Runtime` и вызывает `block_on`.

## 3. Сборка под Android

### Требования
- Android NDK (через Android Studio: Tools → SDK Manager → SDK Tools → NDK).
- `cargo install cargo-ndk`
- `ANDROID_NDK_HOME` (опц.) — скрипт умеет искать NDK в
  `~/Library/Android/sdk/ndk/*`.

### Таргеты
```bash
rustup target add aarch64-linux-android      # arm64-v8a
rustup target add armv7-linux-androideabi    # armeabi-v7a
rustup target add x86_64-linux-android       # x86_64
# i686-linux-android НЕ используем — устаревшая, проблемы с атомарными
# операциями в OpenSSL.
```

### Переменные окружения (отключаем fontconfig)
```bash
export FONTCONFIG_NO_PKG_CONFIG=1
export RUST_FONTCONFIG_DLOPEN=1
export PKG_CONFIG_ALLOW_CROSS=1
```

### Команда сборки (на каждый таргет)
```bash
cargo ndk -t aarch64-linux-android build --release
```

### Куда класть `.so`
Flutter автоматически подхватывает нативные либы из `jniLibs/`:
```
flutter-ex/android/app/src/main/jniLibs/arm64-v8a/libnettest.so
flutter-ex/android/app/src/main/jniLibs/armeabi-v7a/libnettest.so
flutter-ex/android/app/src/main/jniLibs/x86_64/libnettest.so
```
Копируем из `target/<rust_target>/release/libnettest.so`.

Готовый скрипт делает всё это циклом: `build_android_simple.sh`.

### Загрузка в Dart
```dart
DynamicLibrary.open('libnettest.so');
```

## 4. Сборка под iOS

### Таргет
```bash
rustup target add aarch64-apple-ios-sim
cargo build --release --target aarch64-apple-ios-sim
```

> На Apple Silicon **одна** библиотека `aarch64-apple-ios-sim` работает и для
> симулятора, и для устройства. Для настоящего релиза под устройство отдельно
> собирается `aarch64-apple-ios`.

### Куда класть `.a`
```
flutter-ex/ios/Rust/libnettest.a
```
Копируем из `target/aarch64-apple-ios-sim/release/libnettest.a`.
Проверка архитектуры: `lipo -info flutter-ex/ios/Rust/libnettest.a`.

### Линковка (Xcode через .xcconfig)
В `flutter-ex/ios/Flutter/Debug.xcconfig` и `Release.xcconfig`:
```
LIBRARY_SEARCH_PATHS = $(inherited) $(PROJECT_DIR)/Rust
OTHER_LDFLAGS = $(inherited) -force_load $(PROJECT_DIR)/Rust/libnettest.a
```
`-force_load` обязателен, иначе линкер выкинет «неиспользуемые» FFI-символы.

### Загрузка в Dart
Библиотека вшита статически в исполняемый файл, поэтому:
```dart
DynamicLibrary.process();
```

Готовый скрипт: `build_ios_simple.sh`.

> ⚠️ В ветке `flutter` остались устаревшие проверочные скрипты
> (`check_ios_lib.sh`, `check_xcode_setup.sh`, `test_symbols.sh`), которые
> ссылаются на старый подход через `libnettest.xcframework` в
> `flutter-ex/ios/Frameworks/`. Актуальный рабочий подход — статическая `.a` +
> `-force_load` (см. выше), а не XCFramework.

## 5. Сборка под macOS (desktop)

```bash
cargo build --release            # -> target/release/libnettest.dylib
```
Динамическая либа кладётся в app bundle через build phase скрипт
`flutter-ex/macos/copy_lib.sh`, который копирует
`Runner/Frameworks/libnettest.dylib` в
`<app>.app/Contents/Frameworks/` и правит install name:
```bash
install_name_tool -id @rpath/libnettest.dylib "$LIB_DEST"
```
Dart ищет `.dylib` рядом с исполняемым файлом / в `../Frameworks/` / в
`target/release/` (см. `_loadLibrary()` в `flutter-ex/lib/main.dart`).

## 6. Сборка под Linux / Windows (desktop)

- Linux: `cargo build --release` → `target/release/libnettest.so`,
  Dart ищет в текущей папке и `../target/release/`.
- Windows: `cargo build --release` → `target/release/nettest.dll`,
  `DynamicLibrary.open('nettest.dll')`.

## 7. Сторона Flutter

- `flutter-ex/pubspec.yaml`: зависимости `ffi: ^2.1.0`, `convert: ^3.1.1`.
- `flutter-ex/lib/ffi_bindings.dart`: класс `NettestFFI(DynamicLibrary lib)` с
  `lookupFunction` на `client_run_ffi`, `client_run_with_progress_ffi`,
  `get_progress_ffi`, `free_string`. Аргументы — JSON через `jsonEncode`,
  строки конвертируются `toNativeUtf8()` / `toDartString()`, после вызова
  обязательно `free_string` (Rust-память) и `malloc.free` (Dart-память).
- `flutter-ex/lib/main.dart`: `_loadLibrary()` выбирает способ загрузки по
  `Platform.isAndroid/isIOS/isMacOS/...`.

## 8. Краткий чек-лист «как собрать»

| Платформа | Артефакт | Куда | Загрузка в Dart |
|-----------|----------|------|------------------|
| Android | `libnettest.so` (per-ABI) | `android/app/src/main/jniLibs/<abi>/` | `DynamicLibrary.open('libnettest.so')` |
| iOS | `libnettest.a` | `ios/Rust/` + `-force_load` в xcconfig | `DynamicLibrary.process()` |
| macOS | `libnettest.dylib` | app bundle `Contents/Frameworks/` | `DynamicLibrary.open(<path>)` |
| Linux | `libnettest.so` | рядом / `target/release` | `DynamicLibrary.open(...)` |
| Windows | `nettest.dll` | рядом | `DynamicLibrary.open('nettest.dll')` |

## 9. Главные грабли (на что ушло время)

1. **fontconfig/plotters не собираются под Android** → вынести в
   `cfg(not(target_os = "android"))` + env-переменные `FONTCONFIG_*`.
2. **i686-linux-android падает** на атомарных операциях OpenSSL → исключить.
3. **iOS-символы выпиливаются линкером** без `-force_load`.
4. **`crate-type`** обязан включать `cdylib` (so/dylib) и `staticlib` (iOS .a).
5. **Серверный модуль `mioserver`** не нужен мобильному клиенту и тянет лишнее →
   `#[cfg(not(target_os = "android"))]`.
6. На **Apple Silicon** `aarch64-apple-ios-sim` универсален для sim+device при
   разработке — не плодить таргеты раньше времени.
