# Platform overrides and domain mutations reported as parameter or out-parameter defects

**Status:** `too_many_parameters` part fixed after 1.5.0 (2d32b8e,
unreleased); `out_parameter` part open. **Observed:** 2026-10-30 with
Engineering Auditor 1.5.0 in Java Minecraft (Fabric) mods; `out_parameter`
reproduced again on 2026-11-18 at 5513420.

Two patterns in framework-driven game code drew findings that the code could
not act on:

1. `too_many_parameters` on methods whose signature the platform dictates:
   overrides of platform classes and Mixin injector callbacks.
2. `out_parameter` on methods that update a live game entity, or that pass
   scratch arrays down a recursive search.

The examples below are reconstructions with the downstream shapes; they
contain no downstream source.

## 1. Platform Overrides And Mixin Callbacks (`too_many_parameters`)

An item class overrides a seven-parameter method of a platform base class:

```java
public final class LongBowItem extends BowItem {
    @Override
    protected void shootProjectile(LivingEntity shooter, Projectile projectile,
            int index, float velocity, float divergence, float roll,
            @Nullable LivingEntity target) {
        super.shootProjectile(shooter, projectile, index, velocity, divergence, roll, target);
    }
}
```

A Mixin injector must take its target method's parameters, plus the
callback object:

```java
@Inject(method = "queueFunctions", at = @At("HEAD"))
private static void beginTiming(Collection<CommandFunction<CommandSourceStack>> functions,
        Object parent, CommandSourceStack source, ExecutionContext<CommandSourceStack> context,
        Frame frame, TraceCallbacks tracer, boolean returnResult,
        CallbackInfoReturnable<Integer> cir) {
}
```

1.5.0 reported both (`Function shootProjectile takes 7 parameters (> 5).`).
Neither signature is the method's choice: the override must match the
supertype, and the Mixin transformer rejects an injector whose descriptor
does not match its target.

**Resolution.** 2d32b8e skips overrides (Java `@Override`; Kotlin, C#, and
TypeScript `override`), whose supertype's declaration carries the finding
where the repository declares it. It also skips Mixin and MixinExtras
injectors in a file importing the Mixin library. A javax or Guice `@Inject`
method is still measured.

## 2. Entity Mutations And Scratch Arrays (`out_parameter`)

A helper debits experience from the platform's player entity:

```java
import net.minecraft.world.entity.player.Player;

public final class Xp {
    public static void debitExperiencePoints(Player player, int points) {
        player.experienceLevel = Math.max(0, player.experienceLevel - points);
    }
}
```

A recursive bipartite-matching helper reuses its visited and assignment
arrays across calls instead of allocating per step:

```java
private static boolean assign(int inputIndex, List<Set<Integer>> validSlotsPerInput,
        boolean[] visitedSlots, int[] inputForSlot) {
    for (int slot : validSlotsPerInput.get(inputIndex)) {
        if (!visitedSlots[slot]) {
            visitedSlots[slot] = true;
            if (inputForSlot[slot] == -1
                    || assign(inputForSlot[slot], validSlotsPerInput, visitedSlots, inputForSlot)) {
                inputForSlot[slot] = inputIndex;
                return true;
            }
        }
    }
    return false;
}
```

Both are still reported at 5513420:

> `Function debitExperiencePoints writes through parameter player.`
> `Function assign writes through parameter visitedSlots.`
> `Function assign writes through parameter inputForSlot.`

A live game entity is a stateful world object owned by the game loop.
Updating it is the helper's purpose, and the caller cannot take back a
modified copy instead. The scratch arrays are a deliberate zero-allocation
choice on a path that runs whenever a crafting grid changes.

A write through a cast to an accessor interface
(`((HookAccessor) hook).setSyncVelocity(true)`) is not reported.

## Suggested Follow-Up

- Read a write to a field of an imported platform entity type (for example
  one under `net.minecraft.world.entity`) as an entity update, not an out
  parameter.
- Recognize visited and assignment arrays that a private recursive helper
  passes unchanged to its own recursive call.
- Keep reporting a write through a parameter of a repository value type that
  the caller could receive as a return value instead.
