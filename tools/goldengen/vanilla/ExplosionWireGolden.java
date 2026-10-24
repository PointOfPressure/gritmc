// ExplosionWireGolden — the bytes vanilla's own codec writes for
// `ClientboundExplodePacket`.
//
// Not a reading of the codec composition: the codec is run.
// `ClientboundExplodePacket.STREAM_CODEC.encode(buf, packet)` on a real
// `RegistryFriendlyByteBuf`, and the buffer's bytes come out as hex. So the
// field order, the varint-versus-fixed-int choice for each field, the
// `Holder` `+1`, the `Optional` prefix and the `WeightedList` framing are all
// recorded rather than inferred.
//
// # The packet changed shape in 26.2 and a reader of the 1.21 one will be wrong
//
// ```text
// ClientboundExplodePacket(Vec3 center, float radius, int blockCount,
//                          Optional<Vec3> playerKnockback,
//                          ParticleOptions explosionParticle,
//                          Holder<SoundEvent> explosionSound,
//                          WeightedList<ExplosionParticleInfo> blockParticles)
// ```
//
// `radius`, `blockCount` and `blockParticles` are all new. `blockCount` is the
// value `ServerExplosion.explode()` returns, which is the size of the
// candidate position list **before** `interactWithBlocks` removes anything —
// not the number of blocks that broke.
//
// `blockCount` goes through `ByteBufCodecs.INT`, which is
// `ByteBuf.writeInt` — four fixed big-endian bytes, not a varint. That is the
// same trap `crate::interact`'s `level_event` documents, and it is worth a
// golden on its own: a varint `1120` and a fixed-int `1120` are different
// lengths and the client would resynchronise on the wrong byte.
//
// # The cases, and what each one is here to pin
//
//   small_no_knockback   `ServerExplosion.isSmall()` true — radius < 2.0f or
//                        the interaction is KEEP — so `ServerLevel.explode`
//                        picks `ParticleTypes.EXPLOSION` (offsets 163-178).
//                        No player knockback: the `Optional` is empty and this
//                        pins its one-byte prefix.
//   large_with_knockback the other branch, `ParticleTypes.EXPLOSION_EMITTER`,
//                        with a knockback vector — so the `Optional`'s present
//                        form and the three doubles after it.
//   tnt_default          TNT's own numbers: radius 4.0f, and a `blockCount`
//                        taken from `ci/explosion-ray-golden.sh`'s
//                        `open_air_r4_s1` so the two goldens agree on one
//                        figure.
//   zero_blocks          blockCount 0, which is what an obsidian-walled blast
//                        sends, and the case where a varint and a fixed int
//                        happen to agree in length — recorded so that the
//                        others' disagreement is the evidence.
//   empty_block_particles  `WeightedList.of()`, to pin the list's own count
//                        prefix independently of its entries.
//
// Every case carries `DEFAULT_EXPLOSION_BLOCK_PARTICLES` unless it says
// otherwise: `Level.<clinit>` offsets 54-92 build it as
// `POOF` at scaling 0.5f speed 1.0f, then `SMOKE` at 1.0f/1.0f, both at
// `Builder.add(E)`'s implicit weight of 1.
//
// Converted to JSON on stdout; `ci/explosion-wire-golden.sh` checks it.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;

import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.particles.ExplosionParticleInfo;
import net.minecraft.core.particles.ParticleOptions;
import net.minecraft.core.particles.ParticleTypes;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundExplodePacket;
import net.minecraft.server.Bootstrap;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundEvents;
import net.minecraft.util.random.WeightedList;
import net.minecraft.world.phys.Vec3;

public final class ExplosionWireGolden {
    private ExplosionWireGolden() {}

    private static String q(String s) {
        return "\"" + s + "\"";
    }

    private static String hex(ByteBuf buf) {
        StringBuilder b = new StringBuilder();
        for (int i = 0; i < buf.writerIndex(); i++) {
            b.append(String.format("%02x", buf.getByte(i)));
        }
        return b.toString();
    }

    private static RegistryAccess.Frozen access;

    private static String run(String name, ClientboundExplodePacket p) {
        RegistryFriendlyByteBuf buf =
                new RegistryFriendlyByteBuf(Unpooled.buffer(), access);
        ClientboundExplodePacket.STREAM_CODEC.encode(buf, p);
        return "    {" + q("name") + ": " + q(name)
                + ", " + q("bytes") + ": " + q(hex(buf))
                + ", " + q("length") + ": " + buf.writerIndex()
                + ", " + q("centerBits") + ": [" + Double.doubleToRawLongBits(p.center().x)
                + ", " + Double.doubleToRawLongBits(p.center().y)
                + ", " + Double.doubleToRawLongBits(p.center().z) + "]"
                + ", " + q("radiusBits") + ": " + Float.floatToRawIntBits(p.radius())
                + ", " + q("blockCount") + ": " + p.blockCount()
                + ", " + q("knockbackBits") + ": "
                + p.playerKnockback()
                        .map(v -> "[" + Double.doubleToRawLongBits(v.x) + ", "
                                + Double.doubleToRawLongBits(v.y) + ", "
                                + Double.doubleToRawLongBits(v.z) + "]")
                        .orElse("null")
                + ", " + q("particle") + ": "
                + q(BuiltInRegistries.PARTICLE_TYPE.getKey(p.explosionParticle().getType())
                        .toString())
                + ", " + q("sound") + ": " + q(p.explosionSound().value().location().toString())
                + ", " + q("blockParticleCount") + ": " + p.blockParticles().unwrap().size()
                + "}";
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        access = RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);

        Holder<SoundEvent> sound = SoundEvents.GENERIC_EXPLODE;
        // `Level.DEFAULT_EXPLOSION_BLOCK_PARTICLES`, rebuilt here from the same
        // two constructor calls the `<clinit>` makes, because the field is
        // private and its value is the point of the record rather than an
        // input to it — the assertion that these are the right two is the
        // `blockParticleCount` and the recorded bytes.
        WeightedList<ExplosionParticleInfo> defaults = WeightedList
                .<ExplosionParticleInfo>builder()
                .add(new ExplosionParticleInfo(ParticleTypes.POOF, 0.5F, 1.0F))
                .add(new ExplosionParticleInfo(ParticleTypes.SMOKE, 1.0F, 1.0F))
                .build();
        WeightedList<ExplosionParticleInfo> none =
                WeightedList.<ExplosionParticleInfo>builder().build();

        ParticleOptions small = ParticleTypes.EXPLOSION;
        ParticleOptions large = ParticleTypes.EXPLOSION_EMITTER;

        List<String> cases = new ArrayList<>();
        cases.add(run("small_no_knockback", new ClientboundExplodePacket(
                new Vec3(0.5, 65.5, 0.5), 1.5F, 3, Optional.empty(), small, sound, defaults)));
        cases.add(run("large_with_knockback", new ClientboundExplodePacket(
                new Vec3(0.5, 65.5, 0.5), 4.0F, 780,
                Optional.of(new Vec3(0.25, -0.5, 1.5)), large, sound, defaults)));
        cases.add(run("tnt_default", new ClientboundExplodePacket(
                new Vec3(0.5, 65.5, 0.5), 4.0F, 1120, Optional.empty(), large, sound, defaults)));
        cases.add(run("zero_blocks", new ClientboundExplodePacket(
                new Vec3(-7.5, 64.0625, -3.25), 4.0F, 0, Optional.empty(), large, sound,
                defaults)));
        cases.add(run("empty_block_particles", new ClientboundExplodePacket(
                new Vec3(0.5, 65.5, 0.5), 4.0F, 1120, Optional.empty(), large, sound, none)));

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("packetId")).append(": ")
                .append("36").append(",\n");
        out.append("  ").append(q("soundId")).append(": ")
                .append(BuiltInRegistries.SOUND_EVENT.getId(SoundEvents.GENERIC_EXPLODE.value()))
                .append(",\n");
        out.append("  ").append(q("particleIds")).append(": {");
        out.append(q("minecraft:explosion")).append(": ")
                .append(BuiltInRegistries.PARTICLE_TYPE.getId(ParticleTypes.EXPLOSION));
        out.append(", ").append(q("minecraft:explosion_emitter")).append(": ")
                .append(BuiltInRegistries.PARTICLE_TYPE.getId(ParticleTypes.EXPLOSION_EMITTER));
        out.append(", ").append(q("minecraft:poof")).append(": ")
                .append(BuiltInRegistries.PARTICLE_TYPE.getId(ParticleTypes.POOF));
        out.append(", ").append(q("minecraft:smoke")).append(": ")
                .append(BuiltInRegistries.PARTICLE_TYPE.getId(ParticleTypes.SMOKE));
        out.append("},\n");
        out.append("  ").append(q("cases")).append(": [\n");
        out.append(String.join(",\n", cases));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }
}
