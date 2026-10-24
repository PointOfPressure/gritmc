package net.minecraft.world.level.levelgen;

import java.io.PrintStream;
import java.math.BigInteger;
import java.security.KeyFactory;
import java.security.KeyPair;
import java.security.PrivateKey;
import java.security.PublicKey;
import java.security.spec.RSAPrivateCrtKeySpec;
import java.security.spec.RSAPublicKeySpec;
import javax.crypto.Cipher;
import javax.crypto.SecretKey;
import javax.crypto.spec.SecretKeySpec;
import io.netty.buffer.Unpooled;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.protocol.login.ClientboundHelloPacket;
import net.minecraft.network.protocol.login.ServerboundKeyPacket;
import net.minecraft.util.Crypt;

/**
 * Online mode's cryptography, as vanilla's own `Crypt` produces it.
 *
 * <p>Everything here runs through `net.minecraft.util.Crypt` rather than
 * through the JCE directly, so what is captured is what the server does and
 * not what a reading of the standard says it should: `Crypt.digestData`'s
 * argument order (serverId, then the SECRET, then the public key — the method
 * takes them in a different order from the one it hashes them in),
 * `Crypt.getCipher`'s `AES/CFB8/NoPadding` with the key reused as the IV, and
 * `Crypt.encryptUsingKey`'s bare `RSA`, which is SunJCE's
 * `RSA/ECB/PKCS1Padding`.
 *
 * <p>**The RSA key pair is fixed, not generated.** A generated one would make
 * this file different on every run, and the two-runs-must-agree check in
 * `ci/crypt-golden.sh` would refuse it — correctly, because a golden nobody
 * can reproduce is not a golden. The modulus and exponents below were produced
 * once by `Crypt.generateKeyPair()` and are pasted in; the key is rebuilt from
 * them through `KeyFactory`, so the encodings captured are still the JCE's own
 * and not a transcription.
 *
 * <p>Run by {@code ci/crypt-golden.sh}. Writes to
 * {@code crates/rustmc-proto/golden/crypt.json}.
 */
public final class CryptGolden {
    private CryptGolden() {}

    // A 1024-bit RSA key, from one run of Crypt.generateKeyPair(). Fixed so the
    // capture is reproducible; see the class docs.
    private static final String N =
            "134667493458173536663937701940886549649500053255774527714989"
                    + "665384911028515785178843196989203376713141677097577395262384"
                    + "845042900167131066021373154364065302512473323847710267465766"
                    + "862416165727627793997361431202863392902968936898253140744418"
                    + "154361196104900739025868301624245388688439402101549572443701"
                    + "090005809";
    private static final String E = "65537";
    private static final String D =
            "170047582604195355701728253110732356016362923343784748173611"
                    + "391411390697847380906589693865168156001892663506263901993357"
                    + "307345853538168231206779916526515131449677616175104742650347"
                    + "818364991789073112661632369873391739236085319687989033348983"
                    + "231915064170598455498415958727263579062882592451988322410147"
                    + "59857343";
    private static final String P =
            "100861154722361270478086221702455274535756626412467166075686"
                    + "737276447386363610477331934718722025831248883874123967537293"
                    + "39461732319878437937975745749880519";
    private static final String Q =
            "133517699483879974602580045197075065372327901138284403593878"
                    + "324093846949522221361676223132059585323672230268350156540496"
                    + "94552740113924809916314443852387911";

    public static void main(String[] args) throws Exception {
        PrintStream out = System.out;

        BigInteger n = new BigInteger(N);
        BigInteger e = new BigInteger(E);
        BigInteger d = new BigInteger(D);
        BigInteger p = new BigInteger(P);
        BigInteger q = new BigInteger(Q);
        // Refuse rather than capture a key that is not one: a pasted constant
        // with a typo would still produce a file, and everything below it
        // would be a check against nonsense.
        if (!p.multiply(q).equals(n)) {
            throw new IllegalStateException("p * q != n — the pasted key is wrong");
        }
        BigInteger dp = d.mod(p.subtract(BigInteger.ONE));
        BigInteger dq = d.mod(q.subtract(BigInteger.ONE));
        BigInteger qinv = q.modInverse(p);

        KeyFactory kf = KeyFactory.getInstance("RSA");
        PublicKey pub = kf.generatePublic(new RSAPublicKeySpec(n, e));
        PrivateKey priv = kf.generatePrivate(new RSAPrivateCrtKeySpec(n, e, d, p, q, dp, dq, qinv));
        KeyPair pair = new KeyPair(pub, priv);

        // A fixed 16-byte AES key, in place of Crypt.generateSecretKey().
        byte[] secretBytes = new byte[16];
        for (int i = 0; i < 16; i++) {
            secretBytes[i] = (byte) (i * 17 + 3);
        }
        SecretKey secret = new SecretKeySpec(secretBytes, "AES");

        // The four-byte challenge, as ServerLoginPacketListenerImpl builds it:
        // Ints.toByteArray(RandomSource.create().nextInt()) — a big-endian int.
        byte[] challenge = new byte[] {0x12, 0x34, (byte) 0xab, (byte) 0xcd};

        out.println("{");
        // Decimal for a reader, and fixed-width big-endian hex for a
        // comparator — a consumer that has to parse base 10 to check a
        // modulus is a consumer with a second arithmetic implementation in
        // it, and that one has no oracle at all.
        out.printf("  \"modulus\": \"%s\",%n", n);
        out.printf("  \"modulusHex\": \"%s\",%n", hex(toFixed(n, 128)));
        out.printf("  \"publicExponent\": \"%s\",%n", e);
        out.printf("  \"publicExponentHex\": \"%s\",%n", hex(toFixed(e, 3)));
        out.printf("  \"privateExponent\": \"%s\",%n", d);
        out.printf("  \"privateExponentHex\": \"%s\",%n", hex(toFixed(d, 128)));
        out.printf("  \"p\": \"%s\",%n", p);
        out.printf("  \"q\": \"%s\",%n", q);
        out.printf("  \"secretKey\": \"%s\",%n", hex(secretBytes));
        out.printf("  \"challenge\": \"%s\",%n", hex(challenge));

        // The X.509 SubjectPublicKeyInfo the ClientboundHelloPacket carries.
        // ServerLoginPacketListenerImpl sends `getKeyPair().getPublic().getEncoded()`
        // verbatim, so this is the exact byte string a client receives.
        out.printf("  \"publicKeyEncoded\": \"%s\",%n", hex(pub.getEncoded()));
        out.printf("  \"publicKeyFormat\": \"%s\",%n", pub.getFormat());
        out.printf("  \"privateKeyFormat\": \"%s\",%n", priv.getFormat());
        out.printf("  \"privateKeyEncoded\": \"%s\",%n", hex(priv.getEncoded()));

        // The server hash, exactly as handleKey computes it:
        //   new BigInteger(Crypt.digestData("", publicKey, secret)).toString(16)
        // A SIGNED BigInteger, so a digest whose first byte has the high bit
        // set produces a hash with a leading '-'. The empty string is
        // ServerLoginPacketListenerImpl's `serverId` field, which is a
        // constant "" — the value is not configurable and is not the server's
        // name.
        byte[] digest = Crypt.digestData("", pair.getPublic(), secret);
        out.printf("  \"digest\": \"%s\",%n", hex(digest));
        out.printf("  \"serverHash\": \"%s\",%n", new BigInteger(digest).toString(16));

        // A second one whose digest is negative, so the sign is exercised. The
        // server id is not "" here, which is the only way to move the digest
        // without changing the key: it is a capture of digestData, not of the
        // handshake.
        for (String id : new String[] {"", "a", "b", "notch", "jeb_", "-"}) {
            byte[] dg = Crypt.digestData(id, pair.getPublic(), secret);
            out.printf(
                    "  \"hash_%s\": \"%s\",%n",
                    id.isEmpty() ? "empty" : id.replace("-", "dash"),
                    new BigInteger(dg).toString(16));
        }

        // RSA. `Crypt.encryptUsingKey(publicKey, bytes)` is what the CLIENT
        // does to the shared secret and to the challenge; the server undoes
        // both. PKCS#1 v1.5 padding is randomised, so the ciphertext is not
        // reproducible — what is captured instead is a fixed ciphertext
        // produced ONCE below with a deterministic padding, plus the
        // round-trip result, and the shell script asserts the round trip
        // rather than the bytes.
        byte[] encSecret = Crypt.encryptUsingKey(pub, secretBytes);
        byte[] encChallenge = Crypt.encryptUsingKey(pub, challenge);
        out.printf(
                "  \"decryptedSecret\": \"%s\",%n",
                hex(Crypt.decryptUsingKey(priv, encSecret)));
        out.printf(
                "  \"decryptedChallenge\": \"%s\",%n",
                hex(Crypt.decryptUsingKey(priv, encChallenge)));

        // A DETERMINISTIC ciphertext, so the Rust side has something fixed to
        // decrypt. Raw RSA on a hand-built PKCS#1 v1.5 block: 0x00 0x02, then
        // 117 non-zero pad bytes, 0x00, then the 16-byte secret. The padding
        // is fixed rather than random on purpose — the point is a ciphertext
        // this file can contain, and every decryptor must strip the same
        // block.
        byte[] block = new byte[128];
        block[0] = 0x00;
        block[1] = 0x02;
        for (int i = 2; i < 128 - 16 - 1; i++) {
            block[i] = (byte) (0x40 + (i % 60));
        }
        block[128 - 16 - 1] = 0x00;
        System.arraycopy(secretBytes, 0, block, 128 - 16, 16);
        BigInteger c = new BigInteger(1, block).modPow(e, n);
        byte[] fixed = toFixed(c, 128);
        out.printf("  \"fixedCiphertext\": \"%s\",%n", hex(fixed));
        // And it must decrypt through the JCE to the secret, or the hand-built
        // block is not a valid PKCS#1 block and the Rust side would be checked
        // against a fiction.
        byte[] back = Crypt.decryptUsingKey(priv, fixed);
        if (!java.util.Arrays.equals(back, secretBytes)) {
            throw new IllegalStateException("the hand-built PKCS#1 block does not decrypt");
        }

        // AES/CFB8. Two ciphers, encrypt and decrypt, both keyed with the
        // secret and both using the secret AS THE IV — Crypt.getCipher passes
        // `new IvParameterSpec(key.getEncoded())`.
        Cipher enc = Crypt.getCipher(Cipher.ENCRYPT_MODE, secret);
        Cipher dec = Crypt.getCipher(Cipher.DECRYPT_MODE, secret);
        // A stream, not a block: CFB8 is a self-synchronising byte stream, so
        // the cipher's state advances across calls and splitting the input
        // must not change the output. Both are captured.
        byte[] plain = new byte[64];
        for (int i = 0; i < plain.length; i++) {
            plain[i] = (byte) (i * 7);
        }
        byte[] whole = enc.update(plain);
        out.printf("  \"cfb8Plain\": \"%s\",%n", hex(plain));
        out.printf("  \"cfb8Cipher\": \"%s\",%n", hex(whole));
        out.printf("  \"cfb8Decrypted\": \"%s\",%n", hex(dec.update(whole)));

        // The same stream fed in three pieces, from a fresh cipher.
        Cipher split = Crypt.getCipher(Cipher.ENCRYPT_MODE, secret);
        byte[] a = split.update(plain, 0, 1);
        byte[] b = split.update(plain, 1, 16);
        byte[] cc = split.update(plain, 17, plain.length - 17);
        byte[] joined = new byte[a.length + b.length + cc.length];
        System.arraycopy(a, 0, joined, 0, a.length);
        System.arraycopy(b, 0, joined, a.length, b.length);
        System.arraycopy(cc, 0, joined, a.length + b.length, cc.length);
        out.printf("  \"cfb8CipherSplit\": \"%s\",%n", hex(joined));

        // One raw AES-128 block encryption, which is the primitive CFB8 is
        // built on. Captured separately so a failure can be localised: a wrong
        // key schedule shows up here, a wrong feedback rule only above.
        Cipher ecb = Cipher.getInstance("AES/ECB/NoPadding");
        ecb.init(Cipher.ENCRYPT_MODE, secret);
        byte[] blockIn = new byte[16];
        for (int i = 0; i < 16; i++) {
            blockIn[i] = (byte) (0xF0 - i * 9);
        }
        out.printf("  \"aesBlockIn\": \"%s\",%n", hex(blockIn));
        out.printf("  \"aesBlockOut\": \"%s\",%n", hex(ecb.doFinal(blockIn)));

        // SHA-1 on its own, for the same reason.
        java.security.MessageDigest sha = java.security.MessageDigest.getInstance("SHA-1");
        out.printf("  \"sha1Empty\": \"%s\",%n", hex(sha.digest(new byte[0])));
        out.printf(
                "  \"sha1Abc\": \"%s\",%n",
                hex(java.security.MessageDigest.getInstance("SHA-1").digest("abc".getBytes("ISO_8859_1"))));
        byte[] long55 = new byte[55];
        byte[] long56 = new byte[56];
        byte[] long119 = new byte[119];
        for (int i = 0; i < long119.length; i++) {
            long119[i] = (byte) i;
            if (i < 55) {
                long55[i] = (byte) i;
            }
            if (i < 56) {
                long56[i] = (byte) i;
            }
        }
        // 55 and 56 bytes bracket the one-block/two-block boundary, which is
        // where a padding bug lives and nowhere else.
        out.printf("  \"sha1Len55\": \"%s\",%n", hex(digestOf(long55)));
        out.printf("  \"sha1Len56\": \"%s\",%n", hex(digestOf(long56)));
        out.printf("  \"sha1Len119\": \"%s\",%n", hex(digestOf(long119)));

        // The two login packets, encoded by their own STREAM_CODEC.
        //
        // `ClientboundHelloPacket` is what handleHello sends when
        // usesAuthentication() is true: the constant "" server id, the X.509
        // public key, the four-byte challenge, and `shouldAuthenticate`, which
        // handleHello passes as `iconst_1` — always true.
        FriendlyByteBuf b1 = new FriendlyByteBuf(Unpooled.buffer());
        ClientboundHelloPacket.STREAM_CODEC.encode(
                b1, new ClientboundHelloPacket("", pub.getEncoded(), challenge, true));
        out.printf("  \"clientboundHello\": \"%s\",%n", hex(drain(b1)));

        // `ServerboundKeyPacket(secret, publicKey, challenge)` RSA-encrypts
        // both fields, so its bytes are not reproducible. What is captured is
        // a packet built from the FIXED ciphertext instead — the same
        // two-length-prefixed-byte-arrays body, with something a decoder can
        // check itself against.
        FriendlyByteBuf b2 = new FriendlyByteBuf(Unpooled.buffer());
        b2.writeByteArray(fixed);
        b2.writeByteArray(fixed);
        out.printf("  \"serverboundKeyBody\": \"%s\",%n", hex(drain(b2)));

        // And one real one, to prove the two-byte-array shape is what the
        // packet's own codec writes rather than what this file assumed.
        FriendlyByteBuf b3 = new FriendlyByteBuf(Unpooled.buffer());
        ServerboundKeyPacket real = new ServerboundKeyPacket(secret, pub, challenge);
        ServerboundKeyPacket.STREAM_CODEC.encode(b3, real);
        byte[] realBytes = drain(b3);
        out.printf("  \"serverboundKeyLength\": %d,%n", realBytes.length);
        out.printf("  \"serverboundKeyChallengeValid\": %b,%n", real.isChallengeValid(challenge, priv));
        out.printf(
                "  \"serverboundKeyWrongChallengeValid\": %b,%n",
                real.isChallengeValid(new byte[] {0, 0, 0, 0}, priv));
        out.printf("  \"serverboundKeySecret\": \"%s\"%n", hex(real.getSecretKey(priv).getEncoded()));
        out.println("}");
    }

    private static byte[] drain(FriendlyByteBuf b) {
        byte[] out = new byte[b.readableBytes()];
        b.readBytes(out);
        return out;
    }

    private static byte[] digestOf(byte[] b) throws Exception {
        return java.security.MessageDigest.getInstance("SHA-1").digest(b);
    }

    private static byte[] toFixed(BigInteger v, int len) {
        byte[] raw = v.toByteArray();
        byte[] out = new byte[len];
        if (raw.length <= len) {
            System.arraycopy(raw, 0, out, len - raw.length, raw.length);
        } else {
            System.arraycopy(raw, raw.length - len, out, 0, len);
        }
        return out;
    }

    private static String hex(byte[] b) {
        StringBuilder sb = new StringBuilder(b.length * 2);
        for (byte x : b) {
            sb.append(String.format("%02x", x));
        }
        return sb.toString();
    }
}
