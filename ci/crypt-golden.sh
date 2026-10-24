#!/usr/bin/env bash
# border-golden.sh — vanilla's own world-border numbers and the six clientbound
# border packets, captured at a fixed script of operations.
#
#     ./ci/border-golden.sh [out.json]
#
# Produces `crates/rustmc-proto/golden/crypt.json`, which is a
# **committed fixture compared against**, not a `data/vanilla/` input the
# server reads at runtime. The distinction is the one `ci/attributes-golden.sh`
# documents: a stale input silently changes behaviour, a stale fixture reddens
# a test.
#
# See `tools/goldengen/vanilla/CryptGolden.java` for what the script of
# operations is and why a default border alone would prove nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-proto/golden/crypt.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$(dirname "$OUT")"
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/CryptGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx1G -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.CryptGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. Nothing in the border draws from a random
# source; a disagreement would mean the capture is reading something ambient
# and every byte below would be a coincidence.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of CryptGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import hashlib, json, sys

d = json.load(open(sys.argv[1]))
print(f"crypt -> {sys.argv[1]} (identical across two runs)")

fail = []
h = lambda k: bytes.fromhex(d[k])

# The public key encoding is what the client is handed; if it is not an
# X.509 SubjectPublicKeyInfo the whole handshake is wrong and nothing else
# below would say so.
if d["publicKeyFormat"] != "X.509":
    fail.append(f"public key format is {d['publicKeyFormat']}, not X.509")
if d["privateKeyFormat"] != "PKCS#8":
    fail.append(f"private key format is {d['privateKeyFormat']}, not PKCS#8")

# The digest is SHA-1 over serverId + SECRET + PUBLIC KEY, in that order —
# `Crypt.digestData(String, PublicKey, SecretKey)` takes the key before the
# secret and hashes them the other way round. Recomputed here so the capture
# is not merely self-consistent.
want = hashlib.sha1(b"" + h("secretKey") + h("publicKeyEncoded")).digest()
if want != h("digest"):
    fail.append(f"digest is not sha1(serverId||secret||publicKey): {want.hex()}")

# The hash is a SIGNED BigInteger in base 16. At least one of the six ids
# must produce a negative one, or the sign is untested.
if not any(d[k].startswith("-") for k in d if k.startswith("hash_")):
    fail.append("no server id produced a negative hash — the sign is untested")
if d["hash_empty"] != d["serverHash"]:
    fail.append("serverHash and hash_empty disagree")

# The round trips.
if h("decryptedSecret") != h("secretKey"):
    fail.append("RSA round trip lost the secret")
if h("decryptedChallenge") != h("challenge"):
    fail.append("RSA round trip lost the challenge")
if h("cfb8Decrypted") != h("cfb8Plain"):
    fail.append("CFB8 round trip lost the plaintext")
if h("cfb8Cipher") != h("cfb8CipherSplit"):
    fail.append("CFB8 in three pieces differs from CFB8 in one — the stream is not carrying state")
if h("cfb8Cipher") == h("cfb8Plain"):
    fail.append("CFB8 ciphertext equals its plaintext")
if len(h("fixedCiphertext")) != 128:
    fail.append(f"fixed ciphertext is {len(h('fixedCiphertext'))} bytes, not 128")

# SHA-1 against the standard's own answers, so a JVM whose SHA-1 was wrong
# could not seed this file.
if d["sha1Abc"] != "a9993e364706816aba3e25717850c26c9cd0d89d":
    fail.append(f"sha1(\"abc\") is {d['sha1Abc']}")
if d["sha1Empty"] != "da39a3ee5e6b4b0d3255bfef95601890afd80709":
    fail.append(f"sha1(\"\") is {d['sha1Empty']}")

print(f"  modulus {len(bin(int(d['modulus']))) - 2} bits, "
      f"public key {len(h('publicKeyEncoded'))} bytes X.509, "
      f"private key {len(h('privateKeyEncoded'))} bytes PKCS#8")
print(f"  server hash (id \"\") = {d['serverHash']}")
for k in sorted(k for k in d if k.startswith("hash_")):
    print(f"  {k:<14} {d[k]}")
print(f"  cfb8 {len(h('cfb8Plain'))} bytes, aes block, sha1 at 0/3/55/56/119 bytes")
print(f"  clientbound hello {len(h('clientboundHello'))} bytes, "
      f"serverbound key {d['serverboundKeyLength']} bytes")

# The hello's shape, checked here rather than assumed by the consumer: the
# empty server id is one zero byte, then two length-prefixed arrays and a
# boolean.
hello = h("clientboundHello")
pk = h("publicKeyEncoded")
if hello[0] != 0:
    fail.append("the clientbound hello does not start with an empty server id")
if hello[-1] != 1:
    fail.append("shouldAuthenticate is not true in the captured hello")
if pk not in hello:
    fail.append("the captured hello does not contain the public key")
# 1 byte for the empty server id's varint length, then the public key with a
# TWO-byte varint length (162 > 127), then the four-byte challenge with a
# one-byte length, then the boolean.
if len(hello) != 1 + 2 + len(pk) + 1 + 4 + 1:
    fail.append(f"the hello is {len(hello)} bytes, not the expected field widths")
if not d["serverboundKeyChallengeValid"]:
    fail.append("a real key packet failed its own challenge check")
if d["serverboundKeyWrongChallengeValid"]:
    fail.append("a key packet accepted the WRONG challenge — isChallengeValid is inert")
if h("serverboundKeySecret") != h("secretKey"):
    fail.append("getSecretKey did not recover the secret")

if fail:
    for x in fail:
        print(f"  !! {x}")
    sys.exit(1)
PYEOF
