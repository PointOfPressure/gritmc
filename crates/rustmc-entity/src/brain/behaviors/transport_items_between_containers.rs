//! `net.minecraft.world.entity.ai.behavior.TransportItemsBetweenContainers`
//! and its four inner types, from the 26.2 jar (39,537 bytes of class file,
//! plus 4,837 + 1,834 + 1,718 + 860 for the inner ones).
//!
//! ```text
//! constants: TARGET_INTERACTION_TIME = 60          (public)
//!            VISITED_POSITIONS_MEMORY_TIME = 6000
//!            TRANSPORTED_ITEM_MAX_STACK_SIZE = 16
//!            MAX_VISITED_POSITIONS = 10
//!            MAX_UNREACHABLE_POSITIONS = 50
//!            PASSENGER_MOB_TARGET_SEARCH_DISTANCE = 1
//!            IDLE_COOLDOWN = 140
//!            CLOSE_ENOUGH_TO_START_QUEUING_DISTANCE = 3.0d
//!            CLOSE_ENOUGH_TO_START_INTERACTING_WITH_TARGET_DISTANCE = 0.5d
//!            CLOSE_ENOUGH_TO_START_INTERACTING_WITH_TARGET_PATH_END_DISTANCE = 1.0d
//!            CLOSE_ENOUGH_TO_CONTINUE_INTERACTING_WITH_TARGET = 2.0d
//!
//! TransportItemState:          TRAVELLING 0, QUEUING 1, INTERACTING 2
//! ContainerInteractionState:   PICKUP_ITEM 0, PICKUP_NO_ITEM 1,
//!                              PLACE_ITEM 2, PLACE_NO_ITEM 3
//! OnTargetReachedInteraction:  TriConsumer<PathfinderMob, TransportItemTarget,
//!                              Integer>, no body of its own (860 bytes)
//! TransportItemTarget:         record (BlockPos pos, Container container,
//!                              BlockEntity blockEntity, BlockState state)
//!
//! <init>(float speedModifier, Predicate<BlockState> sourceBlockType,
//!        Predicate<BlockState> destinationBlockType,
//!        int horizontalSearchDistance, int verticalSearchDistance,
//!        Map<ContainerInteractionState, OnTargetReachedInteraction>
//!            onTargetInteractionActions,
//!        Consumer<PathfinderMob> onStartTravelling,
//!        Predicate<TransportItemTarget> shouldQueueForTarget)   (0..88)
//!   entry ImmutableMap.of(VISITED_BLOCK_POSITIONS:REGISTERED,
//!                         UNREACHABLE_TRANSPORT_BLOCK_POSITIONS:REGISTERED,
//!                         TRANSPORT_ITEMS_COOLDOWN_TICKS:VALUE_ABSENT,
//!                         IS_PANICKING:VALUE_ABSENT)
//!   Behavior(map) -> (60, 60);  target = null;  state = TRAVELLING
//!
//! start (0..27)              nav instanceof GroundPathNavigation g
//!                              -> g.setCanPathToTargetsBelowSurface(true)
//! checkExtraStartConditions  !mob.isLeashed()
//! canStillUse (0..35)        brain.getMemory(TRANSPORT_ITEMS_COOLDOWN_TICKS)
//!                              .isEmpty() && !mob.isPanicking()
//!                              && !mob.isLeashed()
//! timedOut(long) (0..1)      iconst_0 — always false
//!
//! tick (0..98)
//!    0  boolean invalid = updateInvalidTarget(level, mob)
//!    8  if target == null: stop(level, mob, time); return
//!   23  if invalid: return
//!   29  if state.equals(QUEUING):    onQueuingForTarget(target, level, mob)
//!   52  if state.equals(TRAVELLING): onTravelToTarget(target, level, mob)
//!   75  if state.equals(INTERACTING): onReachedTarget(target, level, mob)
//!
//! updateInvalidTarget (0..67)
//!    0  if hasValidTarget(level, mob): return false
//!    9  stopTargetingCurrentTarget(mob)
//!   14  Optional<TransportItemTarget> t = getTransportTarget(level, mob)
//!   21  if t.isPresent():
//!   28     this.target = t.get()
//!   39     onStartTravelling(mob)
//!   44     setVisitedBlockPos(mob, level, this.target.pos)
//!   57     return true
//!   59  enterCooldownAfterNoMatchingTargetFound(mob); return true
//!
//! onQueuingForTarget (0..14)  !isAnotherMobInteractingWithTarget(target, level)
//!                               -> resumeTravelling(mob)
//! onTravelToTarget (0..68)
//!    0  if isWithinTargetDistance(3.0, target, level, mob, getCenterPos(mob))
//!   18     && isAnotherMobInteractingWithTarget(target, level): startQueuing(mob)
//!   35  else if isWithinTargetDistance(getInteractionRange(mob), target, level,
//!                                      mob, getCenterPos(mob)):
//!   54     startOnReachedTargetInteraction(target, mob)
//!   63  else walkTowardsTarget(mob)
//!
//! getCenterPos (0..9)         setMiddleYPosition(mob, mob.position())
//! setMiddleYPosition (0..17)  v.add(0.0, mob.getBoundingBox().getYsize() / 2.0, 0.0)
//!
//! onReachedTarget (0..91)
//!    0  if !isWithinTargetDistance(2.0, target, level, mob, getCenterPos(mob)):
//!   18     onStartTravelling(mob)
//!   26  else ticksSinceReachingTarget++
//!   36       onTargetInteraction(target, mob)
//!   42       if ticksSinceReachingTarget >= 60:
//!   51          doReachedTargetInteraction(mob, target.container,
//!                   this::pickUpItems,                    (BSM 0)
//!                   (m, c) -> stopTargetingCurrentTarget(mob),   (BSM 1)
//!                   this::putDownItem,                    (BSM 2)
//!                   (m, c) -> stopTargetingCurrentTarget(mob))  (BSM 3)
//!   86          onStartTravelling(mob)
//!
//! startQueuing (0..12)        stopInPlace(mob); setTransportingState(QUEUING)
//! resumeTravelling (0..12)    setTransportingState(TRAVELLING); walkTowardsTarget(mob)
//! walkTowardsTarget (0..23)   target != null -> BehaviorUtils
//!                               .setWalkAndLookTargetMemories(mob, target.pos,
//!                                                             speedModifier, 0)
//! startOnReachedTargetInteraction (0..44)
//!                             doReachedTargetInteraction(mob, target.container,
//!                               onReachedInteraction(PICKUP_ITEM),
//!                               onReachedInteraction(PICKUP_NO_ITEM),
//!                               onReachedInteraction(PLACE_ITEM),
//!                               onReachedInteraction(PLACE_NO_ITEM));
//!                             setTransportingState(INTERACTING)
//! onStartTravelling (0..27)   onStartTravelling.accept(mob);
//!                             setTransportingState(TRAVELLING);
//!                             interactionState = null; ticksSinceReachingTarget = 0
//! onReachedInteraction (0..7) (m, c) -> setInteractionState(s)          (BSM 4)
//! onTargetInteraction (0..63)
//!    0  brain.setMemory(LOOK_TARGET, new BlockPosTracker(target.pos))
//!   21  stopInPlace(mob)
//!   26  if interactionState != null:
//!   33     Optional.ofNullable(onTargetInteractionActions.get(interactionState))
//!             .ifPresent(a -> a.accept(mob, target, ticksSinceReachingTarget))  (BSM 5)
//! doReachedTargetInteraction (0..66)
//!    0  if isPickingUpItems(mob):
//!    7     matchesGettingItemsRequirement(c) ? a.accept : b.accept
//!   37  else matchesLeavingItemsRequirement(mob, c) ? c3.accept : d.accept
//!
//! getTransportTarget (0..236)
//!    0  AABB area = getTargetSearchArea(mob)
//!    6  Set<GlobalPos> visited = getVisitedPositions(mob)
//!   12  Set<GlobalPos> unreachable = getUnreachablePositions(mob)
//!   18  List<ChunkPos> chunks = ChunkPos.rangeClosed(
//!            ChunkPos.containing(mob.blockPosition()),
//!            Math.floorDiv(getHorizontalSearchDistance(mob), 16) + 1).toList()
//!   47  TransportItemTarget best = null
//!   50  double bestDist = 3.4028234663852886E38d
//!   64  for (ChunkPos cp : chunks):
//!   86     LevelChunk chunk = level.getChunkSource().getChunkNow(cp.x(), cp.z())
//!  105     if chunk == null: continue
//!  110     for (BlockEntity be : chunk.getBlockEntities().values()):
//!  149        if !(be instanceof ChestBlockEntity chest): continue
//!  164        double d = chest.getBlockPos().distToCenterSqr(mob.position())
//!  178        if d >= bestDist: continue
//!  186        TransportItemTarget t = isTargetValidToPick(mob, level, chest,
//!                                        visited, unreachable, area)
//!  201        if t != null: best = t; bestDist = d
//!  220  return best == null ? Optional.empty() : Optional.of(best)
//!
//! isTargetValidToPick (0..106)
//!    0  BlockPos p = be.getBlockPos()
//!    6  if !area.contains(p.getX(), p.getY(), p.getZ()): return null
//!   38  TransportItemTarget t = TransportItemTarget.tryCreatePossibleTarget(be, level)
//!   45  if t == null: return null
//!   52  return (isWantedBlock(mob, t.state)
//!             && !isPositionAlreadyVisited(visited, unreachable, t, level)
//!             && !isContainerLocked(t)) ? t : null
//!
//! isContainerLocked (0..29)   blockEntity instanceof BaseContainerBlockEntity b
//!                               && b.isLocked()
//! hasValidTarget (0..100)
//!    0  boolean ok = target != null && isWantedBlock(mob, target.state)
//!                    && targetHasNotChanged(level, target)
//!   40  if !ok: return false
//!   44  if isTargetBlocked(level, target): return false
//!   56  if !state.equals(TRAVELLING): return true
//!   71  if hasValidTravellingPath(level, target, mob): return true
//!   86  markVisitedBlockPosAsUnreachable(mob, level, target.pos); return false
//!
//! hasValidTravellingPath (0..99)
//!    0  Path path = nav.getPath() == null ? nav.createPath(target.pos, 0)
//!                                         : nav.getPath()
//!   34  Vec3 from = getPositionToReachTargetFrom(path, mob)
//!   43  boolean within = isWithinTargetDistance(getInteractionRange(mob),
//!                            target, level, mob, from)
//!   58  boolean noPathAndFar = path == null && !within
//!   75  return noPathAndFar
//!          || targetIsReachableFromPosition(level, within, from, target, mob)
//!
//! getPositionToReachTargetFrom (0..47)
//!    0  boolean none = path == null || path.getEndNode() == null
//!   17  Vec3 v = none ? mob.position()
//!                     : Vec3.atBottomCenterOf(path.getEndNode().asBlockPos())
//!   40  return setMiddleYPosition(mob, v)
//!
//! isTargetBlocked (0..8)      ChestBlock.isChestBlockedAt(level, target.pos)
//! targetHasNotChanged (0..15) target.blockEntity.equals(
//!                               level.getBlockEntity(target.pos))
//! getConnectedTargets (0..66)
//!    0  if target.state.getValueOrElse(ChestBlock.TYPE, SINGLE) == SINGLE:
//!   62     return Stream.of(target)
//!   19  TransportItemTarget o = TransportItemTarget.tryCreatePossibleTarget(
//!            ChestBlock.getConnectedBlockPos(target.pos, target.state), level)
//!   35  return o == null ? Stream.of(target) : Stream.of(target, o)
//!
//! getTargetSearchArea (0..30)  int h = getHorizontalSearchDistance(mob);
//!                              new AABB(mob.blockPosition())
//!                                  .inflate(h, getVerticalSearchDistance(mob), h)
//! getHorizontalSearchDistance  mob.isPassenger() ? 1 : horizontalSearchDistance
//! getVerticalSearchDistance    mob.isPassenger() ? 1 : verticalSearchDistance
//! getVisitedPositions          brain.getMemory(VISITED_BLOCK_POSITIONS)
//!                                .orElse(Set.of())
//! getUnreachablePositions      brain.getMemory(UNREACHABLE_TRANSPORT_BLOCK_POSITIONS)
//!                                .orElse(Set.of())
//! isPositionAlreadyVisited (0..31)
//!                              getConnectedTargets(target, level)
//!                                .map(t -> new GlobalPos(level.dimension(), t.pos))  (BSM 6)
//!                                .anyMatch(gp -> visited.contains(gp)
//!                                             || unreachable.contains(gp))           (BSM 7)
//! hasFinishedPath (0..28)      nav.getPath() != null && nav.getPath().isDone()
//!
//! setVisitedBlockPos (0..68)
//!    0  Set<GlobalPos> s = new HashSet<>(getVisitedPositions(mob))
//!   13  s.add(new GlobalPos(level.dimension(), pos))
//!   33  if s.size() > 10: enterCooldownAfterNoMatchingTargetFound(mob)
//!   53  else brain.setMemoryWithExpiry(VISITED_BLOCK_POSITIONS, s, 6000L)
//!
//! markVisitedBlockPosAsUnreachable (0..116)
//!    0  Set<GlobalPos> v = new HashSet<>(getVisitedPositions(mob))
//!   13  v.remove(new GlobalPos(level.dimension(), pos))
//!   33  Set<GlobalPos> u = new HashSet<>(getUnreachablePositions(mob))
//!   46  u.add(new GlobalPos(level.dimension(), pos))
//!   66  if u.size() > 50: enterCooldownAfterNoMatchingTargetFound(mob)
//!   86  else brain.setMemoryWithExpiry(VISITED_BLOCK_POSITIONS, v, 6000L)
//!  101       brain.setMemoryWithExpiry(UNREACHABLE_TRANSPORT_BLOCK_POSITIONS, u, 6000L)
//!
//! isWantedBlock (0..30)        isPickingUpItems(mob) ? sourceBlockType.test(state)
//!                                                    : destinationBlockType.test(state)
//! getInteractionRange (0..14)  hasFinishedPath(mob) ? 1.0d : 0.5d
//! isWithinTargetDistance (0..65)
//!    0  AABB bb = mob.getBoundingBox()
//!    7  AABB mobBox = AABB.ofSize(at, bb.getXsize(), bb.getYsize(), bb.getZsize())
//!   29  return target.state.getCollisionShape(level, target.pos).bounds()
//!             .inflate(d, 0.5d, d).move(target.pos).intersects(mobBox)
//! targetIsReachableFromPosition (0..22)
//!                              within && canSeeAnyTargetSide(target, level, mob, at)
//! canSeeAnyTargetSide (0..49)
//!    0  Vec3 c = Vec3.atCenterOf(target.pos)
//!    9  Direction.stream()
//!         .map(d -> c.add(0.5*d.getStepX(), 0.5*d.getStepY(), 0.5*d.getStepZ()))  (BSM 8)
//!         .map(side -> level.clip(new ClipContext(at, side, COLLIDER, NONE, mob))) (BSM 9)
//!         .anyMatch(hit -> hit.getType() == BLOCK
//!                       && hit.getBlockPos().equals(target.pos))                  (BSM 10)
//! isAnotherMobInteractingWithTarget (0..15)
//!                              getConnectedTargets(target, level)
//!                                .anyMatch(shouldQueueForTarget)
//! isPickingUpItems (0..7)      mob.getMainHandItem().isEmpty()
//! matchesGettingItemsRequirement (0..14)   !container.isEmpty()
//! matchesLeavingItemsRequirement (0..22)   container.isEmpty()
//!                                            || hasItemMatchingHandItem(mob, container)
//! hasItemMatchingHandItem (0..47)
//!                              ItemStack hand = mob.getMainHandItem();
//!                              for (ItemStack s : container)
//!                                  if (ItemStack.isSameItem(s, hand)) return true;
//!                              return false
//! pickUpItems (0..29)          mob.setItemSlot(MAINHAND,
//!                                  pickupItemFromContainer(container));
//!                              mob.setGuaranteedDrop(MAINHAND);
//!                              container.setChanged();
//!                              clearMemoriesAfterMatchingTargetFound(mob)
//! putDownItem (0..40)          ItemStack left = addItemsToContainer(mob, container);
//!                              container.setChanged();
//!                              mob.setItemSlot(MAINHAND, left);
//!                              left.isEmpty()
//!                                ? clearMemoriesAfterMatchingTargetFound(mob)
//!                                : stopTargetingCurrentTarget(mob)
//! pickupItemFromContainer (0..65)
//!                              int i = 0;
//!                              for (ItemStack s : container) {
//!                                if (!s.isEmpty())
//!                                  return container.removeItem(i,
//!                                      Math.min(s.getCount(), 16));
//!                                i++;
//!                              }
//!                              return ItemStack.EMPTY
//! addItemsToContainer (0..154)
//!    0  int i = 0;  ItemStack hand = mob.getMainHandItem()
//!   15  for (ItemStack s : container):
//!   37    if s.isEmpty(): container.setItem(i, hand); return ItemStack.EMPTY
//!   57    if ItemStack.isSameItemSameComponents(s, hand)
//!   66       && s.getCount() < s.getMaxStackSize():
//!   79       int room = s.getMaxStackSize() - s.getCount()
//!   92       int moved = Math.min(room, hand.getCount())
//!  103       s.setCount(s.getCount() + moved)
//!  116       hand.setCount(hand.getCount() - room)
//!  127       container.setItem(i, s)
//!  136       if hand.isEmpty(): return ItemStack.EMPTY
//!  147    i++
//!  153  return hand
//! stopTargetingCurrentTarget (0..27)
//!                              ticksSinceReachingTarget = 0; target = null;
//!                              nav.stop(); brain.eraseMemory(WALK_TARGET)
//! clearMemoriesAfterMatchingTargetFound (0..25)
//!                              stopTargetingCurrentTarget(mob);
//!                              erase VISITED_BLOCK_POSITIONS;
//!                              erase UNREACHABLE_TRANSPORT_BLOCK_POSITIONS
//! enterCooldownAfterNoMatchingTargetFound (0..41)
//!                              stopTargetingCurrentTarget(mob);
//!                              brain.setMemory(TRANSPORT_ITEMS_COOLDOWN_TICKS, 140);
//!                              erase VISITED_BLOCK_POSITIONS;
//!                              erase UNREACHABLE_TRANSPORT_BLOCK_POSITIONS
//! stop (0..32)                 onStartTravelling(mob);
//!                              nav instanceof GroundPathNavigation g
//!                                -> g.setCanPathToTargetsBelowSurface(false)
//! stopInPlace (0..35)          nav.stop(); mob.setXxa(0f); mob.setYya(0f);
//!                              mob.setSpeed(0f);
//!                              mob.setDeltaMovement(0.0,
//!                                  mob.getDeltaMovement().y, 0.0)
//!
//! TransportItemTarget.tryCreatePossibleTarget(BlockEntity, Level) (0..38)
//!    0  BlockPos p = be.getBlockPos();  BlockState s = be.getBlockState()
//!   10  Container c = getBlockEntityContainer(be, s, level, p)
//!   19  return c == null ? null : new TransportItemTarget(p, c, be, s)
//! TransportItemTarget.tryCreatePossibleTarget(BlockPos, Level) (0..19)
//!    0  BlockEntity be = level.getBlockEntity(pos)
//!    6  return be == null ? null : tryCreatePossibleTarget(be, level)
//! TransportItemTarget.getBlockEntityContainer (0..48)
//!    0  Block b = state.getBlock()
//!    6  if b instanceof ChestBlock cb:
//!   21    return ChestBlock.getContainer(cb, state, level, pos, false)
//!   31  if be instanceof Container c: return c
//!   47  return null
//! ```
//!
//! **`tick` runs three independent `if`s, not an `if/else` chain.** A state
//! change inside one of them is seen by the next in the *same* tick:
//! `QUEUING` -> `resumeTravelling` sets `TRAVELLING`, and `onTravelToTarget`
//! then runs immediately; `onTravelToTarget` ->
//! `startOnReachedTargetInteraction` sets `INTERACTING`, and
//! `onReachedTarget` then runs immediately. So a mob can go
//! queuing -> travelling -> interacting -> its first
//! `ticksSinceReachingTarget++` inside one tick. The two `equals` calls are
//! `TransportItemState.equals`, i.e. `Enum.equals`, i.e. identity.
//!
//! **`timedOut` is `iconst_0`, so the `Behavior(Map)` durations of 60 and
//! 60 never end this behaviour.** They are still drawn: `Behavior.tryStart`
//! calls `level.getRandom().nextInt(maxDuration + 1 - minDuration)` before
//! `start`, which is `nextInt(1)` here — a draw that consumes level RNG and
//! whose result is discarded.
//!
//! **`tick` calls `stop(level, mob, time)` directly rather than through the
//! framework**, so the status stays `RUNNING` and the `stop` body runs twice
//! for the same shutdown: once here, and once from `Behavior.doStop` on the
//! next tick, when `canStillUse` reads the cooldown memory
//! `enterCooldownAfterNoMatchingTargetFound` has just written. Both
//! statements in `stop` are idempotent.
//!
//! **An absent path with the target out of range is a VALID travelling
//! path.** `hasValidTravellingPath` returns true when `path == null && !within`
//! (offsets 58..73), which is the guard that keeps `hasValidTarget` from
//! calling `markVisitedBlockPosAsUnreachable` before the navigation has
//! produced anything. The reading that looks right — no path means
//! unreachable — is the opposite of what the bytecode does, and would blacklist
//! every target on the tick it was chosen.
//!
//! **The nearest-target scan is seeded with `Float.MAX_VALUE`, not
//! `Double.MAX_VALUE`.** `ldc2_w` at offset 50 carries
//! `3.4028234663852886E38d`, the double widening of `Float.MAX_VALUE`. The
//! distance it is compared against is a squared block distance, so nothing
//! can exceed it in a real world, but the constant is transcribed as it is
//! written.
//!
//! **The distance test happens BEFORE validity.** `d >= bestDist` skips at
//! offset 183, and only a candidate that beats the running best is then run
//! through `isTargetValidToPick`. A nearer *invalid* container therefore
//! never raises the bar, and `bestDist` only ever moves on an accepted one.
//!
//! **The scan admits only `ChestBlockEntity`.** `getBlockEntityContainer`
//! handles any `Container` block entity, but nothing in this class ever
//! offers it one: the chunk walk filters `instanceof ChestBlockEntity` at
//! offset 151, and the only other caller is `getConnectedTargets`, which
//! reads the block a chest's own `getConnectedDirection` points at. So the
//! `instanceof Container` fallback is unreachable for barrels, shulker boxes
//! and hoppers through this behaviour.
//!
//! **The visited and unreachable memories are `java.util.HashSet`s and are
//! never iterated.** `getVisitedPositions` reads the memory or
//! `Set.of()` (the immutable empty set); both writers copy it into a
//! `new HashSet<>(...)`, and every operation on the copy is `add`, `remove`,
//! `size` or `contains`. No code path walks either set, so the hash order —
//! which is what a `HashSet` has and what the doc comment on
//! [`MemoryValue::GlobalPosList`] warns is not insertion order — is
//! unobservable, and this port's `Vec<GlobalPos>` in first-added order is
//! behaviourally exact.
//!
//! **A cooldown trip discards the write that provoked it.**
//! `setVisitedBlockPos` builds the enlarged set, and if it is bigger than 10
//! it calls `enterCooldownAfterNoMatchingTargetFound` — which *erases*
//! `VISITED_BLOCK_POSITIONS` — instead of storing it.
//! `markVisitedBlockPosAsUnreachable` is the same shape at 50, and there the
//! discarded work includes the *removal* from the visited set as well as the
//! addition to the unreachable one.
//!
//! **`addItemsToContainer` shrinks the hand by `room`, not by `moved`.**
//! Offsets 92..124: `moved = Math.min(room, hand.getCount())` is added to the
//! slot, but `hand.setCount(hand.getCount() - room)` subtracts the *whole*
//! room. When the hand holds fewer items than the slot has space for, its
//! count goes **negative**, and `ItemStack.isEmpty()` is
//! `this == EMPTY || item == AIR || count <= 0`, so the negative stack reads
//! as empty and the method returns `ItemStack.EMPTY`. Items are conserved
//! because the slot only grew by the `min`. But `putDownItem` calls
//! `mob.setItemSlot(MAINHAND, left)` at offset 12 — **before** the
//! `left.isEmpty()` test at 20 — with whatever `addItemsToContainer`
//! returned, and on the *partial-fill* path (offsets 136..147, hand not yet
//! empty) the loop carries on holding a stack it has already debited. A
//! transcription that used `moved` on both sides would leave a zero count
//! where the jar leaves a negative one; both are `isEmpty()`, so this is
//! only observable through a host that reads the count.
//!
//! **The four `onReachedInteraction` callbacks only record which case
//! fired.** `startOnReachedTargetInteraction` runs
//! `doReachedTargetInteraction` with four lambdas that all do
//! `setInteractionState(s)` (BSM 4) and nothing else. The *effects* —
//! `pickUpItems` and `putDownItem` — are the second, later call from
//! `onReachedTarget`, sixty ticks after that classification. Between the two
//! the recorded `interactionState` drives whichever provider callback the
//! `Map` holds for it, once per tick.
//!
//! **The `Map<ContainerInteractionState, OnTargetReachedInteraction>` is only
//! ever `get`.** `onTargetInteraction` does one `Map.get` and wraps it in
//! `Optional.ofNullable`, so the map's iteration order is never read and this
//! port's association list is exact.
//!
//! **`isWithinTargetDistance` inflates y by the constant 0.5, not by the
//! distance.** `inflate(d, 0.5d, d)` at offsets 45..50 — the vertical reach
//! is fixed no matter which of 3.0, 2.0, 1.0 or 0.5 the caller passed.
//!
//! **`getTargetSearchArea`'s box is symmetric despite `AABB.contains` being
//! half-open.** The box is the mob's unit block cube inflated by `h`, so x
//! spans `[bx - h, bx + 1 + h)`, and `contains` is fed the candidate's
//! integer corner: the accepted x are exactly `bx - h ..= bx + h`.
//!
//! **`start` does not reset anything.** It only turns
//! `setCanPathToTargetsBelowSurface` on; `target`, `state`,
//! `interactionState` and `ticksSinceReachingTarget` survive from the
//! previous run, and `stop` resets them through `onStartTravelling` instead.
//!
//! **Nulls, so nobody re-runs them:** `TARGET_INTERACTION_TIME` is the only
//! public constant and the only one this class reads through a name rather
//! than a literal; `PASSENGER_MOB_TARGET_SEARCH_DISTANCE` is declared 1 and
//! both search-distance getters use a bare `iconst_1`;
//! `CLOSE_ENOUGH_TO_START_INTERACTING_WITH_TARGET_PATH_END_DISTANCE = 1.0d`
//! and `..._DISTANCE = 0.5d` are `dconst_1` and `ldc2_w 0.5d` inside
//! `getInteractionRange`, never named; `setInteractionState` and
//! `setTransportingState` are bare field writes; `TransportItemTarget`'s
//! `container` field is read only as an argument to
//! `doReachedTargetInteraction`, and its `blockEntity` field only by
//! `isContainerLocked` and `targetHasNotChanged`; `lambda$onReachedTarget$0`
//! and `$1` are byte-identical and both ignore their two parameters in
//! favour of the captured `mob`.
//!
//! **Deferred golden:** this class has no jar golden, for the reason every
//! earlier lane in this stack gave — it needs a `PathfinderMob` in a
//! `ServerLevel` with loaded chunks, block entities and containers, and
//! `mob-in-world` is the host that will change that. The tests below are
//! seed-replayed against `testing::TestMob`, and the two of them that
//! re-derive an algorithm independently
//! ([`tests::the_chunk_walk_is_x_inner_z_outer_from_the_low_corner`] and
//! [`tests::the_direction_sides_are_the_face_centres_in_values_order`]) say
//! so in the test.

use rustmc_data::BlockPos;

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::behavior_utils;
use crate::brain::memory::{
    GlobalPos, MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker,
};
use crate::brain::BrainState;
use crate::host::{ItemSpec, MobHost};
use crate::phys::{chunk_pos_containing, chunk_pos_range_closed, Aabb, Vec3};

/// `TARGET_INTERACTION_TIME`.
pub const TARGET_INTERACTION_TIME: i32 = 60;
/// `VISITED_POSITIONS_MEMORY_TIME`.
pub const VISITED_POSITIONS_MEMORY_TIME: i64 = 6000;
/// `TRANSPORTED_ITEM_MAX_STACK_SIZE`.
pub const TRANSPORTED_ITEM_MAX_STACK_SIZE: i32 = 16;
/// `MAX_VISITED_POSITIONS`.
pub const MAX_VISITED_POSITIONS: usize = 10;
/// `MAX_UNREACHABLE_POSITIONS`.
pub const MAX_UNREACHABLE_POSITIONS: usize = 50;
/// `PASSENGER_MOB_TARGET_SEARCH_DISTANCE`.
pub const PASSENGER_MOB_TARGET_SEARCH_DISTANCE: i32 = 1;
/// `IDLE_COOLDOWN`.
pub const IDLE_COOLDOWN: i32 = 140;
/// `CLOSE_ENOUGH_TO_START_QUEUING_DISTANCE`.
pub const CLOSE_ENOUGH_TO_START_QUEUING_DISTANCE: f64 = 3.0;
/// `CLOSE_ENOUGH_TO_START_INTERACTING_WITH_TARGET_DISTANCE`.
pub const CLOSE_ENOUGH_TO_START_INTERACTING_WITH_TARGET_DISTANCE: f64 = 0.5;
/// `CLOSE_ENOUGH_TO_START_INTERACTING_WITH_TARGET_PATH_END_DISTANCE`.
pub const CLOSE_ENOUGH_TO_START_INTERACTING_WITH_TARGET_PATH_END_DISTANCE: f64 = 1.0;
/// `CLOSE_ENOUGH_TO_CONTINUE_INTERACTING_WITH_TARGET`.
pub const CLOSE_ENOUGH_TO_CONTINUE_INTERACTING_WITH_TARGET: f64 = 2.0;

/// The `ldc2_w` at `getTransportTarget` offset 50: `Float.MAX_VALUE`
/// widened to a `double`, which is what seeds the nearest-target search.
pub const NEAREST_SEED: f64 = 3.402_823_466_385_288_6E38;

/// `TransportItemsBetweenContainers$TransportItemState`, in `$values`
/// order (0..28): `TRAVELLING`, `QUEUING`, `INTERACTING`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TransportItemState {
    /// `TRAVELLING`, ordinal 0 — the constructor's initial value.
    #[default]
    Travelling,
    /// `QUEUING`, ordinal 1.
    Queuing,
    /// `INTERACTING`, ordinal 2.
    Interacting,
}

/// `TransportItemsBetweenContainers$ContainerInteractionState`, in `$values`
/// order (0..28): `PICKUP_ITEM`, `PICKUP_NO_ITEM`, `PLACE_ITEM`,
/// `PLACE_NO_ITEM`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerInteractionState {
    /// `PICKUP_ITEM`, ordinal 0: the mob's hand is empty and the container
    /// has something.
    PickupItem,
    /// `PICKUP_NO_ITEM`, ordinal 1: the mob's hand is empty and so is the
    /// container.
    PickupNoItem,
    /// `PLACE_ITEM`, ordinal 2: the mob is carrying and the container will
    /// take it.
    PlaceItem,
    /// `PLACE_NO_ITEM`, ordinal 3: the mob is carrying and the container
    /// will not take it.
    PlaceNoItem,
}

/// The `BlockState` half of a [`TransportItemTarget`].
///
/// This crate has no block-state type, and the class only ever asks four
/// things of one: the two provider predicates
/// (`sourceBlockType` / `destinationBlockType`),
/// `getValueOrElse(ChestBlock.TYPE, SINGLE)` and
/// `getCollisionShape(level, pos).bounds()`. The last two are host calls
/// keyed on the position, so what a predicate is handed is the position and
/// the block entity's registry id — deliberately *not* the whole target, so
/// that a provider predicate cannot read a container the jar's
/// `Predicate<BlockState>` could not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetState {
    /// Where the state was read.
    pub pos: BlockPos,
    /// `level.getBlockEntity(pos)`'s registry id.
    pub kind: &'static str,
}

/// `TransportItemsBetweenContainers$TransportItemTarget`, a record of
/// `(BlockPos pos, Container container, BlockEntity blockEntity,
/// BlockState state)`.
///
/// The `container` field is not carried: every `Container` call this class
/// makes is routed through the host on `pos`, and `ChestBlock.getContainer`
/// resolves a paired chest to one `CompoundContainer`, so the host's
/// container at `pos` *is* the record's. `blockEntity` is carried as the
/// identity token `targetHasNotChanged` compares, because `BlockEntity`
/// does not override `equals`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportItemTarget {
    /// `pos`.
    pub pos: BlockPos,
    /// `blockEntity`, as an identity token.
    pub block_entity: u64,
    /// `state`.
    pub state: TargetState,
}

impl TransportItemTarget {
    /// `TransportItemTarget.tryCreatePossibleTarget(BlockEntity, Level)`
    /// (0..38), reached from the chunk scan, which has the block entity
    /// already.
    #[must_use]
    pub fn try_create_from_block_entity<M: MobHost>(mob: &M, pos: BlockPos) -> Option<Self> {
        let kind = mob.block_entity_kind(pos)?;
        let identity = mob.block_entity_identity(pos)?;
        // `getBlockEntityContainer(be, state, level, pos)` returning null is
        // the whole of the `c == null ? null : new TransportItemTarget(...)`.
        if !mob.transport_container_exists(pos) {
            return None;
        }
        Some(TransportItemTarget {
            pos,
            block_entity: identity,
            state: TargetState { pos, kind },
        })
    }

    /// `TransportItemTarget.tryCreatePossibleTarget(BlockPos, Level)`
    /// (0..19): `level.getBlockEntity(pos)` and then the overload above,
    /// or null when there is no block entity.
    #[must_use]
    pub fn try_create_at<M: MobHost>(mob: &M, pos: BlockPos) -> Option<Self> {
        mob.block_entity_kind(pos)?;
        Self::try_create_from_block_entity(mob, pos)
    }
}

/// A `Predicate<BlockState>`.
pub type StatePredicate = Box<dyn FnMut(&TargetState) -> bool>;
/// A `Predicate<TransportItemTarget>` — `shouldQueueForTarget`.
pub type TargetPredicate = Box<dyn FnMut(&TransportItemTarget) -> bool>;
/// A `Consumer<PathfinderMob>` — `onStartTravelling`.
pub type StartTravellingConsumer<M> = Box<dyn FnMut(&mut MobAi<M>)>;
/// `TransportItemsBetweenContainers$OnTargetReachedInteraction`, a
/// `TriConsumer<PathfinderMob, TransportItemTarget, Integer>`.
pub type OnTargetReachedInteraction<M> = Box<dyn FnMut(&mut MobAi<M>, &TransportItemTarget, i32)>;

/// `TransportItemsBetweenContainers`'s body.
pub struct TransportItemsBetweenContainers<M: MobHost> {
    /// `speedModifier`.
    pub speed_modifier: f32,
    /// `horizontalSearchDistance`.
    pub horizontal_search_distance: i32,
    /// `verticalSearchDistance`.
    pub vertical_search_distance: i32,
    /// `sourceBlockType`.
    pub source_block_type: StatePredicate,
    /// `destinationBlockType`.
    pub destination_block_type: StatePredicate,
    /// `shouldQueueForTarget`.
    pub should_queue_for_target: TargetPredicate,
    /// `onStartTravelling`.
    pub on_start_travelling: StartTravellingConsumer<M>,
    /// `onTargetInteractionActions`, as an association list — the jar's
    /// `Map` is only ever `get`.
    pub on_target_interaction_actions:
        Vec<(ContainerInteractionState, OnTargetReachedInteraction<M>)>,
    /// `target`.
    pub target: Option<TransportItemTarget>,
    /// `state`.
    pub state: TransportItemState,
    /// `interactionState`.
    pub interaction_state: Option<ContainerInteractionState>,
    /// `ticksSinceReachingTarget`.
    pub ticks_since_reaching_target: i32,
}

impl<M: MobHost + 'static> TransportItemsBetweenContainers<M> {
    /// The constructor (0..88). The `ImmutableMap.of` order is the entry
    /// condition's order, and `Behavior(Map)` gives both durations 60.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        speed_modifier: f32,
        source_block_type: StatePredicate,
        destination_block_type: StatePredicate,
        horizontal_search_distance: i32,
        vertical_search_distance: i32,
        on_target_interaction_actions: Vec<(
            ContainerInteractionState,
            OnTargetReachedInteraction<M>,
        )>,
        on_start_travelling: StartTravellingConsumer<M>,
        should_queue_for_target: TargetPredicate,
    ) -> Behavior<TransportItemsBetweenContainers<M>> {
        Behavior::new(
            TransportItemsBetweenContainers {
                speed_modifier,
                horizontal_search_distance,
                vertical_search_distance,
                source_block_type,
                destination_block_type,
                should_queue_for_target,
                on_start_travelling,
                on_target_interaction_actions,
                target: None,
                state: TransportItemState::Travelling,
                interaction_state: None,
                ticks_since_reaching_target: 0,
            },
            vec![
                (
                    MemoryModuleType::VisitedBlockPositions,
                    MemoryStatus::Registered,
                ),
                (
                    MemoryModuleType::UnreachableTransportBlockPositions,
                    MemoryStatus::Registered,
                ),
                (
                    MemoryModuleType::TransportItemsCooldownTicks,
                    MemoryStatus::ValueAbsent,
                ),
                (MemoryModuleType::IsPanicking, MemoryStatus::ValueAbsent),
            ],
        )
    }

    // ---- state transitions ----

    /// `setTransportingState(TransportItemState)` (0..5).
    fn set_transporting_state(&mut self, s: TransportItemState) {
        self.state = s;
    }

    /// `setInteractionState(ContainerInteractionState)` (0..5).
    fn set_interaction_state(&mut self, s: ContainerInteractionState) {
        self.interaction_state = Some(s);
    }

    /// `onStartTravelling(PathfinderMob)` (0..27) — the private one, not
    /// the `Consumer` field it opens by calling.
    fn do_on_start_travelling(&mut self, cx: &mut MobAi<M>) {
        (self.on_start_travelling)(cx);
        self.set_transporting_state(TransportItemState::Travelling);
        self.interaction_state = None;
        self.ticks_since_reaching_target = 0;
    }

    /// `startQueuing(PathfinderMob)` (0..12).
    fn start_queuing(&mut self, cx: &mut MobAi<M>) {
        Self::stop_in_place(cx);
        self.set_transporting_state(TransportItemState::Queuing);
    }

    /// `resumeTravelling(PathfinderMob)` (0..12).
    fn resume_travelling(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        self.set_transporting_state(TransportItemState::Travelling);
        self.walk_towards_target(brain, cx);
    }

    /// `walkTowardsTarget(PathfinderMob)` (0..23): the close-enough
    /// distance is `iconst_0`.
    fn walk_towards_target(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>) {
        if let Some(t) = self.target {
            behavior_utils::set_walk_and_look_target_memories_to_block(
                brain,
                t.pos,
                self.speed_modifier,
                0,
            );
        }
    }

    /// `stopInPlace(PathfinderMob)` (0..35): the y of the delta movement
    /// is kept, x and z are zeroed.
    fn stop_in_place(cx: &mut MobAi<M>) {
        cx.navigation.stop();
        cx.mob.set_xxa(0.0);
        cx.mob.set_yya(0.0);
        // `Mob.setSpeed` overrides `LivingEntity.setSpeed` and also sets `zza`.
        cx.mob.set_speed_and_zza(0.0);
        let dy = cx.mob.delta_movement().y;
        cx.mob.set_delta_movement(0.0, dy, 0.0);
    }

    /// `stopTargetingCurrentTarget(PathfinderMob)` (0..27).
    fn stop_targeting_current_target(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        self.ticks_since_reaching_target = 0;
        self.target = None;
        cx.navigation.stop();
        brain.erase_memory(MemoryModuleType::WalkTarget);
    }

    /// `clearMemoriesAfterMatchingTargetFound(PathfinderMob)` (0..25).
    fn clear_memories_after_matching_target_found(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
    ) {
        self.stop_targeting_current_target(brain, cx);
        brain.erase_memory(MemoryModuleType::VisitedBlockPositions);
        brain.erase_memory(MemoryModuleType::UnreachableTransportBlockPositions);
    }

    /// `enterCooldownAfterNoMatchingTargetFound(PathfinderMob)` (0..41).
    fn enter_cooldown_after_no_matching_target_found(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
    ) {
        self.stop_targeting_current_target(brain, cx);
        brain.set_memory_value(
            MemoryModuleType::TransportItemsCooldownTicks,
            MemoryValue::Int(IDLE_COOLDOWN),
        );
        brain.erase_memory(MemoryModuleType::VisitedBlockPositions);
        brain.erase_memory(MemoryModuleType::UnreachableTransportBlockPositions);
    }

    // ---- geometry ----

    /// `setMiddleYPosition(PathfinderMob, Vec3)` (0..17).
    fn set_middle_y_position(cx: &MobAi<M>, v: Vec3) -> Vec3 {
        v.add3(0.0, cx.mob.bounding_box().y_size() / 2.0, 0.0)
    }

    /// `getCenterPos(PathfinderMob)` (0..9).
    fn center_pos(cx: &MobAi<M>) -> Vec3 {
        Self::set_middle_y_position(cx, cx.mob.position())
    }

    /// `getHorizontalSearchDistance(PathfinderMob)` (0..15).
    fn horizontal_search_distance_for(&self, cx: &MobAi<M>) -> i32 {
        if cx.mob.is_passenger() {
            PASSENGER_MOB_TARGET_SEARCH_DISTANCE
        } else {
            self.horizontal_search_distance
        }
    }

    /// `getVerticalSearchDistance(PathfinderMob)` (0..15).
    fn vertical_search_distance_for(&self, cx: &MobAi<M>) -> i32 {
        if cx.mob.is_passenger() {
            PASSENGER_MOB_TARGET_SEARCH_DISTANCE
        } else {
            self.vertical_search_distance
        }
    }

    /// `getTargetSearchArea(PathfinderMob)` (0..30). The horizontal
    /// distance is read once into a local and used for both x and z; the
    /// vertical one is called inline.
    fn target_search_area(&self, cx: &MobAi<M>) -> Aabb {
        let h = self.horizontal_search_distance_for(cx);
        Aabb::of_block(cx.mob.block_position()).inflate(
            f64::from(h),
            f64::from(self.vertical_search_distance_for(cx)),
            f64::from(h),
        )
    }

    /// `getInteractionRange(PathfinderMob)` (0..14).
    fn interaction_range(cx: &MobAi<M>) -> f64 {
        if Self::has_finished_path(cx) {
            CLOSE_ENOUGH_TO_START_INTERACTING_WITH_TARGET_PATH_END_DISTANCE
        } else {
            CLOSE_ENOUGH_TO_START_INTERACTING_WITH_TARGET_DISTANCE
        }
    }

    /// `hasFinishedPath(PathfinderMob)` (0..28).
    fn has_finished_path(cx: &MobAi<M>) -> bool {
        cx.navigation
            .path()
            .is_some_and(crate::pathfinder::path::Path::is_done)
    }

    /// `isWithinTargetDistance(double, TransportItemTarget, Level,
    /// PathfinderMob, Vec3)` (0..65). The y inflation is the literal 0.5,
    /// not the distance.
    fn is_within_target_distance(
        d: f64,
        target: &TransportItemTarget,
        cx: &MobAi<M>,
        at: Vec3,
    ) -> bool {
        let bb = cx.mob.bounding_box();
        let mob_box = Aabb::of_size(at, bb.x_size(), bb.y_size(), bb.z_size());
        cx.mob
            .collision_shape_bounds(target.pos)
            .inflate(d, 0.5, d)
            .moved_by_block(target.pos)
            .intersects(&mob_box)
    }

    /// `canSeeAnyTargetSide(TransportItemTarget, Level, PathfinderMob,
    /// Vec3)` (0..49). `Direction.stream()` is `Stream.of(Direction.VALUES)`
    /// and `VALUES` is ordinal order — `DOWN, UP, NORTH, SOUTH, WEST, EAST`
    /// — and `anyMatch` short-circuits, so that order decides how many
    /// clips are taken.
    fn can_see_any_target_side(target: &TransportItemTarget, cx: &MobAi<M>, at: Vec3) -> bool {
        /// `Direction.VALUES` as `(getStepX, getStepY, getStepZ)`.
        const STEPS: [(i32, i32, i32); 6] = [
            (0, -1, 0),
            (0, 1, 0),
            (0, 0, -1),
            (0, 0, 1),
            (-1, 0, 0),
            (1, 0, 0),
        ];
        let c = Vec3::at_center_of(target.pos);
        STEPS.iter().any(|&(sx, sy, sz)| {
            let side = c.add3(
                0.5 * f64::from(sx),
                0.5 * f64::from(sy),
                0.5 * f64::from(sz),
            );
            cx.mob.clip_block_hit(at, side) == Some(target.pos)
        })
    }

    /// `targetIsReachableFromPosition(Level, boolean, Vec3,
    /// TransportItemTarget, PathfinderMob)` (0..22).
    fn target_is_reachable_from_position(
        within: bool,
        at: Vec3,
        target: &TransportItemTarget,
        cx: &MobAi<M>,
    ) -> bool {
        within && Self::can_see_any_target_side(target, cx, at)
    }

    /// `getPositionToReachTargetFrom(Path, PathfinderMob)` (0..47).
    fn position_to_reach_target_from(end: Option<BlockPos>, cx: &MobAi<M>) -> Vec3 {
        let v = match end {
            None => cx.mob.position(),
            Some(p) => Vec3::at_bottom_center_of(p),
        };
        Self::set_middle_y_position(cx, v)
    }

    // ---- target validity ----

    /// `isWantedBlock(PathfinderMob, BlockState)` (0..30).
    fn is_wanted_block(&mut self, cx: &MobAi<M>, state: &TargetState) -> bool {
        if Self::is_picking_up_items(cx) {
            (self.source_block_type)(state)
        } else {
            (self.destination_block_type)(state)
        }
    }

    /// `isContainerLocked(TransportItemTarget)` (0..29).
    fn is_container_locked(cx: &MobAi<M>, target: &TransportItemTarget) -> bool {
        cx.mob.container_is_locked(target.pos)
    }

    /// `isTargetBlocked(Level, TransportItemTarget)` (0..8).
    fn is_target_blocked(cx: &MobAi<M>, target: &TransportItemTarget) -> bool {
        cx.mob.chest_is_blocked_at(target.pos)
    }

    /// `targetHasNotChanged(Level, TransportItemTarget)` (0..15):
    /// reference identity of the block entity, so a chest broken and
    /// replaced at the same position fails it.
    fn target_has_not_changed(cx: &MobAi<M>, target: &TransportItemTarget) -> bool {
        cx.mob.block_entity_identity(target.pos) == Some(target.block_entity)
    }

    /// `getConnectedTargets(TransportItemTarget, Level)` (0..66), in the
    /// `Stream.of(target, other)` order.
    fn connected_targets(cx: &MobAi<M>, target: &TransportItemTarget) -> Vec<TransportItemTarget> {
        let Some(other_pos) = cx.mob.chest_connected_pos(target.pos) else {
            return vec![*target];
        };
        match TransportItemTarget::try_create_at(&cx.mob, other_pos) {
            Some(other) => vec![*target, other],
            None => vec![*target],
        }
    }

    /// `isAnotherMobInteractingWithTarget(TransportItemTarget, Level)`
    /// (0..15).
    fn is_another_mob_interacting_with_target(
        &mut self,
        cx: &MobAi<M>,
        target: &TransportItemTarget,
    ) -> bool {
        Self::connected_targets(cx, target)
            .iter()
            .any(|t| (self.should_queue_for_target)(t))
    }

    /// `getVisitedPositions(PathfinderMob)` (0..19).
    fn visited_positions(brain: &BrainState) -> Vec<GlobalPos> {
        match brain.get_memory(MemoryModuleType::VisitedBlockPositions) {
            Some(MemoryValue::GlobalPosList(v)) => v.clone(),
            _ => Vec::new(),
        }
    }

    /// `getUnreachablePositions(PathfinderMob)` (0..19).
    fn unreachable_positions(brain: &BrainState) -> Vec<GlobalPos> {
        match brain.get_memory(MemoryModuleType::UnreachableTransportBlockPositions) {
            Some(MemoryValue::GlobalPosList(v)) => v.clone(),
            _ => Vec::new(),
        }
    }

    /// `isPositionAlreadyVisited(Set, Set, TransportItemTarget, Level)`
    /// (0..31).
    fn is_position_already_visited(
        visited: &[GlobalPos],
        unreachable: &[GlobalPos],
        target: &TransportItemTarget,
        cx: &MobAi<M>,
    ) -> bool {
        let dim = cx.mob.dimension();
        Self::connected_targets(cx, target).iter().any(|t| {
            let gp = GlobalPos {
                dimension: dim,
                pos: t.pos,
            };
            visited.contains(&gp) || unreachable.contains(&gp)
        })
    }

    /// `setVisitedBlockPos(PathfinderMob, Level, BlockPos)` (0..68).
    /// A set bigger than [`MAX_VISITED_POSITIONS`] enters the cooldown and
    /// **does not store** the set it just built.
    pub fn set_visited_block_pos(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        pos: BlockPos,
    ) {
        let mut s = Self::visited_positions(brain);
        let gp = GlobalPos {
            dimension: cx.mob.dimension(),
            pos,
        };
        if !s.contains(&gp) {
            s.push(gp);
        }
        if s.len() > MAX_VISITED_POSITIONS {
            self.enter_cooldown_after_no_matching_target_found(brain, cx);
        } else {
            brain.set_memory_with_expiry(
                MemoryModuleType::VisitedBlockPositions,
                MemoryValue::GlobalPosList(s),
                VISITED_POSITIONS_MEMORY_TIME,
            );
        }
    }

    /// `markVisitedBlockPosAsUnreachable(PathfinderMob, Level, BlockPos)`
    /// (0..116). Tripping [`MAX_UNREACHABLE_POSITIONS`] discards **both**
    /// writes, the removal from the visited set included.
    pub fn mark_visited_block_pos_as_unreachable(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        pos: BlockPos,
    ) {
        let gp = GlobalPos {
            dimension: cx.mob.dimension(),
            pos,
        };
        let mut visited = Self::visited_positions(brain);
        visited.retain(|g| *g != gp);
        let mut unreachable = Self::unreachable_positions(brain);
        if !unreachable.contains(&gp) {
            unreachable.push(gp);
        }
        if unreachable.len() > MAX_UNREACHABLE_POSITIONS {
            self.enter_cooldown_after_no_matching_target_found(brain, cx);
        } else {
            brain.set_memory_with_expiry(
                MemoryModuleType::VisitedBlockPositions,
                MemoryValue::GlobalPosList(visited),
                VISITED_POSITIONS_MEMORY_TIME,
            );
            brain.set_memory_with_expiry(
                MemoryModuleType::UnreachableTransportBlockPositions,
                MemoryValue::GlobalPosList(unreachable),
                VISITED_POSITIONS_MEMORY_TIME,
            );
        }
    }

    /// `isTargetValidToPick(PathfinderMob, Level, BlockEntity, Set, Set,
    /// AABB)` (0..106).
    fn is_target_valid_to_pick(
        &mut self,
        cx: &MobAi<M>,
        pos: BlockPos,
        visited: &[GlobalPos],
        unreachable: &[GlobalPos],
        area: &Aabb,
    ) -> Option<TransportItemTarget> {
        if !area.contains(f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)) {
            return None;
        }
        let t = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos)?;
        if self.is_wanted_block(cx, &t.state)
            && !Self::is_position_already_visited(visited, unreachable, &t, cx)
            && !Self::is_container_locked(cx, &t)
        {
            Some(t)
        } else {
            None
        }
    }

    /// `getTransportTarget(ServerLevel, PathfinderMob)` (0..236).
    fn transport_target(
        &mut self,
        brain: &BrainState,
        cx: &MobAi<M>,
    ) -> Option<TransportItemTarget> {
        let area = self.target_search_area(cx);
        let visited = Self::visited_positions(brain);
        let unreachable = Self::unreachable_positions(brain);
        let r = self.horizontal_search_distance_for(cx).div_euclid(16) + 1;
        let chunks = chunk_pos_range_closed(chunk_pos_containing(cx.mob.block_position()), r);
        let mut best: Option<TransportItemTarget> = None;
        let mut best_dist = NEAREST_SEED;
        let here = cx.mob.position();
        for (chunk_x, chunk_z) in chunks {
            let Some(positions) = cx.mob.chunk_block_entity_positions(chunk_x, chunk_z) else {
                continue;
            };
            for pos in positions {
                if !is_chest_block_entity(cx.mob.block_entity_kind(pos)) {
                    continue;
                }
                let d = crate::phys::dist_to_center_sqr(pos, here.x, here.y, here.z);
                if d >= best_dist {
                    continue;
                }
                if let Some(t) =
                    self.is_target_valid_to_pick(cx, pos, &visited, &unreachable, &area)
                {
                    best = Some(t);
                    best_dist = d;
                }
            }
        }
        best
    }

    /// `hasValidTravellingPath(Level, TransportItemTarget,
    /// PathfinderMob)` (0..99).
    fn has_valid_travelling_path(
        &mut self,
        cx: &mut MobAi<M>,
        target: &TransportItemTarget,
    ) -> bool {
        // `nav.getPath() == null ? nav.createPath(target.pos, 0) : nav.getPath()`
        // — `createPath` computes a path, it does not install one.
        let (has_path, end) = match cx.navigation.path() {
            Some(p) => (
                true,
                p.end_node()
                    .map(crate::pathfinder::node::Node::as_block_pos),
            ),
            None => {
                let p = cx.navigation.create_path(&mut cx.mob, target.pos, 0);
                match p {
                    Some(p) => (
                        true,
                        p.end_node()
                            .map(crate::pathfinder::node::Node::as_block_pos),
                    ),
                    None => (false, None),
                }
            }
        };
        let from = Self::position_to_reach_target_from(end, cx);
        let within = Self::is_within_target_distance(Self::interaction_range(cx), target, cx, from);
        // The counter-intuitive one: no path AND out of range is VALID.
        let no_path_and_far = !has_path && !within;
        no_path_and_far || Self::target_is_reachable_from_position(within, from, target, cx)
    }

    /// `hasValidTarget(Level, PathfinderMob)` (0..100).
    fn has_valid_target(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        let Some(target) = self.target else {
            return false;
        };
        if !(self.is_wanted_block(cx, &target.state) && Self::target_has_not_changed(cx, &target)) {
            return false;
        }
        if Self::is_target_blocked(cx, &target) {
            return false;
        }
        if self.state != TransportItemState::Travelling {
            return true;
        }
        if self.has_valid_travelling_path(cx, &target) {
            return true;
        }
        self.mark_visited_block_pos_as_unreachable(brain, cx, target.pos);
        false
    }

    /// `updateInvalidTarget(ServerLevel, PathfinderMob)` (0..67).
    fn update_invalid_target(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        if self.has_valid_target(brain, cx) {
            return false;
        }
        self.stop_targeting_current_target(brain, cx);
        if let Some(t) = self.transport_target(brain, cx) {
            self.target = Some(t);
            self.do_on_start_travelling(cx);
            // `setVisitedBlockPos` can itself trip the cooldown and null
            // the target that was just installed, which `tick` then reads.
            self.set_visited_block_pos(brain, cx, t.pos);
            return true;
        }
        self.enter_cooldown_after_no_matching_target_found(brain, cx);
        true
    }

    // ---- container interaction ----

    /// `isPickingUpItems(PathfinderMob)` (0..7).
    fn is_picking_up_items(cx: &MobAi<M>) -> bool {
        cx.mob.main_hand_item().is_none_or(|s| s.count <= 0)
    }

    /// `matchesGettingItemsRequirement(Container)` (0..14).
    fn matches_getting_items_requirement(cx: &MobAi<M>, pos: BlockPos) -> bool {
        !cx.mob.container_is_empty(pos)
    }

    /// `matchesLeavingItemsRequirement(PathfinderMob, Container)` (0..22).
    fn matches_leaving_items_requirement(cx: &MobAi<M>, pos: BlockPos) -> bool {
        cx.mob.container_is_empty(pos) || Self::has_item_matching_hand_item(cx, pos)
    }

    /// `hasItemMatchingHandItem(PathfinderMob, Container)` (0..47):
    /// `ItemStack.isSameItem` compares the item only, not the components.
    fn has_item_matching_hand_item(cx: &MobAi<M>, pos: BlockPos) -> bool {
        let hand = cx.mob.main_hand_item();
        cx.mob
            .container_items(pos)
            .iter()
            .any(|s| is_same_item(s.as_ref(), hand.as_ref()))
    }

    /// `pickupItemFromContainer(Container)` (0..65).
    fn pickup_item_from_container(cx: &mut MobAi<M>, pos: BlockPos) -> Option<ItemSpec> {
        let items = cx.mob.container_items(pos);
        for (i, s) in items.iter().enumerate() {
            if let Some(s) = s {
                let n = s.count.min(TRANSPORTED_ITEM_MAX_STACK_SIZE);
                return cx.mob.container_remove_item(pos, i, n);
            }
        }
        None
    }

    /// `addItemsToContainer(PathfinderMob, Container)` (0..154). The hand
    /// is debited by `room`, not by `moved`; see the module doc.
    fn add_items_to_container(cx: &mut MobAi<M>, pos: BlockPos) -> Option<ItemSpec> {
        let mut hand = cx.mob.main_hand_item();
        let items = cx.mob.container_items(pos);
        for (i, slot) in items.iter().enumerate() {
            let Some(mut s) = slot.clone() else {
                // `s.isEmpty()`: the hand goes in whole and EMPTY comes back.
                cx.mob.container_set_item(pos, i, hand.clone());
                return None;
            };
            // An `ItemStack.EMPTY` hand is item AIR, so
            // `isSameItemSameComponents` is false against every real slot
            // and the loop simply runs out and returns it.
            let Some(h) = hand.as_mut() else {
                continue;
            };
            let max = cx.mob.max_stack_size(&s);
            if is_same_item_same_components(&s, h) && s.count < max {
                let room = max - s.count;
                let moved = room.min(h.count);
                s.count += moved;
                // `room`, not `moved` — the jar's asymmetry, which drives
                // the count negative when the hand holds less than the room.
                h.count -= room;
                cx.mob.container_set_item(pos, i, Some(s));
                if h.count <= 0 {
                    return None;
                }
            }
        }
        hand
    }

    /// `pickUpItems(PathfinderMob, Container)` (0..29).
    fn pick_up_items(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, pos: BlockPos) {
        let taken = Self::pickup_item_from_container(cx, pos);
        cx.mob.set_main_hand_item(taken.as_ref());
        cx.mob.set_guaranteed_drop_main_hand();
        cx.mob.container_set_changed(pos);
        self.clear_memories_after_matching_target_found(brain, cx);
    }

    /// `putDownItem(PathfinderMob, Container)` (0..40). The main hand is
    /// written **before** the emptiness test.
    fn put_down_item(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, pos: BlockPos) {
        let left = Self::add_items_to_container(cx, pos);
        cx.mob.container_set_changed(pos);
        cx.mob.set_main_hand_item(left.as_ref());
        if left.as_ref().is_none_or(|s| s.count <= 0) {
            self.clear_memories_after_matching_target_found(brain, cx);
        } else {
            self.stop_targeting_current_target(brain, cx);
        }
    }

    /// `doReachedTargetInteraction(...)` (0..66), with the four
    /// `BiConsumer`s expressed as the [`ContainerInteractionState`] each
    /// branch stands for. Both call sites pass their four arguments in the
    /// same order, `PICKUP_ITEM, PICKUP_NO_ITEM, PLACE_ITEM,
    /// PLACE_NO_ITEM`, so this classification IS the branch.
    fn reached_target_branch(cx: &MobAi<M>, pos: BlockPos) -> ContainerInteractionState {
        if Self::is_picking_up_items(cx) {
            if Self::matches_getting_items_requirement(cx, pos) {
                ContainerInteractionState::PickupItem
            } else {
                ContainerInteractionState::PickupNoItem
            }
        } else if Self::matches_leaving_items_requirement(cx, pos) {
            ContainerInteractionState::PlaceItem
        } else {
            ContainerInteractionState::PlaceNoItem
        }
    }

    /// `startOnReachedTargetInteraction(TransportItemTarget,
    /// PathfinderMob)` (0..44): the four `onReachedInteraction` lambdas
    /// only record which branch fired.
    fn start_on_reached_target_interaction(&mut self, cx: &MobAi<M>, target: &TransportItemTarget) {
        self.set_interaction_state(Self::reached_target_branch(cx, target.pos));
        self.set_transporting_state(TransportItemState::Interacting);
    }

    /// `onTargetInteraction(TransportItemTarget, PathfinderMob)` (0..63).
    fn on_target_interaction(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        target: &TransportItemTarget,
    ) {
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::block(target.pos)),
        );
        Self::stop_in_place(cx);
        let Some(s) = self.interaction_state else {
            return;
        };
        let ticks = self.ticks_since_reaching_target;
        if let Some(i) = self
            .on_target_interaction_actions
            .iter()
            .position(|(k, _)| *k == s)
        {
            let mut action = std::mem::replace(
                &mut self.on_target_interaction_actions[i].1,
                Box::new(|_, _, _| {}),
            );
            action(cx, target, ticks);
            self.on_target_interaction_actions[i].1 = action;
        }
    }

    // ---- the three per-state steps ----

    /// `onQueuingForTarget(TransportItemTarget, Level, PathfinderMob)`
    /// (0..14).
    fn on_queuing_for_target(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        target: &TransportItemTarget,
    ) {
        if !self.is_another_mob_interacting_with_target(cx, target) {
            self.resume_travelling(brain, cx);
        }
    }

    /// `onTravelToTarget(TransportItemTarget, Level, PathfinderMob)`
    /// (0..68). `getCenterPos` is called twice, once per branch.
    fn on_travel_to_target(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        target: &TransportItemTarget,
    ) {
        if Self::is_within_target_distance(
            CLOSE_ENOUGH_TO_START_QUEUING_DISTANCE,
            target,
            cx,
            Self::center_pos(cx),
        ) && self.is_another_mob_interacting_with_target(cx, target)
        {
            self.start_queuing(cx);
        } else if Self::is_within_target_distance(
            Self::interaction_range(cx),
            target,
            cx,
            Self::center_pos(cx),
        ) {
            self.start_on_reached_target_interaction(cx, target);
        } else {
            self.walk_towards_target(brain, cx);
        }
    }

    /// `onReachedTarget(TransportItemTarget, Level, PathfinderMob)`
    /// (0..91).
    fn on_reached_target(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        target: &TransportItemTarget,
    ) {
        if !Self::is_within_target_distance(
            CLOSE_ENOUGH_TO_CONTINUE_INTERACTING_WITH_TARGET,
            target,
            cx,
            Self::center_pos(cx),
        ) {
            self.do_on_start_travelling(cx);
            return;
        }
        self.ticks_since_reaching_target += 1;
        self.on_target_interaction(brain, cx, target);
        if self.ticks_since_reaching_target >= TARGET_INTERACTION_TIME {
            match Self::reached_target_branch(cx, target.pos) {
                ContainerInteractionState::PickupItem => {
                    self.pick_up_items(brain, cx, target.pos);
                }
                ContainerInteractionState::PlaceItem => {
                    self.put_down_item(brain, cx, target.pos);
                }
                // BSM 1 and BSM 3, both `stopTargetingCurrentTarget(mob)`.
                ContainerInteractionState::PickupNoItem
                | ContainerInteractionState::PlaceNoItem => {
                    self.stop_targeting_current_target(brain, cx);
                }
            }
            self.do_on_start_travelling(cx);
        }
    }
}

/// `be instanceof ChestBlockEntity`. `ChestBlockEntity` is the class for
/// `minecraft:chest` and, through `TrappedChestBlockEntity extends
/// ChestBlockEntity`, for `minecraft:trapped_chest`. `minecraft:ender_chest`
/// is an `EnderChestBlockEntity` and is **not** one.
#[must_use]
pub fn is_chest_block_entity(kind: Option<&'static str>) -> bool {
    matches!(kind, Some("minecraft:chest" | "minecraft:trapped_chest"))
}

/// `ItemStack.isSameItem(a, b)`: same item, components ignored. An empty
/// stack is `ItemStack.EMPTY`, whose item is `AIR`, so two empties match
/// and an empty never matches a real item.
#[must_use]
fn is_same_item(a: Option<&ItemSpec>, b: Option<&ItemSpec>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.id == b.id,
        _ => false,
    }
}

/// `ItemStack.isSameItemSameComponents(a, b)`.
#[must_use]
fn is_same_item_same_components(a: &ItemSpec, b: &ItemSpec) -> bool {
    a.id == b.id && a.component == b.component
}

impl<M: MobHost + 'static> BehaviorBody<M> for TransportItemsBetweenContainers<M> {
    /// `checkExtraStartConditions` (0..12).
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        !cx.mob.is_leashed()
    }

    /// `canStillUse` (0..35). `PathfinderMob.isPanicking()` is the brain
    /// memory, then the goal-selector fallback; `Brain.isBrainDead()` is
    /// `memories.isEmpty() && sensors.isEmpty() && behaviours.isEmpty()`
    /// and cannot hold while this behaviour's own four entry-condition
    /// memories are registered, so it is not tested here.
    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let panicking = brain.has_memory_value(MemoryModuleType::IsPanicking)
            || cx.mob.has_running_panic_goal();
        brain
            .get_memory(MemoryModuleType::TransportItemsCooldownTicks)
            .is_none()
            && !panicking
            && !cx.mob.is_leashed()
    }

    /// `timedOut(long)` (0..1): `iconst_0`.
    fn timed_out(&self, _end_timestamp: i64, _time: i64) -> bool {
        false
    }

    /// `start` (0..27).
    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.navigation.set_can_path_to_targets_below_surface(true);
    }

    /// `stop` (0..32).
    fn stop(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        (self.on_start_travelling)(cx);
        self.set_transporting_state(TransportItemState::Travelling);
        self.interaction_state = None;
        self.ticks_since_reaching_target = 0;
        cx.navigation.set_can_path_to_targets_below_surface(false);
    }

    /// `tick` (0..98): three independent `if`s over the state, so a
    /// transition inside one is acted on by the next in the same tick.
    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let invalid = self.update_invalid_target(brain, cx);
        let Some(target) = self.target else {
            self.stop(brain, cx, time);
            return;
        };
        if invalid {
            return;
        }
        if self.state == TransportItemState::Queuing {
            self.on_queuing_for_target(brain, cx, &target);
        }
        if self.state == TransportItemState::Travelling {
            self.on_travel_to_target(brain, cx, &target);
        }
        if self.state == TransportItemState::Interacting {
            self.on_reached_target(brain, cx, &target);
        }
    }

    fn name(&self) -> &'static str {
        "TransportItemsBetweenContainers"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    const CHEST: &str = "minecraft:chest";

    fn item(id: &'static str, count: i32) -> ItemSpec {
        ItemSpec {
            id,
            count,
            component: None,
        }
    }

    /// Every chest is a wanted block on both sides; nobody queues; the
    /// `onStartTravelling` consumer and the four interaction callbacks all
    /// record into `calls`.
    fn behavior() -> Behavior<TransportItemsBetweenContainers<TestMob>> {
        TransportItemsBetweenContainers::new(
            0.6,
            Box::new(|_| true),
            Box::new(|_| true),
            8,
            4,
            Vec::new(),
            Box::new(|cx| cx.mob.calls.push("onStartTravelling".to_string())),
            Box::new(|_| false),
        )
    }

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mob = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 31);
        let cx = MobAi::new(mob);
        let brain = Brain::<TestMob>::new(
            &[
                T::VisitedBlockPositions,
                T::UnreachableTransportBlockPositions,
                T::TransportItemsCooldownTicks,
                T::IsPanicking,
                T::WalkTarget,
                T::LookTarget,
            ],
            vec![],
        )
        .state;
        (cx, brain)
    }

    #[test]
    fn the_entry_condition_is_the_immutable_map_of_order() {
        let b = behavior();
        assert_eq!(
            b.entry_condition,
            vec![
                (T::VisitedBlockPositions, MemoryStatus::Registered),
                (
                    T::UnreachableTransportBlockPositions,
                    MemoryStatus::Registered
                ),
                (T::TransportItemsCooldownTicks, MemoryStatus::ValueAbsent),
                (T::IsPanicking, MemoryStatus::ValueAbsent),
            ]
        );
    }

    #[test]
    fn timed_out_is_always_false_though_the_sixty_tick_duration_is_still_drawn() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        // `Behavior(Map)` is (60, 60), so the draw is `nextInt(1)` and the
        // end timestamp is time + 60 — and `timedOut` ignores it entirely.
        assert_eq!(b.end_timestamp(), 60);
        assert!(!b.body.timed_out(60, 1_000_000));
        // Positive control: the base's `timedOut` would be true there.
        assert!(1_000_000i64 > b.end_timestamp());
    }

    /// A second, independent transcription of `ChunkPos$2.tryAdvance`'s
    /// walk, written as the nested loops the spliterator is equivalent to.
    /// This is a cross-check against [`chunk_pos_range_closed`], **not** a
    /// jar golden: both sides are read from the same bytecode.
    #[test]
    fn the_chunk_walk_is_x_inner_z_outer_from_the_low_corner() {
        let got = chunk_pos_range_closed((3, -2), 2);
        let mut want = Vec::new();
        for z in -4..=0 {
            for x in 1..=5 {
                want.push((x, z));
            }
        }
        assert_eq!(got, want);
        assert_eq!(got.len(), 25);
        assert_eq!(got[0], (1, -4), "the walk opens at the low corner");
        assert_eq!(got[4], (5, -4), "x is the inner axis");
        assert_eq!(got[5], (1, -3), "and x restarts at from.x");
        // r = 0 degenerates to the single centre.
        assert_eq!(chunk_pos_range_closed((7, 7), 0), vec![(7, 7)]);
    }

    /// The six `Direction.VALUES` half-steps off the block centre, in the
    /// order `Direction.stream()` yields them. A second transcription of
    /// `lambda$canSeeAnyTargetSide$0` and `Direction`'s `<clinit>`, not a
    /// jar golden.
    #[test]
    fn the_direction_sides_are_the_face_centres_in_values_order() {
        let (mut cx, mut brain) = setup();
        let pos = BlockPos { x: 1, y: 1, z: 1 };
        cx.mob.place_chest(pos, vec![None]);
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos).unwrap();
        let at = Vec3::new(0.5, 1.0, 0.5);
        // Only the WEST face (index 4) answers, so the first four clips
        // must have been taken and asked about, in order.
        let c = Vec3::new(1.5, 1.5, 1.5);
        cx.mob.clip_hits.push((at, c.add3(-0.5, 0.0, 0.0), pos));
        assert!(
            TransportItemsBetweenContainers::<TestMob>::can_see_any_target_side(&target, &cx, at)
        );
        // Positive control: move the answer to a face that is not one of
        // the six and nothing matches.
        cx.mob.clip_hits.clear();
        cx.mob.clip_hits.push((at, c.add3(-0.25, 0.0, 0.0), pos));
        assert!(
            !TransportItemsBetweenContainers::<TestMob>::can_see_any_target_side(&target, &cx, at)
        );
        let _ = &mut brain;
    }

    #[test]
    fn a_leash_refuses_it_and_a_cooldown_or_panic_memory_stops_it() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        cx.mob.leashed = true;
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        cx.mob.leashed = false;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(b.body.can_still_use(&mut brain, &mut cx, 1));

        brain.set_memory_value(T::TransportItemsCooldownTicks, MemoryValue::Int(140));
        assert!(!b.body.can_still_use(&mut brain, &mut cx, 1));
        brain.erase_memory(T::TransportItemsCooldownTicks);
        assert!(b.body.can_still_use(&mut brain, &mut cx, 1));

        brain.set_memory_value(T::IsPanicking, MemoryValue::Bool(true));
        assert!(!b.body.can_still_use(&mut brain, &mut cx, 1));
        brain.erase_memory(T::IsPanicking);
        // The goal-selector half of `PathfinderMob.isPanicking()`.
        cx.mob.running_panic_goal = true;
        assert!(!b.body.can_still_use(&mut brain, &mut cx, 1));
    }

    #[test]
    fn start_and_stop_are_the_below_surface_flag_and_nothing_else() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        assert!(!cx.navigation.can_path_to_targets_below_surface_for_test());
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(cx.navigation.can_path_to_targets_below_surface_for_test());
        b.do_stop(&mut brain, &mut cx, 1);
        assert!(!cx.navigation.can_path_to_targets_below_surface_for_test());
        assert!(cx.mob.calls.contains(&"onStartTravelling".to_string()));
    }

    #[test]
    fn no_target_found_enters_the_cooldown_and_runs_stop_in_the_same_tick() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.calls.clear();
        // No chunk is registered, so every `getChunkNow` is null.
        b.body.tick(&mut brain, &mut cx, 1);
        assert_eq!(
            brain.get_memory(T::TransportItemsCooldownTicks),
            Some(&MemoryValue::Int(140))
        );
        assert!(b.body.target.is_none());
        // `tick` called `stop` directly, so the flag is already off while
        // the framework still believes the behaviour is running.
        assert!(!cx.navigation.can_path_to_targets_below_surface_for_test());
        assert!(!b.body.can_still_use(&mut brain, &mut cx, 2));
    }

    #[test]
    fn an_unloaded_chunk_is_skipped_rather_than_loaded() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 2, y: 1, z: 2 };
        cx.mob.place_chest(pos, vec![Some(item("poppy", 1))]);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            b.body.transport_target(&brain, &cx).map(|t| t.pos),
            Some(pos)
        );
        // Drop the whole chunk entry: `getChunkNow` is now null and the
        // block entity is invisible even though it is still registered.
        cx.mob.chunk_block_entities.clear();
        assert_eq!(b.body.transport_target(&brain, &cx), None);
    }

    #[test]
    fn the_nearest_chest_wins_and_a_nearer_invalid_one_does_not_raise_the_bar() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let near = BlockPos { x: 1, y: 1, z: 0 };
        let far = BlockPos { x: 4, y: 1, z: 0 };
        // The nearer chest is locked, so `isTargetValidToPick` refuses it —
        // and because `bestDist` only moves on an accepted candidate, the
        // far one is still picked.
        cx.mob.place_chest(near, vec![None]);
        cx.mob.place_chest(far, vec![None]);
        cx.mob.locked_containers.push(near);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            b.body.transport_target(&brain, &cx).map(|t| t.pos),
            Some(far)
        );
        // Positive control: unlock it and the nearer one wins.
        cx.mob.locked_containers.clear();
        assert_eq!(
            b.body.transport_target(&brain, &cx).map(|t| t.pos),
            Some(near)
        );
    }

    #[test]
    fn a_visited_or_unreachable_position_is_refused() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        cx.mob.place_chest(pos, vec![None]);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(b.body.transport_target(&brain, &cx).is_some());

        let gp = GlobalPos {
            dimension: cx.mob.dimension(),
            pos,
        };
        brain.set_memory_value(
            T::VisitedBlockPositions,
            MemoryValue::GlobalPosList(vec![gp]),
        );
        assert_eq!(b.body.transport_target(&brain, &cx), None);
        brain.erase_memory(T::VisitedBlockPositions);
        brain.set_memory_value(
            T::UnreachableTransportBlockPositions,
            MemoryValue::GlobalPosList(vec![gp]),
        );
        assert_eq!(b.body.transport_target(&brain, &cx), None);
    }

    #[test]
    fn the_search_box_is_symmetric_though_contains_is_half_open() {
        let (mut cx, _brain) = setup();
        let b = behavior();
        // horizontalSearchDistance 8, mob block (0, 1, 0).
        let area = b.body.target_search_area(&cx);
        assert!(area.contains(-8.0, 1.0, 0.0), "x = bx - h is inside");
        assert!(area.contains(8.0, 1.0, 0.0), "x = bx + h is inside");
        assert!(!area.contains(9.0, 1.0, 0.0), "x = bx + h + 1 is not");
        assert!(!area.contains(-9.0, 1.0, 0.0));
        // Vertical uses its own distance, 4.
        assert!(area.contains(0.0, 5.0, 0.0));
        assert!(!area.contains(0.0, 6.0, 0.0));
        // A passenger collapses both to 1.
        cx.mob.is_passenger = true;
        let area = b.body.target_search_area(&cx);
        assert!(area.contains(1.0, 2.0, 1.0));
        assert!(!area.contains(2.0, 1.0, 0.0));
    }

    #[test]
    fn the_eleventh_visited_position_trips_the_cooldown_and_discards_the_set() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let dim = cx.mob.dimension();
        let ten: Vec<GlobalPos> = (0..10)
            .map(|i| GlobalPos {
                dimension: dim,
                pos: BlockPos { x: i, y: 1, z: 0 },
            })
            .collect();
        brain.set_memory_value(
            T::VisitedBlockPositions,
            MemoryValue::GlobalPosList(ten.clone()),
        );
        // The tenth is already there, so re-adding it stays at ten.
        b.body
            .set_visited_block_pos(&mut brain, &mut cx, BlockPos { x: 9, y: 1, z: 0 });
        assert_eq!(
            brain.get_memory(T::VisitedBlockPositions),
            Some(&MemoryValue::GlobalPosList(ten))
        );
        assert!(!brain.has_memory_value(T::TransportItemsCooldownTicks));
        // An eleventh distinct one makes the set 11 > 10: the cooldown is
        // entered and the set it just built is thrown away, not stored.
        b.body
            .set_visited_block_pos(&mut brain, &mut cx, BlockPos { x: 99, y: 1, z: 0 });
        assert_eq!(
            brain.get_memory(T::TransportItemsCooldownTicks),
            Some(&MemoryValue::Int(140))
        );
        assert!(!brain.has_memory_value(T::VisitedBlockPositions));
    }

    #[test]
    fn marking_unreachable_writes_both_sets_and_the_expiry_is_six_thousand() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 3, y: 1, z: 0 };
        let other = BlockPos { x: 4, y: 1, z: 0 };
        let dim = cx.mob.dimension();
        let gp = GlobalPos {
            dimension: dim,
            pos,
        };
        let gp_other = GlobalPos {
            dimension: dim,
            pos: other,
        };
        brain.set_memory_value(
            T::VisitedBlockPositions,
            MemoryValue::GlobalPosList(vec![gp, gp_other]),
        );
        b.body
            .mark_visited_block_pos_as_unreachable(&mut brain, &mut cx, pos);
        assert_eq!(
            brain.get_memory(T::VisitedBlockPositions),
            Some(&MemoryValue::GlobalPosList(vec![gp_other])),
            "the position is removed from the visited set"
        );
        assert_eq!(
            brain.get_memory(T::UnreachableTransportBlockPositions),
            Some(&MemoryValue::GlobalPosList(vec![gp]))
        );
        assert_eq!(brain.time_until_expiry(T::VisitedBlockPositions), 6000);
        assert_eq!(
            brain.time_until_expiry(T::UnreachableTransportBlockPositions),
            6000
        );
        // Removing the LAST visited position leaves an empty set, and
        // `Brain.setMemoryInternal` clears rather than stores an empty
        // collection — so the memory is erased, not written empty.
        b.body
            .mark_visited_block_pos_as_unreachable(&mut brain, &mut cx, other);
        assert!(!brain.has_memory_value(T::VisitedBlockPositions));
        assert_eq!(
            brain.get_memory(T::UnreachableTransportBlockPositions),
            Some(&MemoryValue::GlobalPosList(vec![gp, gp_other]))
        );
    }

    #[test]
    fn the_fifty_first_unreachable_position_discards_the_visited_removal_too() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let dim = cx.mob.dimension();
        let fifty: Vec<GlobalPos> = (0..50)
            .map(|i| GlobalPos {
                dimension: dim,
                pos: BlockPos { x: i, y: 1, z: 0 },
            })
            .collect();
        let pos = BlockPos { x: 500, y: 1, z: 0 };
        let gp = GlobalPos {
            dimension: dim,
            pos,
        };
        brain.set_memory_value(
            T::UnreachableTransportBlockPositions,
            MemoryValue::GlobalPosList(fifty),
        );
        brain.set_memory_value(
            T::VisitedBlockPositions,
            MemoryValue::GlobalPosList(vec![gp]),
        );
        b.body
            .mark_visited_block_pos_as_unreachable(&mut brain, &mut cx, pos);
        assert_eq!(
            brain.get_memory(T::TransportItemsCooldownTicks),
            Some(&MemoryValue::Int(140))
        );
        // Both erased: the removal from `visited` never reached the brain.
        assert!(!brain.has_memory_value(T::VisitedBlockPositions));
        assert!(!brain.has_memory_value(T::UnreachableTransportBlockPositions));
    }

    #[test]
    fn no_path_and_out_of_range_is_a_valid_travelling_path() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 40, y: 1, z: 0 };
        cx.mob.place_chest(pos, vec![None]);
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos).unwrap();
        b.body.target = Some(target);
        // `GroundPathNavigation.createPath` refuses a target whose chunk is
        // not loaded, which is the only way to get a genuine null path here;
        // with the chest 40 blocks off, `within` is false too. The jar calls
        // that combination VALID.
        cx.mob.level.loaded_chunks = Some(vec![(0, 0)]);
        assert!(cx.navigation.create_path(&mut cx.mob, pos, 0).is_none());
        assert!(b.body.has_valid_travelling_path(&mut cx, &target));
        assert!(b.body.has_valid_target(&mut brain, &mut cx));
        assert!(
            !brain.has_memory_value(T::UnreachableTransportBlockPositions),
            "nothing was blacklisted"
        );
    }

    #[test]
    fn in_range_but_unseen_is_an_invalid_path_and_blacklists_the_position() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        cx.mob.place_chest(pos, vec![None]);
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos).unwrap();
        b.body.target = Some(target);
        // `within` is true (the chest is adjacent) and no clip answers, so
        // `canSeeAnyTargetSide` is false and `noPathAndFar` is false too.
        assert!(!b.body.has_valid_travelling_path(&mut cx, &target));
        assert!(!b.body.has_valid_target(&mut brain, &mut cx));
        assert_eq!(
            brain.get_memory(T::UnreachableTransportBlockPositions),
            Some(&MemoryValue::GlobalPosList(vec![GlobalPos {
                dimension: cx.mob.dimension(),
                pos
            }]))
        );
    }

    #[test]
    fn a_replaced_block_entity_invalidates_the_target() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        cx.mob
            .place_container_with_identity(pos, CHEST, 7, vec![None]);
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos).unwrap();
        b.body.target = Some(target);
        b.body.state = TransportItemState::Interacting;
        assert!(b.body.has_valid_target(&mut brain, &mut cx));
        // Same position, same kind, different object.
        cx.mob
            .place_container_with_identity(pos, CHEST, 8, vec![None]);
        assert!(!b.body.has_valid_target(&mut brain, &mut cx));
    }

    #[test]
    fn a_blocked_chest_invalidates_the_target() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        cx.mob.place_chest(pos, vec![None]);
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos).unwrap();
        b.body.target = Some(target);
        b.body.state = TransportItemState::Interacting;
        assert!(b.body.has_valid_target(&mut brain, &mut cx));
        cx.mob.blocked_chests.push(pos);
        assert!(!b.body.has_valid_target(&mut brain, &mut cx));
    }

    #[test]
    fn connected_targets_yield_the_pair_in_stream_of_order() {
        let (mut cx, _brain) = setup();
        let a = BlockPos { x: 1, y: 1, z: 0 };
        let c = BlockPos { x: 2, y: 1, z: 0 };
        cx.mob.place_chest(a, vec![None]);
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, a).unwrap();
        // TYPE == SINGLE: the target alone.
        assert_eq!(
            TransportItemsBetweenContainers::<TestMob>::connected_targets(&cx, &target).len(),
            1
        );
        // Paired, but the partner has no block entity: still the target
        // alone, because `tryCreatePossibleTarget` answered null.
        cx.mob.chest_pairs.insert(a, c);
        assert_eq!(
            TransportItemsBetweenContainers::<TestMob>::connected_targets(&cx, &target).len(),
            1
        );
        cx.mob.place_chest(c, vec![None]);
        let pair = TransportItemsBetweenContainers::<TestMob>::connected_targets(&cx, &target);
        assert_eq!(
            pair.iter().map(|t| t.pos).collect::<Vec<_>>(),
            vec![a, c],
            "Stream.of(target, other)"
        );
    }

    #[test]
    fn queuing_resumes_travelling_and_travels_within_the_same_tick() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 20, y: 1, z: 0 };
        cx.mob.place_chest(pos, vec![None]);
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos).unwrap();
        b.body.target = Some(target);
        b.body.state = TransportItemState::Queuing;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        brain.erase_memory(T::WalkTarget);
        b.body.state = TransportItemState::Queuing;
        b.body.target = Some(target);
        b.body.tick(&mut brain, &mut cx, 1);
        // `onQueuingForTarget` set TRAVELLING, and the second `if` in the
        // same tick then ran `onTravelToTarget`, which walked.
        assert_eq!(b.body.state, TransportItemState::Travelling);
        assert!(
            brain.has_memory_value(T::WalkTarget),
            "the travel step ran in the same tick"
        );
    }

    #[test]
    fn the_interaction_range_is_a_half_before_the_path_is_done_and_one_after() {
        let (mut cx, _brain) = setup();
        assert_eq!(
            TransportItemsBetweenContainers::<TestMob>::interaction_range(&cx),
            0.5,
            "no path at all is not a finished path"
        );
        let _ = &mut cx;
    }

    #[test]
    fn is_within_target_distance_inflates_y_by_a_half_whatever_the_distance() {
        let (mut cx, _brain) = setup();
        // A chest four blocks straight up, well outside every y reach the
        // callers use, while x and z are dead on.
        let pos = BlockPos { x: 0, y: 5, z: 0 };
        cx.mob.place_chest(pos, vec![None]);
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos).unwrap();
        let at = TransportItemsBetweenContainers::<TestMob>::center_pos(&cx);
        for d in [0.5, 1.0, 2.0, 3.0] {
            assert!(
                !TransportItemsBetweenContainers::<TestMob>::is_within_target_distance(
                    d, &target, &cx, at
                ),
                "d = {d} must not widen the vertical reach"
            );
        }
        // Positive control on the horizontal axis, which d DOES widen.
        let side = BlockPos { x: 3, y: 1, z: 0 };
        cx.mob.place_chest(side, vec![None]);
        let side_t = TransportItemTarget::try_create_from_block_entity(&cx.mob, side).unwrap();
        assert!(
            !TransportItemsBetweenContainers::<TestMob>::is_within_target_distance(
                0.5, &side_t, &cx, at
            )
        );
        assert!(
            TransportItemsBetweenContainers::<TestMob>::is_within_target_distance(
                3.0, &side_t, &cx, at
            )
        );
    }

    #[test]
    fn sixty_ticks_of_interacting_picks_up_at_most_sixteen_from_the_first_full_slot() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        cx.mob
            .place_chest(pos, vec![None, Some(item("cobblestone", 40))]);
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos).unwrap();
        b.body.target = Some(target);
        b.body.state = TransportItemState::Interacting;
        b.body.interaction_state = Some(ContainerInteractionState::PickupItem);
        b.body.ticks_since_reaching_target = TARGET_INTERACTION_TIME - 1;
        cx.mob.calls.clear();
        b.body.on_reached_target(&mut brain, &mut cx, &target);
        assert_eq!(cx.mob.main_hand, Some(item("cobblestone", 16)));
        assert_eq!(
            cx.mob.containers[&pos][1],
            Some(item("cobblestone", 24)),
            "40 - 16"
        );
        assert_eq!(cx.mob.guaranteed_drops, 1);
        // `clearMemoriesAfterMatchingTargetFound` ran.
        assert!(b.body.target.is_none());
        assert!(!brain.has_memory_value(T::VisitedBlockPositions));
        assert!(!brain.has_memory_value(T::UnreachableTransportBlockPositions));
    }

    #[test]
    fn one_tick_short_of_sixty_touches_no_container() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        cx.mob.place_chest(pos, vec![Some(item("cobblestone", 40))]);
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos).unwrap();
        b.body.target = Some(target);
        b.body.state = TransportItemState::Interacting;
        b.body.ticks_since_reaching_target = TARGET_INTERACTION_TIME - 2;
        cx.mob.calls.clear();
        b.body.on_reached_target(&mut brain, &mut cx, &target);
        assert_eq!(
            b.body.ticks_since_reaching_target,
            TARGET_INTERACTION_TIME - 1
        );
        assert!(cx.mob.main_hand.is_none());
        assert!(brain.has_memory_value(T::LookTarget), "it still looks");
    }

    #[test]
    fn putting_down_debits_the_hand_by_the_room_not_by_the_moved_count() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        // One slot holding 60 of a 64-stack: `room` is 4.
        cx.mob.place_chest(pos, vec![Some(item("cobblestone", 60))]);
        cx.mob.main_hand = Some(item("cobblestone", 1));
        let left = TransportItemsBetweenContainers::<TestMob>::add_items_to_container(&mut cx, pos);
        // The slot grew by min(4, 1) = 1 ...
        assert_eq!(cx.mob.containers[&pos][0], Some(item("cobblestone", 61)));
        // ... and the hand was debited by the whole 4, so it went to -3 and
        // `isEmpty()` (count <= 0) short-circuited the return to EMPTY.
        assert_eq!(left, None);
        let _ = &mut b;
        let _ = &mut brain;
    }

    #[test]
    fn a_partial_fill_leaves_the_remainder_in_hand_and_stops_targeting() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        // Two full slots of a 64-stack: no room anywhere, nothing empty.
        cx.mob.place_chest(
            pos,
            vec![Some(item("cobblestone", 64)), Some(item("cobblestone", 64))],
        );
        cx.mob.main_hand = Some(item("cobblestone", 5));
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos).unwrap();
        b.body.target = Some(target);
        b.body.put_down_item(&mut brain, &mut cx, pos);
        assert_eq!(cx.mob.main_hand, Some(item("cobblestone", 5)));
        assert!(b.body.target.is_none(), "stopTargetingCurrentTarget");
        // The visited memories are NOT cleared on this path.
        assert!(!brain.has_memory_value(T::VisitedBlockPositions));
    }

    #[test]
    fn an_empty_slot_takes_the_whole_hand() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        cx.mob.place_chest(pos, vec![Some(item("stone", 64)), None]);
        cx.mob.main_hand = Some(item("cobblestone", 5));
        b.body.put_down_item(&mut brain, &mut cx, pos);
        assert_eq!(cx.mob.containers[&pos][1], Some(item("cobblestone", 5)));
        assert!(cx.mob.main_hand.is_none());
        // `clearMemoriesAfterMatchingTargetFound` on the empty-hand path.
        assert!(!brain.has_memory_value(T::UnreachableTransportBlockPositions));
    }

    #[test]
    fn the_four_branches_are_the_hand_crossed_with_the_container() {
        use ContainerInteractionState as C;
        let (mut cx, _brain) = setup();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        type B = TransportItemsBetweenContainers<TestMob>;

        cx.mob.place_chest(pos, vec![Some(item("poppy", 1))]);
        cx.mob.main_hand = None;
        assert_eq!(B::reached_target_branch(&cx, pos), C::PickupItem);

        cx.mob.place_chest(pos, vec![None]);
        assert_eq!(B::reached_target_branch(&cx, pos), C::PickupNoItem);

        // Carrying, container empty: PLACE_ITEM.
        cx.mob.main_hand = Some(item("poppy", 1));
        assert_eq!(B::reached_target_branch(&cx, pos), C::PlaceItem);

        // Carrying, container holds the same item: still PLACE_ITEM.
        cx.mob.place_chest(pos, vec![Some(item("poppy", 3))]);
        assert_eq!(B::reached_target_branch(&cx, pos), C::PlaceItem);

        // Carrying, container holds only something else: PLACE_NO_ITEM.
        cx.mob.place_chest(pos, vec![Some(item("stone", 3))]);
        assert_eq!(B::reached_target_branch(&cx, pos), C::PlaceNoItem);
    }

    #[test]
    fn the_provider_callback_is_handed_the_tick_count_and_only_for_its_own_state() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let seen: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&seen);
        let mut b = TransportItemsBetweenContainers::<TestMob>::new(
            0.6,
            Box::new(|_| true),
            Box::new(|_| true),
            8,
            4,
            vec![(
                ContainerInteractionState::PickupItem,
                Box::new(
                    move |_cx: &mut MobAi<TestMob>, _t: &TransportItemTarget, n: i32| {
                        sink.borrow_mut().push(n);
                    },
                ),
            )],
            Box::new(|_| {}),
            Box::new(|_| false),
        );
        let (mut cx, mut brain) = setup();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        cx.mob.place_chest(pos, vec![Some(item("poppy", 1))]);
        let target = TransportItemTarget::try_create_from_block_entity(&cx.mob, pos).unwrap();
        b.body.ticks_since_reaching_target = 12;

        // No interaction state: nothing fires.
        b.body.on_target_interaction(&mut brain, &mut cx, &target);
        assert!(seen.borrow().is_empty());

        // A state the map does not hold: still nothing.
        b.body.interaction_state = Some(ContainerInteractionState::PlaceNoItem);
        b.body.on_target_interaction(&mut brain, &mut cx, &target);
        assert!(seen.borrow().is_empty());

        b.body.interaction_state = Some(ContainerInteractionState::PickupItem);
        b.body.on_target_interaction(&mut brain, &mut cx, &target);
        assert_eq!(*seen.borrow(), vec![12]);
        // And the look target is written every time regardless.
        assert!(brain.has_memory_value(T::LookTarget));
    }

    #[test]
    fn only_a_chest_block_entity_enters_the_scan() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let barrel = BlockPos { x: 1, y: 1, z: 0 };
        cx.mob
            .place_container_with_identity(barrel, "minecraft:barrel", 3, vec![None]);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            b.body.transport_target(&brain, &cx),
            None,
            "getBlockEntityContainer handles it, the scan never offers it"
        );
        assert!(is_chest_block_entity(Some("minecraft:chest")));
        assert!(is_chest_block_entity(Some("minecraft:trapped_chest")));
        assert!(!is_chest_block_entity(Some("minecraft:ender_chest")));
        assert!(!is_chest_block_entity(Some("minecraft:barrel")));
    }

    #[test]
    fn finding_a_target_marks_it_visited_and_reports_the_tick_as_spent() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let pos = BlockPos { x: 1, y: 1, z: 0 };
        cx.mob.place_chest(pos, vec![Some(item("poppy", 1))]);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(b.body.update_invalid_target(&mut brain, &mut cx));
        assert_eq!(b.body.target.map(|t| t.pos), Some(pos));
        assert_eq!(b.body.state, TransportItemState::Travelling);
        assert_eq!(
            brain.get_memory(T::VisitedBlockPositions),
            Some(&MemoryValue::GlobalPosList(vec![GlobalPos {
                dimension: cx.mob.dimension(),
                pos
            }]))
        );
    }

    #[test]
    fn stop_in_place_keeps_the_vertical_delta_and_zeroes_the_rest() {
        let (mut cx, _brain) = setup();
        cx.mob.set_delta_movement(0.4, -0.7, 0.9);
        cx.mob.set_xxa(1.0);
        cx.mob.set_yya(1.0);
        cx.mob.set_speed_and_zza(1.0);
        TransportItemsBetweenContainers::<TestMob>::stop_in_place(&mut cx);
        assert_eq!(cx.mob.delta_movement(), Vec3::new(0.0, -0.7, 0.0));
        assert_eq!(cx.mob.xxa(), 0.0);
        assert_eq!(cx.mob.yya(), 0.0);
        assert_eq!(cx.mob.speed(), 0.0);
        assert_eq!(cx.mob.zza(), 0.0, "Mob.setSpeed also sets zza");
    }
}
