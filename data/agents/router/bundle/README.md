# router — bundle

Channel-level dispatcher. Reads a trigger, decides the next actor,
hands off. Stateless across turns aside from reading existing
artifacts on the channel.

- Consumes: the trigger event + any existing channel artifacts.
- Produces: a single `joi event append --handoff <actor_id>` (or a
  direct reply for trivial questions).
- Never produces business artifacts itself.

Provider: `claude` via `interactive_command`. Standard envelope.
