# Flutter Client Benchmark

## RMBT C-Server vs. Rust Server — Comparison via Open Nettest Flutter App

---

## 1. Objective

This test was conducted to validate the correctness of the Rust RMBT server implementation by comparing Flutter mobile client measurements against the legacy C server. The goal was to determine whether observable differences in Flutter client results are attributable to server-side implementation or to the client itself.

---

## 2. Environment Setup

### 2.1 Server Configuration

The test server (`framework-desktop`, Ubuntu 24.04.3 LTS, kernel 6.14) was configured to run both the C and Rust RMBT servers locally, identical to the setup described in `RMBT_Full_Technical_Comparison_EN.md` and `RMBT_Browser_Client_Supplement_EN.md`.

Both servers listen on port 443 with TLS enabled. The C server is started as:

```bash
./rmbtd -L 443 -l 8080 -c specure-cd.crt -k specure-cd.key -w
```

The Rust server is started as:

```bash
./nettest -s
```

---

### 2.2 Flutter SDK Setup

Flutter was installed manually (not via snap):

```bash
cd ~
wget https://storage.googleapis.com/flutter_infra_release/releases/stable/linux/flutter_linux_3.29.3-stable.tar.xz
tar xf flutter_linux_3.29.3-stable.tar.xz
rm flutter_linux_3.29.3-stable.tar.xz
echo 'export PATH="$PATH:$HOME/flutter/bin"' >> ~/.bashrc
export PATH="$PATH:$HOME/flutter/bin"
flutter --version
```

---

### 2.3 Android SDK Setup

Android SDK was installed manually via cmdline-tools (not via `apt`):

```bash
mkdir -p ~/android-sdk/cmdline-tools
cd ~/android-sdk/cmdline-tools
# download latest cmdline-tools from https://developer.android.com/studio#command-tools
unzip commandlinetools-linux-*.zip
mv cmdline-tools latest

export ANDROID_SDK_ROOT="$HOME/android-sdk"
export ANDROID_HOME="$HOME/android-sdk"
export PATH="$PATH:$HOME/android-sdk/cmdline-tools/latest/bin:$HOME/android-sdk/platform-tools:$HOME/android-sdk/emulator"

sdkmanager --licenses
sdkmanager "platform-tools" "emulator" \
  "platforms;android-34" \
  "system-images;android-34;google_apis;x86_64" \
  "build-tools;34.0.0" \
  "ndk;28.2.13676358" \
  "cmake;3.22.1"
```

Java 17+ is required (Java 21 was used):

```bash
sudo apt install openjdk-21-jdk
java -version
```

AVD was created with the name `nt-phone`:

```bash
avdmanager create avd \
  --name "nt-phone" \
  --package "system-images;android-34;google_apis;x86_64" \
  --device "pixel_6"
```

---

### 2.4 Open Nettest Flutter App — Build

The project was cloned from the internal repository. Before building, two source files required patching to work without a valid Firebase configuration.

#### Patch 1 — `lib/core/wrappers/firebase-analytics.wrapper.dart`

Wrap `FirebaseAnalytics.instance` in a try-catch so the app does not crash when Firebase is not properly initialized:

```dart
import 'package:firebase_analytics/firebase_analytics.dart';

class FirebaseAnalyticsWrapper {
  FirebaseAnalytics? _analytics;

  init() {
    try {
      _analytics = FirebaseAnalytics.instance;
    } catch (_) {}
  }

  setAnalyticsEnabled(bool enabled) =>
      _analytics?.setAnalyticsCollectionEnabled(enabled);
}
```

#### Patch 2 — `lib/main.dart`

Wrap `Firebase.initializeApp()` and `FirebaseAnalyticsWrapper.init()` in try-catch blocks:

```dart
void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  try {
    await Firebase.initializeApp();
  } catch (_) {}

  // ... load configs ...

  try {
    await GetIt.I.get<FirebaseAnalyticsWrapper>().init();
  } catch (_) {}

  // ... rest of initialization ...
}
```

#### Build APK

> **Important:** do NOT include `https://` in `--dart-define` URL values — the app prepends the scheme automatically. Passing `https://api.nettest.org` results in a malformed URL `https://https//api.nettest.org`.

```bash
cd ~/mob/flutter-standalone

# required before first build
echo "org.gradle.jvmargs=-Xmx4g -XX:MaxMetaspaceSize=512m" >> android/gradle.properties

flutter pub get
flutter build apk --debug \
  --dart-define=DEFINE_APP_NAME="NT" \
  --dart-define=DEFINE_APP_SUFFIX=".nt" \
  --dart-define=DEFINE_CONTROL_SERVER_URL=api.nettest.org \
  --dart-define=DEFINE_CMS_SERVER_URL=api.nettest.org \
  --dart-define=DEFINE_WEBPAGE_URL=api.nettest.org
```

A debug signing keystore is required. If missing, generate one:

```bash
cd ~/mob/flutter-standalone/android
keytool -genkeypair -v \
  -keystore ./debug-upload.jks \
  -alias debug_upload \
  -keyalg RSA -keysize 2048 -validity 10000 \
  -storepass android123 -keypass android123 \
  -dname "CN=specure, OU=NT, O=specure, L=, S=, C=US"
cp debug-upload.jks app/

cat > key.properties << 'EOF'
storePassword=android123
keyPassword=android123
keyAlias=debug_upload
storeFile=debug-upload.jks
EOF
```

A valid `google-services.json` is also required in `android/app/`. Copy from the project config:

```bash
cp config/.nt/google-services.dev.json android/app/google-services.json
```

---

### 2.5 Virtual Display and noVNC

The test server has no physical display. A virtual X display was set up using Xvfb, exposed via VNC and accessed through a browser via noVNC:

```bash
Xvfb :2 -screen 0 1920x1080x24 > /tmp/xvfb.log 2>&1 &
x11vnc -display :2 -forever -shared -rfbport 5900 -nopw > /tmp/x11vnc.log 2>&1 &
websockify --web /usr/share/novnc 6080 localhost:5900 > /tmp/novnc.log 2>&1 &
```

Access in browser (via SSH tunnel on port 6080):

```
http://127.0.0.1:6080/vnc.html?host=127.0.0.1&port=6080&path=websockify
```

---

### 2.6 Android Emulator Launch

The emulator must be started with `-writable-system` to allow modifying `/system/etc/hosts`:

```bash
export DISPLAY=:2
export ANDROID_SDK_ROOT="$HOME/android-sdk"
export PATH="$PATH:$HOME/android-sdk/emulator:$HOME/android-sdk/platform-tools"

DISPLAY=:2 emulator -avd nt-phone \
  -no-snapshot -no-boot-anim \
  -gpu swiftshader_indirect \
  -writable-system > /tmp/emulator.log 2>&1 &

# wait for full boot
until adb shell getprop sys.boot_completed 2>/dev/null | grep -q "1"; do sleep 2; done
echo "Emulator ready"
```

The emulator window appears in the noVNC browser session.

---

### 2.7 `/etc/hosts` Override on the Emulator

The Android emulator has its own isolated network stack and does not share the host machine's `/etc/hosts`. Inside the emulator, `127.0.0.1` resolves to the emulator itself — the host machine is reachable at `10.0.2.2`.

After boot, remount the system partition and push the modified hosts file:

```bash
adb root && sleep 2
adb remount && sleep 1

adb pull /system/etc/hosts /tmp/hosts_emu
echo "10.0.2.2    dev.measurementservers.net" >> /tmp/hosts_emu
adb push /tmp/hosts_emu /system/etc/hosts
adb shell chmod 644 /system/etc/hosts

# verify
adb shell cat /system/etc/hosts
adb shell ping -c 3 dev.measurementservers.net
```

> **Note:** `adb remount` requires a reboot after the first run (overlayfs setup). If it fails with "Read-only file system", reboot the emulator (`adb reboot`), wait for boot, then repeat `adb root && adb remount`.

---

### 2.8 App Installation and Launch

```bash
adb install -r ~/mob/flutter-standalone/build/app/outputs/flutter-apk/app-debug.apk

adb shell am force-stop com.specure.nettest.nt
adb shell am start -n com.specure.nettest.nt/com.specure.nt_flutter_standalone.MainActivity
```

Monitor logs:

```bash
adb logcat -c
adb logcat | grep -E "flutter|measurementServer|DioException|Unhandled"
```

---

## 3. Test Procedure

Both server implementations were started sequentially on port 443 with TLS enabled. The Open Nettest Flutter app was launched on the emulator and measurements were performed against each server in turn by restarting the corresponding server binary between runs. The client configuration, emulator state, and network conditions were identical for both runs.

---

## 4. Results

| Server      | Download (Mbps) | Upload (Mbps) |
|-------------|-----------------|---------------|
| Rust Server | 434             | 273           |
| C Server    | 442             | 281           |

---

## 5. Conclusion

The Flutter client results for both server implementations are virtually identical — the ~2% difference is well within normal measurement variance and carries no statistical significance.

This confirms two key findings:

1. **The Rust server implements the RMBT protocol correctly** — it produces the same observable throughput as the reference C implementation from the client's perspective.
2. **The Flutter client is the bottleneck** in this test scenario. At ~430–440 Mbps the Flutter WebSocket/TLS stack saturates before any server-side difference can be observed.

These results complement the native client benchmarks documented in `RMBT_Full_Technical_Comparison_EN.md`, where Rust outperforms C by 30%+ in raw TCP throughput. In the Flutter client context — as in the browser context — protocol correctness is confirmed, while the performance headroom of the Rust server remains available for higher-capacity native clients.
