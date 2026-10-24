//! `PlayerAdvancements` — one player's progress, what the client is allowed
//! to see, and the `ClientboundUpdateAdvancementsPacket` that tells it.
//!
//! Transcribed from `javap -p -c -constants` on
//! `net.minecraft.server.PlayerAdvancements` and
//! `net.minecraft.server.advancements.AdvancementVisibilityEvaluator` in the
//! pinned 26.2 jar. Every offset quoted below is from those two listings.
//!
//! # A BRAND-NEW PLAYER RECEIVES NO `update_advancements` PACKET AT ALL
//!
//! There is no join-time send. `flushDirty(this, true)` is the last statement
//! of `ServerPlayer.tick()` (offsets 186-195, `iconst_1` is
//! `showAdvancements`), so it runs every tick and offset 28's early exit is
//! what stops a packet on almost all of them.
//!
//! `markForVisibilityUpdate` is reached from exactly three places —
//! `award` (offset 102), `revoke` (offset 58) and the `applyFrom` lambda that
//! replays a saved `advancements/<uuid>.json` (offset 51). With no save file
//! `rootsToUpdate` is empty on the first tick, `updateTreeVisibility` never
//! runs, and offsets 197-223 then refuse to send an empty packet even though
//! `isFirstPacket` got past offset 28: `map.isEmpty() && added.isEmpty() &&
//! removed.isEmpty()` jumps straight to offset 250, which clears the flag.
//!
//! So **the tree does not reach the client on join, and neither does a reset
//! packet.** The client hears nothing until the player's first advancement in
//! a tree completes, when `updateTreeVisibility` walks that whole root — and
//! `reset=true` is then seen only by a player whose save file marked a root
//! dirty before their first tick.
//!
//! `PlayerAdvancements` is the only class in the jar that constructs this
//! packet (`grep -ral` over every class), and `flushDirty`'s only two callers
//! are `ServerPlayer.tick` and `AdvancementCommands$Action`.
//!
//! # And that first tree packet is large, for a reason worth naming
//!
//! `AdvancementVisibilityEvaluator` calls the predicate — which is
//! `getOrStartProgress(holder).isDone()`, and `getOrStartProgress` **inserts**
//! — on every node it visits, before the output callback runs on that node.
//! So `progress.containsKey(holder)` at offset 40 of
//! `lambda$updateTreeVisibility$1` is true for every node in the tree, and
//! every newly visible advancement lands in `progressChanged` as well as in
//! `added`. One completed criterion therefore produces a packet naming every
//! visible advancement in its tree twice: once in `added` with its display,
//! once in `progress` with its criteria all unmet.
//!
//! # What order everything comes out in
//!
//! `flushDirty` (offsets 31-55) allocates a `HashMap` for the progress map
//! and two `HashSet`s for `added` and `removed`, and `List.copyOf` at the
//! packet constructor freezes the set's own walk. All three are
//! [`super::jhash`]'s order over `Identifier.hashCode`. `rootsToUpdate` and
//! `progressChanged` are `HashSet`s too, so the order the two loops visit
//! them is the same walk — which is why this file tracks an insertion
//! sequence for each rather than a plain set.
//!
//! `progress` itself is a `LinkedHashMap` in vanilla and a `BTreeMap` here:
//! it is only ever read by key (`flushDirty` offset 171, `getOrStartProgress`
//! offset 5) and iterated in one place, `asData()`, which serialises the save
//! file this server does not write.

use super::jhash;
use super::{visibility_rule, Progress, Table, Tree, VisibilityRule};
use std::collections::{BTreeMap, BTreeSet};
use std::io;

/// `AdvancementVisibilityEvaluator.VISIBILITY_DEPTH`, and the number of
/// `NO_CHANGE` sentinels the public entry point pushes before descending
/// (offsets 14-36: `for (i = 0; i <= 2; i++) stack.push(NO_CHANGE)`).
const SENTINELS: usize = 3;

/// One player's advancement state.
#[derive(Debug)]
pub struct PlayerAdvancements<'a> {
    table: &'a Table,
    tree: &'a Tree,
    /// `progress` — see the module header for why a `BTreeMap` is faithful.
    progress: BTreeMap<String, Progress>,
    /// `visible` — a `HashSet`, and only membership is ever asked of it.
    visible: BTreeSet<String>,
    /// `progressChanged`, with the insertion sequence kept so
    /// [`jhash::iteration_order`] can reproduce the `HashSet` walk.
    progress_changed: Vec<String>,
    /// `rootsToUpdate`, same.
    roots_to_update: Vec<String>,
    /// `isFirstPacket` — the packet's `reset` flag, true until the first
    /// `flushDirty` returns.
    first_packet: bool,
    /// Advancements that completed but whose `holder_wire` the extract does
    /// not hold, so they could not be put in `added`. Two exist in vanilla
    /// 26.2 (`adventure/hero_of_the_village`, `adventure/voluntary_exile`).
    unsendable: usize,
}

/// What one [`PlayerAdvancements::award`] earned, for the caller to act on.
///
/// `AdvancementRewards.grant` (offsets in `AdvancementRewards`) does four
/// things: award experience, roll every loot table into the player's
/// inventory, hand the recipes to `ServerRecipeBook.addRecipes`, and run the
/// function. This type reports the first three and **refuses the fourth by
/// name** — a datapack function is a command engine driven by pack text, and
/// a reward that silently did nothing would read as one that worked.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rewards {
    pub experience: i32,
    pub recipes: Vec<String>,
    pub loot: Vec<String>,
    /// The advancements whose reward named a function. Never run.
    pub refused_functions: Vec<String>,
}

impl<'a> PlayerAdvancements<'a> {
    #[must_use]
    pub fn new(table: &'a Table, tree: &'a Tree) -> Self {
        Self {
            table,
            tree,
            progress: BTreeMap::new(),
            visible: BTreeSet::new(),
            progress_changed: Vec::new(),
            roots_to_update: Vec::new(),
            first_packet: true,
            unsendable: 0,
        }
    }

    /// `getOrStartProgress(AdvancementHolder)` — offsets 0-33. Inserts a
    /// fresh `AdvancementProgress`, `update`d against the advancement's
    /// requirements, when there is none. **This mutates**, which is why the
    /// visibility predicate below can turn every node of a tree into a
    /// `progress` key.
    fn get_or_start(&mut self, id: &str) -> &mut Progress {
        if !self.progress.contains_key(id) {
            let mut p = Progress::default();
            if let Some(a) = self.table.get(id) {
                p.update(&a.requirements);
            }
            self.progress.insert(id.to_string(), p);
        }
        self.progress.get_mut(id).expect("just inserted")
    }

    /// Read-only view of a player's progress on one advancement.
    #[must_use]
    pub fn progress_of(&self, id: &str) -> Option<&Progress> {
        self.progress.get(id)
    }

    #[must_use]
    pub fn is_visible(&self, id: &str) -> bool {
        self.visible.contains(id)
    }

    /// How many completed advancements could not be put in a packet because
    /// the extract holds no `holder_wire` for them.
    #[must_use]
    pub fn unsendable(&self) -> usize {
        self.unsendable
    }

    /// `award(AdvancementHolder, String)` — offsets 0-106.
    ///
    /// Returns `(changed, rewards)`. `changed` is offset 41's `flag = true`:
    /// the criterion was known and newly granted. Rewards are granted only on
    /// the transition into `isDone` (offsets 43-53: `wasDone` false and it is
    /// done now), which is also the only path to `markForVisibilityUpdate`
    /// (offsets 87-102) — a second award of an already-complete advancement
    /// pays nothing and moves nothing.
    ///
    /// `unregisterListeners` (offset 27) is not reproduced: this server's
    /// trigger index is [`Table::listeners`], recomputed per fire and
    /// filtered on progress, so there is no per-player listener set to
    /// remove from. The observable difference is work, not answers.
    pub fn award(&mut self, id: &str, criterion: &str, obtained_millis: i64) -> (bool, Rewards) {
        let mut rewards = Rewards::default();
        let Some(a) = self.table.get(id) else {
            return (false, rewards);
        };
        let was_done = self.get_or_start(id).is_done();
        let granted = self
            .progress
            .get_mut(id)
            .expect("get_or_start inserted")
            .grant(criterion, obtained_millis);
        let mut changed = false;
        if granted {
            self.push_progress_changed(id);
            changed = true;
            if !was_done && self.progress[id].is_done() {
                rewards.experience = a.experience;
                rewards.recipes.clone_from(&a.reward_recipes);
                rewards.loot.clone_from(&a.reward_loot);
                if a.reward_function {
                    rewards.refused_functions.push(id.to_string());
                }
            }
        }
        if !was_done && self.progress[id].is_done() {
            self.mark_for_visibility_update(id);
        }
        (changed, rewards)
    }

    /// `revoke(AdvancementHolder, String)` — offsets mirror `award`: the
    /// transition out of `isDone` is what marks the tree dirty.
    pub fn revoke(&mut self, id: &str, criterion: &str) -> bool {
        if self.table.get(id).is_none() {
            return false;
        }
        let was_done = self.get_or_start(id).is_done();
        let revoked = self
            .progress
            .get_mut(id)
            .expect("get_or_start inserted")
            .revoke(criterion);
        if revoked {
            self.push_progress_changed(id);
        }
        if was_done && !self.progress[id].is_done() {
            self.mark_for_visibility_update(id);
        }
        revoked
    }

    /// `markForVisibilityUpdate(AdvancementHolder)` — offsets 0-27:
    /// `rootsToUpdate.add(tree.get(holder).root())`, and nothing at all when
    /// the tree does not hold it.
    fn mark_for_visibility_update(&mut self, id: &str) {
        if let Some(root) = self.tree.root_of(id) {
            let root = root.to_string();
            if !self.roots_to_update.contains(&root) {
                self.roots_to_update.push(root);
            }
        }
    }

    fn push_progress_changed(&mut self, id: &str) {
        if !self.progress_changed.iter().any(|x| x == id) {
            self.progress_changed.push(id.to_string());
        }
    }

    /// `flushDirty(ServerPlayer, boolean)` — offsets 0-255.
    ///
    /// Returns the packet body, or `None` when offset 28's early exit fires
    /// (not the first packet and nothing dirty) or offset 223's does (the
    /// walk produced nothing). `isFirstPacket` is cleared either way, at
    /// offset 250, which is outside both branches.
    ///
    /// # Errors
    /// Only from the byte writer.
    pub fn flush_dirty(&mut self, show_advancements: bool) -> io::Result<Option<Vec<u8>>> {
        if !self.first_packet && self.roots_to_update.is_empty() && self.progress_changed.is_empty()
        {
            self.first_packet = false;
            return Ok(None);
        }
        let mut added: Vec<String> = Vec::new();
        let mut removed: Vec<String> = Vec::new();

        let roots = std::mem::take(&mut self.roots_to_update);
        let refs: Vec<&str> = roots.iter().map(String::as_str).collect();
        for root in jhash::iteration_order(&refs, &jhash::identifier_hash) {
            self.update_tree_visibility(&root, &mut added, &mut removed);
        }

        let changed = std::mem::take(&mut self.progress_changed);
        let crefs: Vec<&str> = changed.iter().map(String::as_str).collect();
        let mut map_keys: Vec<String> = Vec::new();
        for holder in jhash::iteration_order(&crefs, &jhash::identifier_hash) {
            if self.visible.contains(&holder) {
                map_keys.push(holder);
            }
        }

        let reset = self.first_packet;
        self.first_packet = false;
        if map_keys.is_empty() && added.is_empty() && removed.is_empty() {
            return Ok(None);
        }

        let mut body = Vec::new();
        rustmc_proto::wire::write_bool(&mut body, reset)?;

        // `AdvancementHolder.LIST_STREAM_CODEC` — a `VarInt` count then each
        // holder's own bytes. An advancement the extract could not encode is
        // left out and counted; sending a truncated holder would corrupt the
        // stream for everything after it.
        let arefs: Vec<&str> = added.iter().map(String::as_str).collect();
        let add_order = jhash::iteration_order(&arefs, &jhash::identifier_hash);
        let sendable: Vec<&[u8]> = add_order
            .iter()
            .filter_map(|id| self.table.get(id).and_then(|a| a.holder_wire.as_deref()))
            .collect();
        self.unsendable += add_order.len() - sendable.len();
        rustmc_proto::varint::write_varint(&mut body, i32::try_from(sendable.len()).unwrap_or(0))?;
        for w in sendable {
            body.extend_from_slice(w);
        }

        let rrefs: Vec<&str> = removed.iter().map(String::as_str).collect();
        let rem_order = jhash::iteration_order(&rrefs, &jhash::identifier_hash);
        rustmc_proto::varint::write_varint(&mut body, i32::try_from(rem_order.len()).unwrap_or(0))?;
        for id in &rem_order {
            rustmc_proto::wire::write_string(&mut body, id)?;
        }

        let mrefs: Vec<&str> = map_keys.iter().map(String::as_str).collect();
        let map_order = jhash::iteration_order(&mrefs, &jhash::identifier_hash);
        rustmc_proto::varint::write_varint(&mut body, i32::try_from(map_order.len()).unwrap_or(0))?;
        for id in &map_order {
            rustmc_proto::wire::write_string(&mut body, id)?;
            self.progress[id].write(&mut body)?;
        }

        rustmc_proto::wire::write_bool(&mut body, show_advancements)?;
        Ok(Some(body))
    }

    /// `updateTreeVisibility(AdvancementNode, Set, Set)` — one call into
    /// `AdvancementVisibilityEvaluator.evaluateVisibility` with
    /// `getOrStartProgress(...).isDone()` as the predicate and
    /// `lambda$updateTreeVisibility$1` as the output.
    fn update_tree_visibility(
        &mut self,
        root: &str,
        added: &mut Vec<String>,
        removed: &mut Vec<String>,
    ) {
        // The recursion needs `&mut self` for the predicate (which inserts)
        // and for the output (which moves ids between sets). Splitting it
        // into a walk that returns the accepts, then applying them, keeps
        // vanilla's ORDER — accepts come out post-order, exactly as
        // `out.accept` at offset 111 fires after the children — while
        // staying inside one borrow.
        let mut accepts: Vec<(String, bool)> = Vec::new();
        let mut stack = vec![VisibilityRule::NoChange; SENTINELS];
        self.eval(root, &mut stack, &mut accepts);
        for (id, visible) in accepts {
            if visible {
                if self.visible.insert(id.clone()) {
                    added.push(id.clone());
                    // Always true here: the predicate ran `getOrStartProgress`
                    // on this node on the way down. Written as vanilla writes
                    // it rather than folded away.
                    if self.progress.contains_key(&id) {
                        self.push_progress_changed(&id);
                    }
                }
            } else if self.visible.remove(&id) {
                removed.push(id);
            }
        }
    }

    /// `evaluateVisibility(node, stack, predicate, output)` — offsets 0-118.
    fn eval(
        &mut self,
        id: &str,
        stack: &mut Vec<VisibilityRule>,
        accepts: &mut Vec<(String, bool)>,
    ) -> bool {
        let done = self.get_or_start(id).is_done();
        let rule = self
            .table
            .get(id)
            .map_or(VisibilityRule::Hide, |a| visibility_rule(a, done));
        let mut any_done = done;
        stack.push(rule);
        let children: Vec<String> = self.tree.children(id).to_vec();
        for c in &children {
            any_done |= self.eval(c, stack, accepts);
        }
        let visible = any_done || super::visible_for_unfinished(stack);
        stack.pop();
        accepts.push((id.to_string(), visible));
        any_done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// story/root, one child, one grandchild, one hidden leaf and one
    /// display-less leaf. The shapes `evaluateVisibilityRule` distinguishes,
    /// in one tree small enough to write the expected packet out by hand.
    fn fixture() -> Table {
        Table::parse(&serde_json::json!({
            "advancements": {
                "t:root": {"parent": null, "requirements": [["a"]],
                    "criteria_triggers": {"a": "minecraft:inventory_changed"},
                    "display": {"hidden": false}, "experience": 0,
                    "reward_recipes": [], "reward_loot": [], "reward_function": false,
                    "holder_wire": "aa"},
                "t:child": {"parent": "t:root", "requirements": [["a"]],
                    "criteria_triggers": {"a": "minecraft:placed_block"},
                    "display": {"hidden": false}, "experience": 7,
                    "reward_recipes": ["t:r"], "reward_loot": ["t:l"],
                    "reward_function": true, "holder_wire": "bb"},
                "t:grandchild": {"parent": "t:child", "requirements": [["a"]],
                    "criteria_triggers": {"a": "minecraft:consume_item"},
                    "display": {"hidden": false}, "experience": 0,
                    "reward_recipes": [], "reward_loot": [], "reward_function": false,
                    "holder_wire": "cc"},
                "t:deep": {"parent": "t:grandchild", "requirements": [["a"]],
                    "criteria_triggers": {"a": "minecraft:tick"},
                    "display": {"hidden": false}, "experience": 0,
                    "reward_recipes": [], "reward_loot": [], "reward_function": false,
                    "holder_wire": "dd"},
                "t:nodisplay": {"parent": "t:root", "requirements": [["a"]],
                    "criteria_triggers": {"a": "minecraft:recipe_unlocked"},
                    "display": null, "experience": 0,
                    "reward_recipes": [], "reward_loot": [], "reward_function": false,
                    "holder_wire": null}
            }
        }))
        .expect("fixture parses")
    }

    fn ids_in(body: &[u8]) -> (bool, usize, Vec<String>, Vec<String>, bool) {
        // reset, |added|, removed ids, progress keys, showAdvancements.
        // `added` is opaque here on purpose: its payload is vanilla's
        // captured holder bytes and this module never claims to parse them.
        let mut c = std::io::Cursor::new(body);
        let reset = rustmc_proto::wire::read_bool(&mut c).expect("reset");
        let n_added = rustmc_proto::varint::read_varint(&mut c).expect("added count");
        // Skip the holder payloads: the fixture's are one byte each.
        for _ in 0..n_added {
            let mut b = [0u8; 1];
            std::io::Read::read_exact(&mut c, &mut b).expect("holder byte");
        }
        let n_removed = rustmc_proto::varint::read_varint(&mut c).expect("removed count");
        let mut removed = Vec::new();
        for _ in 0..n_removed {
            removed.push(rustmc_proto::wire::read_string(&mut c).expect("removed id"));
        }
        let n_prog = rustmc_proto::varint::read_varint(&mut c).expect("progress count");
        let mut keys = Vec::new();
        for _ in 0..n_prog {
            keys.push(rustmc_proto::wire::read_string(&mut c).expect("progress id"));
            let n_crit = rustmc_proto::varint::read_varint(&mut c).expect("criteria count");
            for _ in 0..n_crit {
                let _ = rustmc_proto::wire::read_string(&mut c).expect("criterion");
                if rustmc_proto::wire::read_bool(&mut c).expect("nullable") {
                    let mut b = [0u8; 8];
                    std::io::Read::read_exact(&mut c, &mut b).expect("instant");
                }
            }
        }
        let show = rustmc_proto::wire::read_bool(&mut c).expect("showAdvancements");
        #[allow(clippy::cast_sign_loss)]
        (reset, n_added as usize, removed, keys, show)
    }

    #[test]
    fn a_fresh_player_is_sent_no_advancement_packet_at_all() {
        let table = fixture();
        let tree = Tree::build(&table);
        let mut pa = PlayerAdvancements::new(&table, &tree);
        assert!(
            pa.flush_dirty(true).expect("write").is_none(),
            "offsets 197-223 refuse an empty packet even on the first tick"
        );
        assert!(
            pa.flush_dirty(true).expect("write").is_none(),
            "and every tick after it"
        );
        // The positive control for the two Nones above: the same object DOES
        // produce a packet once something is dirty, so the Nones are the
        // early exit and not a writer that never writes.
        pa.award("t:root", "a", 1);
        let body = pa.flush_dirty(true).expect("write").expect("a packet");
        let (reset, _, _, _, show) = ids_in(&body);
        assert!(
            !reset,
            "isFirstPacket was cleared by the empty first tick at offset 250"
        );
        assert!(show, "ServerPlayer.tick passes iconst_1");
    }

    /// The case the whole module exists for, and the one an eyeball would
    /// get wrong: ONE criterion on the root sends the whole tree.
    #[test]
    fn one_award_sends_the_whole_visible_tree_once() {
        let table = fixture();
        let tree = Tree::build(&table);
        let mut pa = PlayerAdvancements::new(&table, &tree);
        pa.flush_dirty(true).expect("write"); // the join packet
        let (changed, rewards) = pa.award("t:root", "a", 1_000);
        assert!(changed);
        assert_eq!(rewards, Rewards::default(), "the root's rewards are empty");
        let body = pa.flush_dirty(true).expect("write").expect("a packet");
        let (reset, added, removed, keys, _) = ids_in(&body);
        assert!(!reset, "only the first packet resets");
        // root, child, grandchild — visible because the root is SHOW and
        // VISIBILITY_DEPTH reaches two below it. `t:deep` is three below and
        // `t:nodisplay` has no display.
        assert_eq!(added, 3, "added: {added}");
        assert!(removed.is_empty());
        let mut k = keys;
        k.sort();
        assert_eq!(
            k,
            vec![
                "t:child".to_string(),
                "t:grandchild".to_string(),
                "t:root".to_string()
            ],
            "getOrStartProgress inserts every walked node, so every newly \
             visible one is also progressChanged"
        );
        assert!(
            pa.flush_dirty(true).expect("write").is_none(),
            "the tree is not re-sent"
        );
    }

    /// The negative control for the case above: without the award nothing is
    /// sent at all, so the 3 added ids are the award's doing and not the
    /// harness's.
    #[test]
    fn without_the_award_no_second_packet_exists() {
        let table = fixture();
        let tree = Tree::build(&table);
        let mut pa = PlayerAdvancements::new(&table, &tree);
        pa.flush_dirty(true).expect("write");
        assert!(pa.flush_dirty(true).expect("write").is_none());
    }

    #[test]
    fn depth_two_is_the_limit_and_a_deeper_award_extends_it() {
        let table = fixture();
        let tree = Tree::build(&table);
        let mut pa = PlayerAdvancements::new(&table, &tree);
        pa.flush_dirty(true).expect("write");
        pa.award("t:root", "a", 1);
        pa.flush_dirty(true).expect("write");
        assert!(
            !pa.is_visible("t:deep"),
            "three below a SHOW is out of reach"
        );
        pa.award("t:child", "a", 2);
        let body = pa.flush_dirty(true).expect("write").expect("a packet");
        let (_, added, _, _, _) = ids_in(&body);
        assert_eq!(added, 1, "only t:deep is newly visible");
        assert!(pa.is_visible("t:deep"));
    }

    #[test]
    fn rewards_are_granted_once_on_completion_and_a_function_is_refused() {
        let table = fixture();
        let tree = Tree::build(&table);
        let mut pa = PlayerAdvancements::new(&table, &tree);
        let (_, r) = pa.award("t:child", "a", 1);
        assert_eq!(r.experience, 7);
        assert_eq!(r.recipes, vec!["t:r".to_string()]);
        assert_eq!(r.loot, vec!["t:l".to_string()]);
        assert_eq!(
            r.refused_functions,
            vec!["t:child".to_string()],
            "a function reward is reported, never run"
        );
        let (changed, again) = pa.award("t:child", "a", 2);
        assert!(
            !changed,
            "a second grant of the same criterion changes nothing"
        );
        assert_eq!(again, Rewards::default(), "and pays nothing");
    }

    #[test]
    fn an_advancement_with_no_captured_bytes_is_counted_not_sent() {
        // `t:nodisplay` has `holder_wire: null`, but it is also HIDE, so
        // reach the counter through a table whose displayed entry has none.
        let table = Table::parse(&serde_json::json!({
            "advancements": {
                "u:root": {"parent": null, "requirements": [["a"]],
                    "criteria_triggers": {"a": "minecraft:tick"},
                    "display": {"hidden": false}, "experience": 0,
                    "reward_recipes": [], "reward_loot": [], "reward_function": false,
                    "holder_wire": null}
            }
        }))
        .expect("parses");
        let tree = Tree::build(&table);
        let mut pa = PlayerAdvancements::new(&table, &tree);
        pa.flush_dirty(true).expect("write");
        pa.award("u:root", "a", 1);
        let body = pa.flush_dirty(true).expect("write").expect("a packet");
        let (_, added, _, keys, _) = ids_in(&body);
        assert_eq!(added, 0, "no bytes, so nothing in added");
        assert_eq!(pa.unsendable(), 1, "and it is counted rather than lost");
        assert_eq!(keys, vec!["u:root".to_string()], "progress still names it");
    }

    #[test]
    fn revoking_a_completed_advancement_removes_the_tree_from_view() {
        let table = fixture();
        let tree = Tree::build(&table);
        let mut pa = PlayerAdvancements::new(&table, &tree);
        pa.flush_dirty(true).expect("write");
        pa.award("t:root", "a", 1);
        pa.flush_dirty(true).expect("write");
        assert!(pa.is_visible("t:grandchild"));
        assert!(pa.revoke("t:root", "a"));
        let body = pa.flush_dirty(true).expect("write").expect("a packet");
        let (_, added, removed, _, _) = ids_in(&body);
        assert_eq!(added, 0);
        let mut r = removed;
        r.sort();
        assert_eq!(
            r,
            vec![
                "t:child".to_string(),
                "t:grandchild".to_string(),
                "t:root".to_string()
            ]
        );
        assert!(!pa.is_visible("t:root"));
    }

    #[test]
    fn awarding_an_unknown_advancement_does_nothing() {
        let table = fixture();
        let tree = Tree::build(&table);
        let mut pa = PlayerAdvancements::new(&table, &tree);
        let (changed, r) = pa.award("t:absent", "a", 1);
        assert!(!changed);
        assert_eq!(r, Rewards::default());
        assert!(pa.progress_of("t:absent").is_none());
    }
}
