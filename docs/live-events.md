# Live branch event evidence

Git Buoy compares consecutive repository surveys. It labels each observed move
with the strongest claim supported by the current branch tip, commit topology,
ahead/behind counts, and local reflogs.

| Case | Evidence between surveys | Label |
| --- | --- | --- |
| Normal commit | The local tip changed; the newest branch reflog entry targets that tip and records `commit`; the tip has at most one parent. | **committed** |
| Merge commit | The local tip changed to a multi-parent commit, and the matching branch reflog entry records `merge`, `pull`, or `commit` (including a manually completed merge). Both provenance and topology are required. | **merged** |
| Fast-forward pull or merge | The local tip changed; the matching reflog entry records `pull` or `merge`, but the new tip has only one parent. | **updated** |
| Squash merge | Git records the resulting single-parent tip as an ordinary `commit`; after the commit, local evidence cannot distinguish it reliably from another commit. | **committed** |
| Rebase, reset, force update, or another branch move | The local tip changed without matching commit evidence, or its reflog action is unclassified. | **updated** |
| Push observed by this repository | The local tip did not change; the same upstream reference changed; its matching reflog entry records `update by push`; and the ahead count decreased. | **pushed** |
| Upstream-only movement | The local tip did not change and the same upstream reference moved, but the full push evidence above is absent. This includes fetches and ambiguous ahead-count decreases. | **updated** |

Events last for 12 seconds from the survey that first observes them. Repeated
surveys of the same tips do not replay or extend an event. The initial survey
only establishes a baseline, so existing history is never emitted as live
activity.

Polling can observe only the state and latest matching reflog entries available
at survey time. Several moves between surveys may collapse into one neutral
update. Git Buoy is therefore neither a historical log nor a remote audit
trail, and it does not attribute an event to a person or process.
