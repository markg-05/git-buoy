# Git Buoy Interface System

## Direction and feel

Git Buoy is a calm, precise terminal harbor for developers monitoring live repository activity from a spare terminal pane. The harbor metaphor must communicate real Git state rather than decorate a conventional status view. Ambient cues should be noticeable at a glance, settle quickly, and leave exact details available through inspect mode.

Use the product's physical vocabulary consistently: harbor, dock, vessel, cargo, mooring, shipping lane, wake, and signal. Prefer plain Git terminology whenever the metaphor would reduce comprehension.

## Visual system

- Keep terminal-native typography and a one-character-cell spacing grid.
- Use character-cell layering and pier lines for structure; do not introduce card-like surfaces, shadows, gradients, or ornamental borders.
- Draw colors only from the existing theme roles: water, pier, text hierarchy, cargo/loading, sealed, outbound, and blocked. Color must communicate state, never decoration.
- Reuse the established vessel, cargo, wake, mooring, and event glyph language. Add a glyph only when it carries a distinct repository fact and has a narrow-terminal fallback.

## Motion patterns

- Update the scene and inspect data to the newest repository truth immediately. Animation explains the change; it never delays the state.
- Use one primary bounded cue per dock. Priority is lane blocked/cleared, vessel arrival/departure, then cargo change. Independent commit, push, and merge events may remain alongside it.
- Derive all progress from the deterministic frame clock. Standard state-change cues last 750 ms and settle without looping, bounce, or spring motion.
- Give motion semantic direction: cargo loads or unloads monotonically, arriving vessels travel toward the pier, and departing vessels travel toward open water.
- Signal lane changes by briefly emphasizing the truthful target condition. In compact layouts, use the same bounded status emphasis as the fallback for every cue.
- Reduced motion collapses cues immediately to their final state and never replays them when motion is restored.

## Interaction and accessibility

- Ambient mode communicates orientation; inspect mode always exposes exact paths, counts, conditions, and repository details.
- Do not rely on color alone. Preserve condition words, cargo glyphs, arrows, activity labels, and emphasis modifiers for limited-color terminals.
- Keep animation useful but nonessential: the static frame must remain accurate and understandable.
