#!/usr/bin/env bash
# Creates a self-signed code-signing identity, "Flatline Dev", in your login
# keychain, and trusts it for code signing only.
#
# Optional: gives Flatline a real, stable signature instead of an ad-hoc one;
# scripts/bundle.sh uses it automatically once it is valid. Not required for
# the System Audio Recording permission, which follows the bundle identifier.
#
# codesign refuses an untrusted certificate, so the script adds a trust
# setting limited to the Code Signing policy (macOS asks for your password).
# Everything stays on this Mac; remove it any time from Keychain Access
# ("Flatline Dev").
set -euo pipefail

NAME="Flatline Dev"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

valid() { security find-identity -v -p codesigning | grep -q "\"$NAME\""; }

if valid; then
  echo "\"$NAME\" is already a valid code-signing identity."
  exit 0
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

if security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
  echo "\"$NAME\" exists but is not trusted yet; adding the code-signing trust."
  security find-certificate -c "$NAME" -p "$KEYCHAIN" >"$TMP/cert.pem"
else
  cat >"$TMP/cert.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
basicConstraints = critical, CA:false
EOF
  # Throwaway password for the temporary .p12 (no pipe: `tr | head` trips pipefail).
  PASS=$(/usr/bin/openssl rand -hex 16)
  # macOS' own LibreSSL produces a PKCS#12 that `security import` accepts.
  /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -sha256 \
    -keyout "$TMP/key.pem" -out "$TMP/cert.pem" -config "$TMP/cert.cnf" 2>/dev/null
  /usr/bin/openssl pkcs12 -export -inkey "$TMP/key.pem" -in "$TMP/cert.pem" \
    -name "$NAME" -out "$TMP/identity.p12" -passout "pass:$PASS"
  # -T lets codesign use the key without a keychain prompt on every build.
  security import "$TMP/identity.p12" -k "$KEYCHAIN" -P "$PASS" -T /usr/bin/codesign >/dev/null
  echo "Imported \"$NAME\" into your login keychain."
fi

echo "Trusting it for code signing (macOS will ask for your password)…"
security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" "$TMP/cert.pem"

if valid; then
  echo "Done: \"$NAME\" is a valid code-signing identity."
  echo "Next build (./scripts/bundle.sh) is signed with it; macOS may ask once"
  echo "more to allow system audio because the signature changed."
else
  echo "\"$NAME\" is still not valid for code signing." >&2
  echo "Open Keychain Access, double-click \"$NAME\", Trust → Code Signing: Always Trust." >&2
  exit 1
fi
