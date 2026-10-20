#!/bin/sh
# Builds dist/Glint.app, dist/Glint.zip (first install) and dist/Glint-macos.app.tar.gz
# (what the in-app updater installs). SIGN_IDENTITY picks the signing certificate
# (ad-hoc by default) and SIGN_KEYCHAIN the keychain holding it. UNIVERSAL=1 builds
# arm64 + x86_64 (needs both rustup targets); otherwise builds for the host only.
set -eu
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' app/Cargo.toml | head -1)

build() { # crate, binary -> prints path of the (possibly lipo'd) binary
  if [ "${UNIVERSAL:-0}" = 1 ]; then
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
      (cd "$1" && cargo build --release --target "$t" --bin "$2" >&2)
    done
    lipo -create -output "$1/target/$2-universal" \
      "$1/target/aarch64-apple-darwin/release/$2" "$1/target/x86_64-apple-darwin/release/$2"
    echo "$1/target/$2-universal"
  else
    (cd "$1" && cargo build --release --bin "$2" >&2)
    echo "$1/target/release/$2"
  fi
}

APP=dist/Glint.app
rm -rf dist && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$(build app glint)" "$APP/Contents/MacOS/Glint"
sed "s/__VERSION__/$VERSION/g" assets/Info.plist > "$APP/Contents/Info.plist"
cp assets/AppIcon.icns "$APP/Contents/Resources/"
printf 'APPL????' > "$APP/Contents/PkgInfo"
codesign --force --deep ${SIGN_KEYCHAIN:+--keychain "$SIGN_KEYCHAIN"} --sign "${SIGN_IDENTITY:--}" "$APP"
(cd dist && ditto -c -k --keepParent Glint.app Glint.zip && tar -czf Glint-macos.app.tar.gz Glint.app)
echo "built $APP ($VERSION)"
