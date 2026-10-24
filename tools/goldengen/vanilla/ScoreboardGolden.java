package net.minecraft.world.level.levelgen;

import io.netty.buffer.Unpooled;
import java.io.PrintStream;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.core.RegistryAccess;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.chat.Component;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.game.ClientboundResetScorePacket;
import net.minecraft.network.protocol.game.ClientboundSetDisplayObjectivePacket;
import net.minecraft.network.protocol.game.ClientboundSetObjectivePacket;
import net.minecraft.network.protocol.game.ClientboundSetPlayerTeamPacket;
import net.minecraft.network.protocol.game.ClientboundSetScorePacket;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.scores.DisplaySlot;
import net.minecraft.world.scores.Objective;
import net.minecraft.world.scores.PlayerTeam;
import net.minecraft.world.scores.Scoreboard;
import net.minecraft.world.scores.Team;
import net.minecraft.world.scores.TeamColor;
import net.minecraft.world.scores.criteria.ObjectiveCriteria;

/**
 * The scoreboard's wire surface: the five clientbound packets, and the
 * numeric ids of every enum they carry.
 *
 * <p>The enum tables are the point as much as the bytes are. `DisplaySlot`,
 * `TeamColor`, `Team.Visibility` and `Team.CollisionRule` all reach the wire
 * through `ByteBufCodecs.idMapper`, which writes a varint of an `id` **field**
 * — a number assigned in the enum constructor, not the declaration ordinal.
 * A transcription that assumes ordinal is right only for as long as the two
 * happen to coincide, so this dumps the `id` vanilla actually writes and the
 * Rust constants are checked against it.
 *
 * <p>`ObjectiveCriteria.RenderType` is the exception and it is written down
 * because it looks like the others: it goes through `FriendlyByteBuf.writeEnum`,
 * which **is** the ordinal.
 *
 * <p>Every component here is a plain `Component.literal`, which serialises as
 * a bare `TAG_String`. Styled components are deliberately absent: their NBT is
 * the chat layer's problem and putting one here would make this capture fail
 * for a reason that has nothing to do with the scoreboard.
 *
 * <p>Run by {@code ci/scoreboard-golden.sh}. Writes to
 * {@code crates/rustmc-server/golden/scoreboard.json}.
 */
public final class ScoreboardGolden {
    private ScoreboardGolden() {}

    public static void main(String[] args) {
        // Captured BEFORE the bootstrap, which installs a log4j redirect over
        // `System.out` that stamps every line with a timestamp. Written
        // afterwards, this dump is different on every run and the
        // two-runs-must-agree check in `ci/scoreboard-golden.sh` catches it —
        // which is what that check is for.
        PrintStream out = System.out;
        // Without these two the first Component encode dies in
        // `BuiltInRegistries.<clinit>` with "Not bootstrapped", six frames
        // deep inside a chat codec — a stack that names nothing about the
        // scoreboard.
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        Scoreboard sb = new Scoreboard();

        List<String> packets = new ArrayList<>();

        // --- objectives -----------------------------------------------------
        // Three methods and two render types, so neither the method byte nor
        // the render-type varint is a constant across the capture.
        Objective plain = new Objective(
                sb,
                "kills",
                ObjectiveCriteria.DUMMY,
                Component.literal("Kills"),
                ObjectiveCriteria.RenderType.INTEGER,
                false,
                null);
        Objective hearts = new Objective(
                sb,
                "hp",
                ObjectiveCriteria.HEALTH,
                Component.literal("HP"),
                ObjectiveCriteria.RenderType.HEARTS,
                true,
                null);
        packets.add(pkt(
                "set_objective/add/kills",
                ClientboundSetObjectivePacket.STREAM_CODEC,
                new ClientboundSetObjectivePacket(plain, ClientboundSetObjectivePacket.METHOD_ADD)));
        packets.add(pkt(
                "set_objective/change/kills",
                ClientboundSetObjectivePacket.STREAM_CODEC,
                new ClientboundSetObjectivePacket(plain, ClientboundSetObjectivePacket.METHOD_CHANGE)));
        // METHOD_REMOVE stops after the method byte — the display name, the
        // render type and the number format are all inside the `if`.
        packets.add(pkt(
                "set_objective/remove/kills",
                ClientboundSetObjectivePacket.STREAM_CODEC,
                new ClientboundSetObjectivePacket(plain, ClientboundSetObjectivePacket.METHOD_REMOVE)));
        packets.add(pkt(
                "set_objective/add/hp-hearts",
                ClientboundSetObjectivePacket.STREAM_CODEC,
                new ClientboundSetObjectivePacket(hearts, ClientboundSetObjectivePacket.METHOD_ADD)));

        // --- scores ---------------------------------------------------------
        // A negative score, because the field is a varint and a varint of a
        // negative int is five bytes: a transcription that reaches for a
        // zigzag encoding gets two here and passes every non-negative case.
        packets.add(pkt(
                "set_score/positive",
                ClientboundSetScorePacket.STREAM_CODEC,
                new ClientboundSetScorePacket("oracle", "kills", 7, Optional.empty(), Optional.empty())));
        packets.add(pkt(
                "set_score/negative",
                ClientboundSetScorePacket.STREAM_CODEC,
                new ClientboundSetScorePacket("oracle", "kills", -3, Optional.empty(), Optional.empty())));
        packets.add(pkt(
                "set_score/with-display",
                ClientboundSetScorePacket.STREAM_CODEC,
                new ClientboundSetScorePacket(
                        "oracle", "kills", 0, Optional.of(Component.literal("seven")), Optional.empty())));
        packets.add(pkt(
                "reset_score/one-objective",
                ClientboundResetScorePacket.STREAM_CODEC,
                new ClientboundResetScorePacket("oracle", "kills")));
        // `objectiveName` is nullable in reset_score — `writeNullable`, so the
        // absent case is one `false` byte and nothing else.
        packets.add(pkt(
                "reset_score/all-objectives",
                ClientboundResetScorePacket.STREAM_CODEC,
                new ClientboundResetScorePacket("oracle", null)));

        // --- display slots --------------------------------------------------
        // The first, a mid-table one and the last, so an off-by-one in the id
        // table cannot pass.
        for (DisplaySlot slot : new DisplaySlot[] {DisplaySlot.LIST, DisplaySlot.SIDEBAR, DisplaySlot.TEAM_WHITE}) {
            packets.add(pkt(
                    "set_display_objective/" + slot.getSerializedName(),
                    ClientboundSetDisplayObjectivePacket.STREAM_CODEC,
                    new ClientboundSetDisplayObjectivePacket(slot, plain)));
        }

        // --- teams ----------------------------------------------------------
        PlayerTeam team = new PlayerTeam(sb, "reds");
        team.setDisplayName(Component.literal("The Reds"));
        team.setPlayerPrefix(Component.literal("["));
        team.setPlayerSuffix(Component.literal("]"));
        team.setColor(Optional.of(TeamColor.RED));
        team.setAllowFriendlyFire(false);
        team.setSeeFriendlyInvisibles(true);
        team.setNameTagVisibility(Team.Visibility.HIDE_FOR_OTHER_TEAMS);
        team.setCollisionRule(Team.CollisionRule.PUSH_OWN_TEAM);
        team.getPlayers().add("oracle");
        team.getPlayers().add("subject");

        packets.add(pkt(
                "set_player_team/create",
                ClientboundSetPlayerTeamPacket.STREAM_CODEC,
                ClientboundSetPlayerTeamPacket.createAddOrModifyPacket(team, true)));
        packets.add(pkt(
                "set_player_team/update",
                ClientboundSetPlayerTeamPacket.STREAM_CODEC,
                ClientboundSetPlayerTeamPacket.createAddOrModifyPacket(team, false)));
        packets.add(pkt(
                "set_player_team/remove",
                ClientboundSetPlayerTeamPacket.STREAM_CODEC,
                ClientboundSetPlayerTeamPacket.createRemovePacket(team)));
        packets.add(pkt(
                "set_player_team/join",
                ClientboundSetPlayerTeamPacket.STREAM_CODEC,
                ClientboundSetPlayerTeamPacket.createPlayerPacket(
                        team, "oracle", ClientboundSetPlayerTeamPacket.Action.ADD)));
        packets.add(pkt(
                "set_player_team/leave",
                ClientboundSetPlayerTeamPacket.STREAM_CODEC,
                ClientboundSetPlayerTeamPacket.createPlayerPacket(
                        team, "oracle", ClientboundSetPlayerTeamPacket.Action.REMOVE)));

        // A second team with the *other* value of every flag, so the options
        // bitfield is not one constant.
        PlayerTeam plainTeam = new PlayerTeam(sb, "greys");
        plainTeam.setAllowFriendlyFire(true);
        plainTeam.setSeeFriendlyInvisibles(false);
        packets.add(pkt(
                "set_player_team/create-defaults",
                ClientboundSetPlayerTeamPacket.STREAM_CODEC,
                ClientboundSetPlayerTeamPacket.createAddOrModifyPacket(plainTeam, true)));

        out.println("{");
        out.println("  \"source\": \"26.2 bytecode; packets encoded by their own STREAM_CODEC\",");
        out.printf(
                "  \"methodAdd\": %d, \"methodRemove\": %d, \"methodChange\": %d,%n",
                ClientboundSetObjectivePacket.METHOD_ADD,
                ClientboundSetObjectivePacket.METHOD_REMOVE,
                ClientboundSetObjectivePacket.METHOD_CHANGE);
        out.println("  \"displaySlots\": {");
        DisplaySlot[] slots = DisplaySlot.values();
        for (int i = 0; i < slots.length; i++) {
            out.printf(
                    "    \"%s\": %d%s%n",
                    slots[i].getSerializedName(), slots[i].id(), i + 1 < slots.length ? "," : "");
        }
        out.println("  },");
        out.println("  \"teamColors\": {");
        TeamColor[] colors = TeamColor.values();
        for (int i = 0; i < colors.length; i++) {
            out.printf(
                    "    \"%s\": %d%s%n",
                    colors[i].getSerializedName(), colorId(colors[i]), i + 1 < colors.length ? "," : "");
        }
        out.println("  },");
        out.println("  \"visibility\": {");
        Team.Visibility[] vis = Team.Visibility.values();
        for (int i = 0; i < vis.length; i++) {
            out.printf("    \"%s\": %d%s%n", vis[i].getSerializedName(), vis[i].id, i + 1 < vis.length ? "," : "");
        }
        out.println("  },");
        out.println("  \"collisionRules\": {");
        Team.CollisionRule[] cr = Team.CollisionRule.values();
        for (int i = 0; i < cr.length; i++) {
            out.printf("    \"%s\": %d%s%n", cr[i].getSerializedName(), cr[i].id, i + 1 < cr.length ? "," : "");
        }
        out.println("  },");
        out.println("  \"renderTypes\": {");
        ObjectiveCriteria.RenderType[] rt = ObjectiveCriteria.RenderType.values();
        for (int i = 0; i < rt.length; i++) {
            // `writeEnum` writes the ORDINAL, not an id field. Dumped as the
            // ordinal on purpose so the difference from every other enum here
            // is visible in the file rather than only in a comment.
            out.printf("    \"%s\": %d%s%n", rt[i].getSerializedName(), rt[i].ordinal(), i + 1 < rt.length ? "," : "");
        }
        out.println("  },");
        out.println("  \"packets\": {");
        for (int i = 0; i < packets.size(); i++) {
            out.print("    " + packets.get(i));
            out.println(i + 1 < packets.size() ? "," : "");
        }
        out.println("  }");
        out.println("}");
    }

    /**
     * `TeamColor.id` is private, so it is recovered the only way that is
     * guaranteed to be the number the wire carries: by encoding one.
     */
    private static int colorId(TeamColor c) {
        RegistryFriendlyByteBuf buf = buf();
        TeamColor.STREAM_CODEC.encode(buf, c);
        return buf.readVarInt();
    }

    private static RegistryFriendlyByteBuf buf() {
        return new RegistryFriendlyByteBuf(Unpooled.buffer(), RegistryAccess.EMPTY);
    }

    private static <T> String pkt(String name, StreamCodec<? super RegistryFriendlyByteBuf, T> codec, T packet) {
        RegistryFriendlyByteBuf b = buf();
        codec.encode(b, packet);
        byte[] raw = new byte[b.readableBytes()];
        b.readBytes(raw);
        StringBuilder sb = new StringBuilder();
        for (byte x : raw) {
            sb.append(String.format("%02x", x));
        }
        return "\"" + name + "\": \"" + sb + "\"";
    }
}
